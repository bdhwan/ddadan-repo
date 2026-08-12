//! Kakao's OIDC discovery document, JWKS, token endpoint and `id_token` checks
//! (§9.2 steps 4–5).
//!
//! Two things here are worth reading twice.
//!
//! **The key cache refreshes exactly once per verification.** §9.2-4 says to cache
//! discovery and JWKS but refresh on a `kid` miss. "Once" is the whole instruction: a
//! `kid` we do not know is either a rotation (one refetch fixes it forever) or a forged
//! header (no number of refetches will ever fix it), and an unbounded retry turns the
//! second case into an outbound request amplifier pointed at Kakao.
//!
//! **[`KAKAO_ISSUER`] is not configurable.** `COUPON_KAKAO_OIDC_BASE_URL` moves where we
//! *fetch* from so a contract mock can stand in for Kakao, but the `iss` an `id_token`
//! must carry stays `https://kauth.kakao.com` either way. That is what makes the mock
//! worth testing against: it exercises the real issuer check rather than a copy of it.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::{ApiError, ApiResult, ErrorCode};

use super::{KAKAO_ISSUER, KakaoIdentity};

/// How long a fetched discovery document and key set stay usable without a refetch.
const CACHE_TTL_SECS: i64 = 3600;
/// Clock skew tolerated on `exp` / `iat`, matching the Firebase verifier.
const CLOCK_SKEW_SECS: u64 = 60;
/// Kakao is a third party on the critical path of a login. Waiting longer than this
/// helps nobody: the user is staring at a spinner and §6.1 would rather tell them to
/// try again.
const HTTP_TIMEOUT_SECS: u64 = 5;

/// The subset of the discovery document we act on.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kid: String,
    /// RSA modulus, base64url.
    n: String,
    /// RSA exponent, base64url.
    e: String,
    #[serde(default)]
    kty: String,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    iss: String,
    aud: String,
    sub: String,
    exp: i64,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    email: Option<String>,
    /// Kakao omits this when the user did not consent to sharing their email.
    #[serde(default)]
    email_verified: Option<bool>,
}

/// Kakao's token response.
///
/// It also carries `access_token` and `refresh_token`. This struct has no field for
/// either, on purpose: §9.2 requires them discarded once login completes, and a value
/// that was never deserialised cannot be logged, stored or forwarded by accident.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub id_token: String,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

struct Cached<T> {
    value: T,
    expires_at: DateTime<Utc>,
}

pub struct KakaoOidc {
    base_url: String,
    http: reqwest::Client,
    discovery: RwLock<Option<Cached<Discovery>>>,
    keys: RwLock<Option<Cached<HashMap<String, DecodingKey>>>>,
}

