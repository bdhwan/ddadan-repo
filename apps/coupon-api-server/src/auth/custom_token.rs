//! Firebase Custom Tokens (§9.2-7).
//!
//! Step 7 is the hinge of the whole Kakao flow: *whatever* route the user took to get
//! here — a brand new Kakao sign-up, a Kakao login they have used for months, or a Kakao
//! identity linked onto an account they originally made with a password — the token this
//! module mints carries **that member's canonical Firebase UID**. Nothing downstream has
//! to know which route was taken, because after this point there is only one identity.
//!
//! A custom token is a plain RS256 JWT signed with a Google service account key, with a
//! fixed `aud` naming the Identity Toolkit, and `uid` naming the Firebase user to sign
//! in as. Angular then calls `signInWithCustomToken` (§9.2-8) and gets an ordinary
//! Firebase session, which the rest of the API verifies exactly like any other (§9.3).
//!
//! Without a service account this module does not exist: [`CustomTokenSigner::from_config`]
//! returns `None`, and the endpoints that need one say so plainly rather than inventing
//! a credential.

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::Serialize;

use crate::config::Config;
use crate::error::{ApiError, ApiResult, ErrorCode};

/// The fixed `aud` every Firebase custom token carries.
pub const IDENTITY_TOOLKIT_AUDIENCE: &str =
    "https://identitytoolkit.googleapis.com/google.identity.identitytoolkit.v1.IdentityToolkit";

/// Firebase's own ceiling on `uid`.
const MAX_UID_LENGTH: usize = 128;

#[derive(Debug, Serialize)]
struct CustomTokenClaims<'a> {
    iss: &'a str,
    sub: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
    uid: &'a str,
}

/// A minted token and the moment it stops working.
#[derive(Debug, Clone)]
pub struct MintedCustomToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

pub struct CustomTokenSigner {
    service_account_email: String,
    key: EncodingKey,
    ttl: Duration,
}

impl std::fmt::Debug for CustomTokenSigner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key must never reach a log line.
        formatter
            .debug_struct("CustomTokenSigner")
            .field("service_account_email", &self.service_account_email)
            .field("ttl_secs", &self.ttl.num_seconds())
            .finish()
    }
}

/// Why a signer could not be built.
#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("COUPON_FIREBASE_SERVICE_ACCOUNT_PRIVATE_KEY is not a usable RS256 PEM: {0}")]
    UnusableKey(String),
}

impl CustomTokenSigner {
    /// Build a signer, or `None` when no service account is configured.
    ///
    /// The distinction matters. *Absent* is a deployment that has not registered a
    /// Firebase service account yet — every other endpoint still works and Kakao sign-in
    /// refuses with an explanation. *Malformed* is a typo in a secret, and that stops the
    /// process at boot rather than turning every sign-in into a 500.
    pub fn from_config(config: &Config) -> Result<Option<Self>, SignerError> {
        let (Some(email), Some(pem)) = (
            config
                .firebase_service_account_email
                .as_deref()
                .map(str::trim)
                .filter(|email| !email.is_empty()),
            config.firebase_service_account_key_pem(),
        ) else {
            return Ok(None);
        };

        let key = EncodingKey::from_rsa_pem(pem.as_bytes())
            .map_err(|error| SignerError::UnusableKey(error.to_string()))?;

        Ok(Some(Self {
            service_account_email: email.to_owned(),
            key,
            ttl: config.firebase_custom_token_ttl(),
        }))
    }

    /// Sign `uid` in. §9.2-7 caps the lifetime at an hour; [`Config`] clamps it there.
    pub fn mint(&self, uid: &str, now: DateTime<Utc>) -> ApiResult<MintedCustomToken> {
        if uid.is_empty() || uid.len() > MAX_UID_LENGTH {
            return Err(ApiError::new(ErrorCode::ServiceUnavailable)
                .internal(format!("uid of {} bytes is not usable", uid.len())));
        }

        let expires_at = now + self.ttl;
        let claims = CustomTokenClaims {
            iss: &self.service_account_email,
            sub: &self.service_account_email,
            aud: IDENTITY_TOOLKIT_AUDIENCE,
            iat: now.timestamp(),
            exp: expires_at.timestamp(),
            uid,
        };

        let token = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)
            .map_err(|error| {
                ApiError::new(ErrorCode::ServiceUnavailable)
                    .internal(format!("custom token signing failed: {error}"))
            })?;

        Ok(MintedCustomToken { token, expires_at })
    }
}

/// The error to return when Kakao sign-in reaches step 7 with no service account.
///
/// Deliberately explicit about *which* setting is missing in the internal detail, and
/// deliberately vague to the user: they cannot fix it and telling them which secret is
/// absent tells an attacker the same thing.
pub fn not_configured() -> ApiError {
    ApiError::with_message(
        ErrorCode::ServiceUnavailable,
        "카카오 로그인을 완료할 수 없습니다. 잠시 후 다시 시도해 주세요.",
    )
    .internal(
        "COUPON_FIREBASE_SERVICE_ACCOUNT_EMAIL and \
         COUPON_FIREBASE_SERVICE_ACCOUNT_PRIVATE_KEY are required to mint Firebase custom \
         tokens (§9.2-7)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_service_account_means_no_signer_rather_than_a_boot_failure() {
        let config: Config = serde_json::from_value(serde_json::json!({
            "env": "test",
            "database_url": "postgres://localhost/coupon",
            "firebase_project_id": "ddadan-test",
        }))
        .expect("config");

        assert!(
            CustomTokenSigner::from_config(&config)
                .expect("absent is not an error")
                .is_none()
        );
    }

    #[test]
    fn a_malformed_key_is_a_boot_failure_not_a_runtime_one() {
        // A typo in a secret must stop the process, not turn every sign-in into a 500.
        let config: Config = serde_json::from_value(serde_json::json!({
            "env": "test",
            "database_url": "postgres://localhost/coupon",
            "firebase_project_id": "ddadan-test",
            "firebase_service_account_email": "signer@ddadan-test.iam.gserviceaccount.com",
            "firebase_service_account_private_key":
                "-----BEGIN PRIVATE KEY-----\nnot a key\n-----END PRIVATE KEY-----",
        }))
        .expect("config");

        let error = CustomTokenSigner::from_config(&config).expect_err("must refuse");
        assert!(error.to_string().contains("PRIVATE_KEY"));
    }

    #[test]
    fn the_missing_configuration_error_names_the_settings_for_the_operator_only() {
        let error = not_configured();
        assert_eq!(error.code, ErrorCode::ServiceUnavailable);
        assert!(
            !error.message.contains("SERVICE_ACCOUNT"),
            "the user-facing message must not name a secret: {}",
            error.message
        );
    }
}
