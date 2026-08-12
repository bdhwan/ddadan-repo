//! Firebase ID token verification (§9.3).
//!
//! Google publishes the securetoken signing keys as X.509 certificates keyed by `kid`.
//! We cache them for as long as the `Cache-Control: max-age` allows, extract the RSA
//! public key from each certificate, and verify `iss`, `aud`, `exp`, `iat` and
//! `auth_time` before the token is allowed to identify anyone.
//!
//! ## The emulator
//!
//! §20.1 puts the `local` environment on the Firebase Auth emulator, and §19.3 asks for the
//! email and token-lifetime flows to be exercised against it. The emulator mints tokens
//! that are byte-for-byte ordinary Firebase ID tokens in their claims and differ in exactly
//! one respect: the header says `alg: none` and the signature is empty, because there is no
//! signing key to publish.
//!
//! So the fork is drawn as narrowly as it can be. [`decode_emulator_claims`] and
//! [`FirebaseVerifier::decode_signed_claims`] answer only "are these bytes a token, and may
//! I read its claims" — everything that decides whether a *credential* is acceptable lives
//! in [`validate_claims`], which both paths run. A local run therefore exercises the
//! production issuer, audience, expiry and `auth_time` checks rather than a lookalike.
//!
//! `COUPON_FIREBASE_AUTH_EMULATOR_HOST` selects the emulator convention, and
//! `Config::validate` refuses to boot a production process that has it set.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, TimeZone, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::RwLock;
use x509_cert::Certificate;
use x509_cert::der::DecodePem;

use crate::config::Config;
use crate::error::{ApiError, ApiResult, ErrorCode};

/// Google's securetoken X.509 certificate endpoint.
const SECURETOKEN_X509_URL: &str =
    "https://www.googleapis.com/robot/v1/metadata/x509/securetoken@system.gserviceaccount.com";

/// Fallback cache lifetime when the response carries no usable `Cache-Control`.
const DEFAULT_JWKS_TTL_SECS: i64 = 3600;

/// Clock skew tolerated on `exp` / `iat`.
const CLOCK_SKEW_SECS: u64 = 60;