impl std::fmt::Debug for KakaoOidc {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KakaoOidc")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl KakaoOidc {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
                .build()
                .expect("reqwest client builds with rustls"),
            discovery: RwLock::new(None),
            keys: RwLock::new(None),
        }
    }

    /// The discovery document, from cache when it is still fresh.
    pub async fn discovery(&self) -> ApiResult<Discovery> {
        if let Some(cached) = self.discovery.read().await.as_ref() {
            if cached.expires_at > Utc::now() {
                return Ok(cached.value.clone());
            }
        }

        let url = format!("{}/.well-known/openid-configuration", self.base_url);
        let document: Discovery = self.get_json(&url).await?;

        // A discovery document that names somebody else as the issuer is not Kakao's,
        // whatever host served it.
        if document.issuer != KAKAO_ISSUER {
            return Err(ApiError::new(ErrorCode::DependencyUnavailable).internal(format!(
                "kakao discovery declares issuer {}, expected {KAKAO_ISSUER}",
                document.issuer
            )));
        }

        *self.discovery.write().await = Some(Cached {
            value: document.clone(),
            expires_at: Utc::now() + Duration::seconds(CACHE_TTL_SECS),
        });
        Ok(document)
    }

    /// Trade an authorization code for an `id_token` (§9.2-3).
    ///
    /// The Kakao access and refresh tokens in the same response are dropped on the floor
    /// — see [`TokenResponse`].
    pub async fn exchange_code(
        &self,
        client_id: &str,
        client_secret: Option<&str>,
        redirect_uri: &str,
        code: &str,
        code_verifier: &str,
    ) -> ApiResult<TokenResponse> {
        let discovery = self.discovery().await?;

        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("code", code),
            ("code_verifier", code_verifier),
        ];
        if let Some(secret) = client_secret {
            form.push(("client_secret", secret));
        }

        let response = self
            .http
            .post(&discovery.token_endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("kakao token request failed: {error}"))
            })?;

        let status = response.status();
        let body = response.bytes().await.map_err(|error| {
            ApiError::new(ErrorCode::DependencyUnavailable)
                .internal(format!("kakao token response unreadable: {error}"))
        })?;

        if status.is_success() {
            return serde_json::from_slice(&body).map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("kakao token response not understood: {error}"))
            });
        }

        Err(classify_token_error(status, &body))
    }

    /// Verify an `id_token` against Kakao's keys and the claims §9.2-5 names.
    pub async fn verify_id_token(
        &self,
        id_token: &str,
        audience: &str,
        expected_nonce: &str,
    ) -> ApiResult<KakaoIdentity> {
        let header = decode_header(id_token).map_err(|error| {
            ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                .internal(format!("id_token header unreadable: {error}"))
        })?;

        // Kakao signs with RS256. Accepting anything else would let the caller choose a
        // weaker algorithm, or `none`.
        if header.alg != Algorithm::RS256 {
            return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                .internal(format!("unexpected id_token algorithm {:?}", header.alg)));
        }
        let kid = header.kid.ok_or_else(|| {
            ApiError::new(ErrorCode::KakaoSecurityCheckFailed).internal("id_token has no kid")
        })?;

        let key = self.decoding_key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[KAKAO_ISSUER]);
        validation.set_audience(&[audience]);
        validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
        validation.leeway = CLOCK_SKEW_SECS;

        let claims = decode::<IdTokenClaims>(id_token, &key, &validation)
            .map_err(|error| {
                ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                    .internal(format!("id_token rejected: {error}"))
            })?
            .claims;

        validate_claims(claims, audience, expected_nonce)
    }

    /// The key for `kid`, refetching the key set **once** if we have not seen it.
    async fn decoding_key(&self, kid: &str) -> ApiResult<DecodingKey> {
        if let Some(cached) = self.keys.read().await.as_ref() {
            if cached.expires_at > Utc::now() {
                if let Some(key) = cached.value.get(kid) {
                    return Ok(key.clone());
                }
                tracing::info!(kid, "kakao jwks miss; refreshing once");
            }
        }

        let keys = self.refresh_keys().await?;
        keys.get(kid).cloned().ok_or_else(|| {
            // Still absent after a refresh. Either the token was not signed by Kakao or
            // it names a key Kakao itself no longer publishes; both are refusals, and
            // asking again would only make us a load generator.
            ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                .internal(format!("unknown kakao signing key {kid}"))
        })
    }

    async fn refresh_keys(&self) -> ApiResult<HashMap<String, DecodingKey>> {
        let discovery = self.discovery().await?;
        let jwks: Jwks = self.get_json(&discovery.jwks_uri).await?;

        let mut keys = HashMap::with_capacity(jwks.keys.len());
        for jwk in jwks.keys {
            if !jwk.kty.is_empty() && jwk.kty != "RSA" {
                continue;
            }
            match DecodingKey::from_rsa_components(&jwk.n, &jwk.e) {
                Ok(key) => {
                    keys.insert(jwk.kid, key);
                }
                // One unusable key must not take verification down for the rest.
                Err(error) => {
                    tracing::warn!(kid = jwk.kid, %error, "skipping unusable kakao jwk")
                }
            }
        }

        if keys.is_empty() {
            return Err(ApiError::new(ErrorCode::DependencyUnavailable)
                .internal("kakao jwks contained no usable keys"));
        }

        *self.keys.write().await = Some(Cached {
            value: keys.clone(),
            expires_at: Utc::now() + Duration::seconds(CACHE_TTL_SECS),
        });
        Ok(keys)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> ApiResult<T> {
        self.http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("GET {url} failed: {error}"))
            })?
            .json()
            .await
            .map_err(|error| {
                ApiError::new(ErrorCode::DependencyUnavailable)
                    .internal(format!("GET {url} returned unreadable JSON: {error}"))
            })
    }
}

