//! Firebase ID token verification (§9.3).
//!
//! Google publishes the securetoken signing keys as X.509 certificates keyed by `kid`.
//! We cache them for as long as the `Cache-Control: max-age` allows, extract the RSA
//! public key from each certificate, and verify `iss`, `aud`, `exp`, `iat` and
//! `auth_time` before the token is allowed to identify anyone.

use std::collections::HashMap;
use std::sync::Arc;

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
        validation.set_audience(&audiences);
        validation.set_required_spec_claims(&["exp", "iat", "aud", "iss", "sub"]);
        validation.leeway = CLOCK_SKEW_SECS;

        let decoded = decode::<FirebaseClaims>(token, &key, &validation).map_err(|error| {
            use jsonwebtoken::errors::ErrorKind;
            match error.kind() {
                ErrorKind::ExpiredSignature => ApiError::new(ErrorCode::TokenExpired),
                _ => ApiError::new(ErrorCode::TokenInvalid).internal(error.to_string()),
            }
        })?;

        let claims = decoded.claims;
        if claims.sub.trim().is_empty() {
            return Err(ApiError::new(ErrorCode::TokenInvalid).internal("empty sub"));
        }

        let auth_time = timestamp(claims.auth_time).ok_or_else(|| {
            ApiError::new(ErrorCode::TokenInvalid).internal("auth_time out of range")
        })?;

        // A token cannot be signed before the sign-in it describes.
        if auth_time > Utc::now() + chrono::Duration::seconds(CLOCK_SKEW_SECS as i64) {
            return Err(
                ApiError::new(ErrorCode::TokenInvalid).internal("auth_time is in the future")
            );
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
            issued_at: timestamp(claims.iat).unwrap_or_else(Utc::now),
            expires_at: timestamp(claims.exp).unwrap_or_else(Utc::now),
        })
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