/// A credential we are willing to act on.
#[derive(Debug, Clone)]
pub struct VerifiedToken {
    pub firebase_uid: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: Option<String>,
    pub sign_in_provider: String,
    pub auth_time: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct FirebaseClaims {
    sub: String,
    /// Carried explicitly rather than left to `jsonwebtoken`'s `Validation`, because
    /// [`validate_claims`] is the one place both the signed and the emulator path check it.
    iss: String,
    aud: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
    #[serde(default)]
    name: Option<String>,
    auth_time: i64,
    iat: i64,
    exp: i64,
    #[serde(default)]
    firebase: FirebaseMetadata,
}

#[derive(Debug, Default, Deserialize)]
struct FirebaseMetadata {
    #[serde(default)]
    sign_in_provider: Option<String>,
}

struct CachedKeys {
    keys: HashMap<String, DecodingKey>,
    expires_at: DateTime<Utc>,
}

pub struct FirebaseVerifier {
    config: Arc<Config>,
    http: reqwest::Client,
    cache: RwLock<Option<CachedKeys>>,
}

impl FirebaseVerifier {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("reqwest client builds with rustls"),
            cache: RwLock::new(None),
        }
    }

    pub async fn verify(&self, token: &str) -> ApiResult<VerifiedToken> {
        let issuer = self.config.firebase_issuer().ok_or_else(|| {
            ApiError::new(ErrorCode::ServiceUnavailable)
                .internal("COUPON_FIREBASE_PROJECT_ID is not configured")
        })?;
        let audiences = self.config.firebase_audiences();

        // The only fork: who vouches for the bytes. What the claims must say is decided
        // once, below, for both.
        let claims = match self.config.firebase_auth_emulator() {
            Some(_) => decode_emulator_claims(token)?,
            None => self.decode_signed_claims(token, &issuer, &audiences).await?,
        };

        validate_claims(claims, &issuer, &audiences)
    }

    /// Read the claims out of a token Google signed, verifying the signature first.
    async fn decode_signed_claims(
        &self,
        token: &str,
        issuer: &str,
        audiences: &[String],
    ) -> ApiResult<FirebaseClaims> {
        let header = decode_header(token)
            .map_err(|error| ApiError::new(ErrorCode::TokenInvalid).internal(error.to_string()))?;

        // Firebase signs ID tokens with RS256. Accepting anything else would let a
        // caller pick a weaker algorithm — or `none`.
        if header.alg != Algorithm::RS256 {
            return Err(ApiError::new(ErrorCode::TokenInvalid)
                .internal(format!("unexpected token algorithm {:?}", header.alg)));
        }
        let kid = header.kid.ok_or_else(|| {
            ApiError::new(ErrorCode::TokenInvalid).internal("token header has no kid")
        })?;

        let key = self.decoding_key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[issuer]);
        validation.set_audience(audiences);
        validation.set_required_spec_claims(&["exp", "iat", "aud", "iss", "sub"]);
        validation.leeway = CLOCK_SKEW_SECS;

        let decoded = decode::<FirebaseClaims>(token, &key, &validation).map_err(|error| {
            use jsonwebtoken::errors::ErrorKind;
            match error.kind() {
                ErrorKind::ExpiredSignature => ApiError::new(ErrorCode::TokenExpired),
                _ => ApiError::new(ErrorCode::TokenInvalid).internal(error.to_string()),
            }
        })?;

        Ok(decoded.claims)
    }

    async fn decoding_key(&self, kid: &str) -> ApiResult<DecodingKey> {
        if let Some(cached) = self.cache.read().await.as_ref() {
            if cached.expires_at > Utc::now() {
                if let Some(key) = cached.keys.get(kid) {
                    return Ok(key.clone());
                }
                // A `kid` we have never seen means Google rotated; fall through and
                // refetch rather than rejecting a valid token.
            }
        }

        let keys = self.refresh_keys().await?;
        keys.keys.get(kid).cloned().ok_or_else(|| {
            ApiError::new(ErrorCode::TokenInvalid).internal(format!("unknown signing key {kid}"))
        })
    }

    async fn refresh_keys(&self) -> ApiResult<CachedKeys> {
        let response = self
            .http
            .get(SECURETOKEN_X509_URL)
            .send()
            .await
            .map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("securetoken key fetch failed: {error}"))
            })?;

        let ttl = cache_max_age(
            response
                .headers()
                .get(reqwest::header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
        );

        let certificates: HashMap<String, String> = response
            .error_for_status()
            .map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("securetoken key fetch failed: {error}"))
            })?
            .json()
            .await
            .map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("securetoken key payload unreadable: {error}"))
            })?;

        let mut keys = HashMap::with_capacity(certificates.len());
        for (kid, pem) in certificates {
            match decoding_key_from_certificate(&pem) {
                Ok(key) => {
                    keys.insert(kid, key);
                }
                // One unparsable certificate must not take down verification for the
                // others; log it and carry on.
                Err(error) => tracing::warn!(kid, error, "skipping unparsable securetoken cert"),
            }
        }

        if keys.is_empty() {
            return Err(ApiError::new(ErrorCode::DependencyUnavailable)
                .internal("securetoken returned no usable keys"));
        }

        let cached = CachedKeys {
            keys: keys.clone(),
            expires_at: Utc::now() + chrono::Duration::seconds(ttl),
        };
        *self.cache.write().await = Some(CachedKeys {
            keys,
            expires_at: cached.expires_at,
        });
        Ok(cached)
    }
}

/// Read the claims out of a token the Auth emulator minted (§20.1 `local`).
///
/// The emulator's convention is `alg: none` with an empty signature. Both are demanded
/// here: an RS256 token arriving at a process configured for the emulator has no key to be
/// checked against, and silently reading its claims anyway would be the bypass this whole
/// arrangement exists to avoid.
fn decode_emulator_claims(token: &str) -> ApiResult<FirebaseClaims> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(ApiError::new(ErrorCode::TokenInvalid)
            .internal("token is not three dot-separated segments"));
    };

    #[derive(Deserialize)]
    struct EmulatorHeader {
        alg: String,
    }

    let header: EmulatorHeader = decode_segment(header)
        .map_err(|error| ApiError::new(ErrorCode::TokenInvalid).internal(error))?;

    if !header.alg.eq_ignore_ascii_case("none") {
        return Err(ApiError::new(ErrorCode::TokenInvalid).internal(format!(
            "the Auth emulator issues unsigned tokens; got alg {}",
            header.alg
        )));
    }
    if !signature.is_empty() {
        return Err(ApiError::new(ErrorCode::TokenInvalid)
            .internal("an unsigned token must carry an empty signature"));
    }

    decode_segment(payload).map_err(|error| ApiError::new(ErrorCode::TokenInvalid).internal(error))
}