/// Turn a non-2xx token response into one of the three failures §6.1 lets us name.
fn classify_token_error(status: reqwest::StatusCode, body: &[u8]) -> ApiError {
    let parsed: Option<TokenError> = serde_json::from_slice(body).ok();
    let code = parsed.as_ref().map(|e| e.error.as_str()).unwrap_or_default();
    let description = parsed
        .as_ref()
        .and_then(|e| e.error_description.clone())
        .unwrap_or_default();
    let detail = format!("kakao token endpoint {status}: {code} {description}");

    match code {
        // The user said no on the consent screen and the SPA followed the redirect
        // anyway. Not an error to show as one (§6.1 취소).
        "access_denied" | "consent_required" => {
            ApiError::new(ErrorCode::KakaoLoginCancelled).internal(detail)
        }
        // A code that was already spent, expired, or never ours. §6.1 보안 검증 실패.
        "invalid_grant" | "invalid_request" | "invalid_client" => {
            ApiError::new(ErrorCode::KakaoSecurityCheckFailed).internal(detail)
        }
        // Anything else — including every 5xx — is Kakao having a bad minute, which the
        // user can act on by trying again (§6.1 일시 장애).
        _ => ApiError::new(ErrorCode::DependencyUnavailable).internal(detail),
    }
}

/// The claim checks §9.2-5 names, in one place so both halves of a future refactor keep
/// running all of them.
///
/// `iss`, `aud` and `exp` were already enforced by `jsonwebtoken`'s `Validation`. They
/// are re-checked here for the same reason [`crate::auth::firebase`] does it: one
/// authority for "is this token for us" is worth a duplicated comparison.
fn validate_claims(
    claims: IdTokenClaims,
    audience: &str,
    expected_nonce: &str,
) -> ApiResult<KakaoIdentity> {
    if claims.iss != KAKAO_ISSUER {
        return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
            .internal(format!("unexpected id_token issuer {}", claims.iss)));
    }
    if claims.aud != audience {
        return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
            .internal(format!("unexpected id_token audience {}", claims.aud)));
    }
    if claims.sub.trim().is_empty() {
        return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed).internal("empty sub"));
    }

    // The nonce ties this token to the authorize request we started. Without the check a
    // token captured from another session would sign its holder in as somebody else.
    match claims.nonce.as_deref() {
        Some(nonce) if constant_time_eq(nonce, expected_nonce) => {}
        Some(_) => {
            return Err(
                ApiError::new(ErrorCode::KakaoSecurityCheckFailed).internal("id_token nonce differs")
            );
        }
        None => {
            return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                .internal("id_token carries no nonce"));
        }
    }

    if Utc::now().timestamp() - i64::from(CLOCK_SKEW_SECS as u32) > claims.exp {
        return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed).internal("id_token expired"));
    }

    Ok(KakaoIdentity {
        provider_subject: claims.sub,
        // §9.2: 이메일은 매핑 힌트일 뿐 계정 자동 병합 키가 아니다. Kakao also omits it
        // entirely when the user declined to share it, which AUTH-002 says must still be
        // a usable sign-up.
        email: claims.email,
        email_verified: claims.email_verified.unwrap_or(false),
    })
}