fn decode_segment<T: for<'de> Deserialize<'de>>(segment: &str) -> Result<T, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(segment.trim_end_matches('='))
        .map_err(|error| format!("token segment is not base64url: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("token segment is not the expected JSON: {error}"))
}

/// Decide whether a set of claims is a credential we are willing to act on (§9.3).
///
/// Every check here runs for a real Firebase token and an emulator one alike. `iss` and
/// `aud` are re-checked rather than left to `jsonwebtoken`'s `Validation` on the signed
/// path: one authority for "is this token for us" is worth a duplicated comparison, and
/// the emulator path has no `Validation` to defer to.
fn validate_claims(
    claims: FirebaseClaims,
    issuer: &str,
    audiences: &[String],
) -> ApiResult<VerifiedToken> {
    if claims.iss != issuer {
        return Err(ApiError::new(ErrorCode::TokenInvalid)
            .internal(format!("unexpected issuer {}", claims.iss)));
    }
    if !audiences.contains(&claims.aud) {
        return Err(ApiError::new(ErrorCode::TokenInvalid)
            .internal(format!("unexpected audience {}", claims.aud)));
    }
    if claims.sub.trim().is_empty() {
        return Err(ApiError::new(ErrorCode::TokenInvalid).internal("empty sub"));
    }

    let now = Utc::now();
    let skew = chrono::Duration::seconds(CLOCK_SKEW_SECS as i64);

    let expires_at = timestamp(claims.exp)
        .ok_or_else(|| ApiError::new(ErrorCode::TokenInvalid).internal("exp out of range"))?;
    if expires_at + skew < now {
        return Err(ApiError::new(ErrorCode::TokenExpired));
    }

    let auth_time = timestamp(claims.auth_time)
        .ok_or_else(|| ApiError::new(ErrorCode::TokenInvalid).internal("auth_time out of range"))?;

    // A token cannot be signed before the sign-in it describes.
    if auth_time > now + skew {
        return Err(ApiError::new(ErrorCode::TokenInvalid).internal("auth_time is in the future"));
    }

    Ok(VerifiedToken {
        firebase_uid: claims.sub,
        email: claims.email,
        email_verified: claims.email_verified,
        display_name: claims.name,
        sign_in_provider: claims
            .firebase
            .sign_in_provider
            .unwrap_or_else(|| "unknown".to_owned()),
        auth_time,
        issued_at: timestamp(claims.iat).unwrap_or(now),
        expires_at,
    })
}

/// Pull the RSA public key out of a PEM X.509 certificate.
///
/// The `subject_public_key` bit string of an RSA `SubjectPublicKeyInfo` is exactly the
/// PKCS#1 `RSAPublicKey` DER that `jsonwebtoken` expects.
fn decoding_key_from_certificate(pem: &str) -> Result<DecodingKey, String> {
    let certificate = Certificate::from_pem(pem.as_bytes())
        .map_err(|error| format!("certificate is not valid PEM/DER: {error}"))?;

    let public_key = certificate
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| "public key bit string is not byte aligned".to_owned())?;

    Ok(DecodingKey::from_rsa_der(public_key))
}

/// Seconds from a `Cache-Control: public, max-age=NNN` header.
fn cache_max_age(header: Option<&str>) -> i64 {
    header
        .and_then(|value| {
            value.split(',').find_map(|directive| {
                directive
                    .trim()
                    .strip_prefix("max-age=")
                    .and_then(|seconds| seconds.trim().parse::<i64>().ok())
            })
        })
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_JWKS_TTL_SECS)
}