/// Compare two secrets without leaking their common prefix through timing.
fn constant_time_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(overrides: serde_json::Value) -> IdTokenClaims {
        let mut value = serde_json::json!({
            "iss": KAKAO_ISSUER,
            "aud": "kakao-rest-key",
            "sub": "3141592653",
            "exp": Utc::now().timestamp() + 600,
            "nonce": "the-nonce",
            "email": "someone@kakao.test",
            "email_verified": true,
        });
        for (key, override_value) in overrides.as_object().expect("object") {
            value[key] = override_value.clone();
        }
        serde_json::from_value(value).expect("claims")
    }

    #[test]
    fn a_well_formed_token_yields_the_provider_identity() {
        let identity =
            validate_claims(claims(serde_json::json!({})), "kakao-rest-key", "the-nonce")
                .expect("accepted");

        assert_eq!(identity.provider_subject, "3141592653");
        assert_eq!(identity.email.as_deref(), Some("someone@kakao.test"));
        assert!(identity.email_verified);
    }

    #[test]
    fn a_token_from_another_issuer_or_for_another_app_is_refused() {
        for wrong in [
            serde_json::json!({ "iss": "https://accounts.google.com" }),
            serde_json::json!({ "aud": "somebody-elses-key" }),
            serde_json::json!({ "sub": "  " }),
        ] {
            let error = validate_claims(claims(wrong.clone()), "kakao-rest-key", "the-nonce")
                .expect_err("must refuse");
            assert_eq!(error.code, ErrorCode::KakaoSecurityCheckFailed, "{wrong}");
        }
    }

    #[test]
    fn a_mismatched_or_missing_nonce_is_refused() {
        // Without this check a token lifted from another session would sign its holder
        // in as that session's owner.
        for wrong in [
            serde_json::json!({ "nonce": "somebody-elses-nonce" }),
            serde_json::json!({ "nonce": serde_json::Value::Null }),
        ] {
            let error = validate_claims(claims(wrong.clone()), "kakao-rest-key", "the-nonce")
                .expect_err("must refuse");
            assert_eq!(error.code, ErrorCode::KakaoSecurityCheckFailed, "{wrong}");
        }
    }

    #[test]
    fn an_expired_token_is_refused() {
        let error = validate_claims(
            claims(serde_json::json!({ "exp": Utc::now().timestamp() - 3600 })),
            "kakao-rest-key",
            "the-nonce",
        )
        .expect_err("must refuse");
        assert_eq!(error.code, ErrorCode::KakaoSecurityCheckFailed);
    }

    #[test]
    fn a_user_who_withheld_their_email_still_gets_an_identity() {
        // AUTH-002: 이메일 제공 동의가 없으면 이메일 없이 가입할 수 있다.
        let identity = validate_claims(
            claims(serde_json::json!({
                "email": serde_json::Value::Null,
                "email_verified": serde_json::Value::Null,
            })),
            "kakao-rest-key",
            "the-nonce",
        )
        .expect("accepted");

        assert_eq!(identity.email, None);
        assert!(!identity.email_verified);
    }

    #[test]
    fn the_three_kakao_failures_stay_distinguishable() {
        // §6.1 allows exactly three: 취소, 보안 검증 실패, 일시 장애.
        let cases = [
            (
                reqwest::StatusCode::BAD_REQUEST,
                r#"{"error":"access_denied"}"#,
                ErrorCode::KakaoLoginCancelled,
            ),
            (
                reqwest::StatusCode::BAD_REQUEST,
                r#"{"error":"invalid_grant","error_description":"authorization code not found"}"#,
                ErrorCode::KakaoSecurityCheckFailed,
            ),
            (
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":"server_error"}"#,
                ErrorCode::DependencyUnavailable,
            ),
            (
                reqwest::StatusCode::BAD_GATEWAY,
                "<html>proxy fell over</html>",
                ErrorCode::DependencyUnavailable,
            ),
        ];

        for (status, body, expected) in cases {
            assert_eq!(
                classify_token_error(status, body.as_bytes()).code,
                expected,
                "{status} {body}"
            );
        }
    }

    #[test]
    fn secret_comparison_does_not_short_circuit_on_length_or_content() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(constant_time_eq("", ""));
    }
}