fn timestamp(seconds: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "ddadan-dev";

    fn issuer() -> String {
        format!("https://securetoken.google.com/{PROJECT}")
    }

    /// An unsigned token in exactly the shape the Auth emulator produces: header
    /// `{"alg":"none","typ":"JWT"}`, the claims, and an empty signature.
    fn emulator_token(claims: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        format!("{header}.{payload}.")
    }

    fn emulator_claims(overrides: serde_json::Value) -> serde_json::Value {
        let now = Utc::now().timestamp();
        let mut claims = serde_json::json!({
            "iss": issuer(),
            "aud": PROJECT,
            "sub": "zl83ezxANweG0x85oq2xVWoqD1Uj",
            "user_id": "zl83ezxANweG0x85oq2xVWoqD1Uj",
            "email": "probe@ddadan.test",
            "email_verified": false,
            "auth_time": now,
            "iat": now,
            "exp": now + 3600,
            "firebase": { "sign_in_provider": "password" },
        });
        for (key, value) in overrides.as_object().expect("overrides object") {
            claims[key] = value.clone();
        }
        claims
    }

    fn verify_emulator(claims: serde_json::Value) -> ApiResult<VerifiedToken> {
        let token = emulator_token(claims);
        validate_claims(
            decode_emulator_claims(&token)?,
            &issuer(),
            &[PROJECT.to_owned()],
        )
    }

    #[test]
    fn an_emulator_token_identifies_its_signer() {
        let verified = verify_emulator(emulator_claims(serde_json::json!({})))
            .expect("a well-formed emulator token is accepted");

        assert_eq!(verified.firebase_uid, "zl83ezxANweG0x85oq2xVWoqD1Uj");
        assert_eq!(verified.email.as_deref(), Some("probe@ddadan.test"));
        assert_eq!(verified.sign_in_provider, "password");
    }

    #[test]
    fn an_expired_emulator_token_is_reported_as_expired_not_merely_invalid() {
        // The client's retry depends on the distinction: TOKEN_EXPIRED means "refresh and
        // try again", TOKEN_INVALID means "sign in again".
        let error = verify_emulator(emulator_claims(serde_json::json!({
            "iat": Utc::now().timestamp() - 7200,
            "exp": Utc::now().timestamp() - 3600,
        })))
        .expect_err("an expired token is refused");
        assert_eq!(error.code, ErrorCode::TokenExpired);
    }

    #[test]
    fn a_token_for_another_project_is_refused() {
        for wrong in [
            serde_json::json!({ "aud": "someone-elses-project" }),
            serde_json::json!({ "iss": "https://securetoken.google.com/someone-elses-project" }),
        ] {
            let error = verify_emulator(emulator_claims(wrong.clone()))
                .expect_err("audience and issuer must match this project");
            assert_eq!(error.code, ErrorCode::TokenInvalid, "{wrong}");
        }
    }

    #[test]
    fn an_emulator_process_still_refuses_a_token_it_cannot_check() {
        // `alg: none` is the emulator's convention, not an invitation to skip the
        // signature on anything else that turns up.
        let claims = URL_SAFE_NO_PAD.encode(emulator_claims(serde_json::json!({})).to_string());
        let rs256 = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"abc","typ":"JWT"}"#);
        let none = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);

        for token in [
            format!("{rs256}.{claims}."),
            // Unsigned means unsigned: a signature nobody checked is worse than none.
            format!("{none}.{claims}.c2lnbmF0dXJl"),
            format!("{none}.{claims}"),
            "not-a-token".to_owned(),
        ] {
            let error = decode_emulator_claims(&token).expect_err("must refuse");
            assert_eq!(error.code, ErrorCode::TokenInvalid, "{token}");
        }
    }

    #[test]
    fn a_sign_in_from_the_future_is_refused() {
        let error = verify_emulator(emulator_claims(serde_json::json!({
            "auth_time": Utc::now().timestamp() + 3600,
        })))
        .expect_err("auth_time cannot precede the token");
        assert_eq!(error.code, ErrorCode::TokenInvalid);
    }

    #[test]
    fn cache_lifetime_follows_the_max_age_directive() {
        assert_eq!(
            cache_max_age(Some("public, max-age=19645, must-revalidate")),
            19645
        );
        assert_eq!(cache_max_age(Some("max-age=60")), 60);
    }

    #[test]
    fn a_missing_or_useless_max_age_falls_back_to_one_hour() {
        assert_eq!(cache_max_age(None), DEFAULT_JWKS_TTL_SECS);
        assert_eq!(cache_max_age(Some("no-store")), DEFAULT_JWKS_TTL_SECS);
        assert_eq!(cache_max_age(Some("max-age=0")), DEFAULT_JWKS_TTL_SECS);
        assert_eq!(cache_max_age(Some("max-age=abc")), DEFAULT_JWKS_TTL_SECS);
    }

    #[test]
    fn garbage_certificates_are_rejected_not_panicked_on() {
        assert!(decoding_key_from_certificate("not a certificate").is_err());
        assert!(
            decoding_key_from_certificate(
                "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----"
            )
            .is_err()
        );
    }

    #[test]
    fn epoch_seconds_convert_to_utc() {
        assert_eq!(
            timestamp(1_780_000_000).map(|time| time.to_rfc3339()),
            Some("2026-05-28T20:26:40+00:00".to_owned())
        );
    }
}
