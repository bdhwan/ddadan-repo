//! The signed QR payload (§16.2).
//!
//! Shape is a JWS-style compact token: `base64url(header).base64url(payload).base64url(sig)`.
//! Ed25519 signs it, and the header carries a key id, so a future verifier-only component
//! (a scanner, an offline check) can validate a token without holding anything that could
//! mint one. A symmetric MAC would have made every verifier an issuer.
//!
//! What is deliberately *not* in the payload: the consumer key, the internal user id, an
//! email, a name. The payload identifies the **nonce**; who that nonce belongs to is a
//! database lookup, so a photographed QR reveals nothing about its owner.
//!
//! This module is pure — no database, no clock of its own. `now` is always passed in, so
//! the server's time is the only time that decides validity (§5.2).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::error::{ApiError, ApiResult, ErrorCode};

/// Payload layout version. A verifier refuses anything it does not recognise rather than
/// guessing at an unknown shape.
pub const PAYLOAD_VERSION: u8 = 1;

/// The only audience Phase 2 issues. A token minted for one purpose must not be accepted
/// for another (SEC-002), so redemption will get its own value rather than reusing this.
pub const AUDIENCE_STAMP: &str = "ddadan.stamp";

/// Nonce entropy. §16.2 requires at least 128 bits; 256 costs nothing here.
const NONCE_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Header {
    alg: String,
    typ: String,
    kid: String,
}

/// What a scanner reads out of the QR.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QrPayload {
    /// Payload version (§16.2).
    pub v: u8,
    /// Base64url single-use nonce. Only its hash is ever stored (§12.5).
    pub nonce: String,
    /// Opaque subject. A keyed hash, not the consumer key — it identifies the token, not
    /// a person, and cannot be reversed into one.
    pub sub: String,
    pub aud: String,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub iat: DateTime<Utc>,
    #[serde(with = "chrono::serde::ts_seconds")]
    pub exp: DateTime<Utc>,
}

/// A freshly minted token and the secrets the caller must persist as hashes.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    pub token: String,
    pub payload: QrPayload,
    /// Plaintext nonce. Hash it, store the hash, and drop this.
    pub nonce: String,
    /// The manual 8-digit code (STORE-005). Independent randomness — deriving it from the
    /// nonce would mean one leak compromised both.
    pub fallback_code: String,
    pub key_id: String,
}

/// Ed25519 issuer/verifier pair for QR tokens.
#[derive(Clone)]
pub struct QrSigner {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    key_id: String,
}

impl std::fmt::Debug for QrSigner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QrSigner")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

impl QrSigner {
    /// Build from configuration.
    ///
    /// Outside production a deterministic development seed is derived so a local run
    /// needs no key handling; `Config::validate` refuses to boot production without an
    /// explicit `COUPON_QR_SIGNING_KEY`.
    pub fn from_config(config: &Config) -> ApiResult<Self> {
        let seed: [u8; 32] = match config.qr_signing_key.as_deref() {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .ok()
                .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
                .ok_or_else(|| {
                    ApiError::new(ErrorCode::ServiceUnavailable)
                        .internal("COUPON_QR_SIGNING_KEY must be base64 for 32 bytes")
                })?,
            None => Sha256::digest(b"coupon-development-qr-signing-key").into(),
        };

        Ok(Self::from_seed(seed))
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        // The key id is derived from the public key, so rotating the key rotates the id
        // automatically and a stale token is rejected on `kid` before signature checking.
        let key_id = hex::encode(&Sha256::digest(verifying_key.as_bytes())[..8]);

        Self {
            signing_key,
            verifying_key,
            key_id,
        }
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Mint a token valid for `ttl` from `now`.
    pub fn issue(
        &self,
        subject: &str,
        audience: &str,
        now: DateTime<Utc>,
        ttl: chrono::Duration,
    ) -> IssuedToken {
        let mut nonce_bytes = [0u8; NONCE_BYTES];
        rand::Rng::fill(&mut rand::thread_rng(), &mut nonce_bytes[..]);
        let nonce = B64.encode(nonce_bytes);

        let payload = QrPayload {
            v: PAYLOAD_VERSION,
            nonce: nonce.clone(),
            sub: subject.to_owned(),
            aud: audience.to_owned(),
            iat: now,
            exp: now + ttl,
        };

        let header = Header {
            alg: "EdDSA".to_owned(),
            typ: "DQR".to_owned(),
            kid: self.key_id.clone(),
        };

        let signing_input = format!(
            "{}.{}",
            B64.encode(serde_json::to_vec(&header).expect("header serialises")),
            B64.encode(serde_json::to_vec(&payload).expect("payload serialises")),
        );
        let signature = self.signing_key.sign(signing_input.as_bytes());
        let token = format!("{signing_input}.{}", B64.encode(signature.to_bytes()));

        IssuedToken {
            token,
            payload,
            nonce,
            fallback_code: generate_fallback_code(),
            key_id: self.key_id.clone(),
        }
    }

    /// Verify a token's signature, shape, audience and lifetime.
    ///
    /// Every failure returns one of two generic codes; the specific reason goes to the
    /// internal detail, which is logged and never serialised (SEC-002).
    pub fn verify(
        &self,
        token: &str,
        audience: &str,
        now: DateTime<Utc>,
    ) -> ApiResult<QrPayload> {
        let invalid = |reason: &str| {
            ApiError::new(ErrorCode::QrTokenInvalid).internal(format!("qr token {reason}"))
        };

        let mut parts = token.trim().split('.');
        let (Some(header_part), Some(payload_part), Some(signature_part), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid("is not a three-part compact token"));
        };

        let header: Header = B64
            .decode(header_part)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| invalid("has an unreadable header"))?;

        if header.alg != "EdDSA" || header.typ != "DQR" {
            return Err(invalid("declares an unexpected algorithm or type"));
        }
        // Checked before the signature so a token from a retired key is cheap to reject.
        if header.kid != self.key_id {
            return Err(invalid("was signed by an unknown key"));
        }

        let signature_bytes: [u8; 64] = B64
            .decode(signature_part)
            .ok()
            .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
            .ok_or_else(|| invalid("has a malformed signature"))?;

        let signing_input = format!("{header_part}.{payload_part}");
        self.verifying_key
            .verify(
                signing_input.as_bytes(),
                &ed25519_dalek::Signature::from_bytes(&signature_bytes),
            )
            .map_err(|_| invalid("failed signature verification"))?;

        let payload: QrPayload = B64
            .decode(payload_part)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| invalid("has an unreadable payload"))?;

        if payload.v != PAYLOAD_VERSION {
            return Err(invalid("uses an unsupported payload version"));
        }
        if payload.aud != audience {
            return Err(invalid("was issued for a different audience"));
        }
        if payload.exp <= payload.iat {
            return Err(invalid("expires before it was issued"));
        }
        // Expiry is judged against the server's clock, never the scanner's (§5.2).
        if now >= payload.exp {
            return Err(ApiError::new(ErrorCode::QrTokenExpired).internal("qr token has expired"));
        }

        Ok(payload)
    }
}

/// Eight digits, uniformly over the whole `00000000`–`99999999` range.
///
/// Leading zeros are kept rather than avoided: dropping them would shrink the space by
/// ten percent for no benefit, and the code is displayed as text either way.
fn generate_fallback_code() -> String {
    let value = rand::Rng::gen_range(&mut rand::thread_rng(), 0u32..100_000_000);
    format!("{value:08}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> QrSigner {
        QrSigner::from_seed([7u8; 32])
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-11T06:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    fn ttl() -> chrono::Duration {
        chrono::Duration::seconds(60)
    }

    #[test]
    fn a_freshly_issued_token_verifies() {
        let signer = signer();
        let issued = signer.issue("subject-1", AUDIENCE_STAMP, now(), ttl());

        let payload = signer
            .verify(&issued.token, AUDIENCE_STAMP, now())
            .expect("verifies");
        assert_eq!(payload, issued.payload);
        assert_eq!(payload.exp - payload.iat, ttl());
    }

    #[test]
    fn the_payload_carries_no_personal_data() {
        let issued = signer().issue("opaque-subject", AUDIENCE_STAMP, now(), ttl());
        let payload = serde_json::to_string(&issued.payload).expect("serialises");

        // The fields are a closed set: version, nonce, subject, audience, timestamps.
        let value: serde_json::Value = serde_json::from_str(&payload).expect("object");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["aud", "exp", "iat", "nonce", "sub", "v"]);
    }

    #[test]
    fn an_expired_token_is_reported_as_expired() {
        let signer = signer();
        let issued = signer.issue("subject-1", AUDIENCE_STAMP, now(), ttl());

        // The boundary is exclusive at the end (§5.2 `[start, end)`).
        signer
            .verify(&issued.token, AUDIENCE_STAMP, now() + ttl() - chrono::Duration::seconds(1))
            .expect("still valid one second before expiry");

        let error = signer
            .verify(&issued.token, AUDIENCE_STAMP, now() + ttl())
            .expect_err("expired exactly at exp");
        assert_eq!(error.code, ErrorCode::QrTokenExpired);
    }

    #[test]
    fn a_tampered_payload_fails_verification() {
        let signer = signer();
        let issued = signer.issue("subject-1", AUDIENCE_STAMP, now(), ttl());

        let mut parts: Vec<&str> = issued.token.split('.').collect();
        let forged_payload = B64.encode(
            serde_json::to_vec(&QrPayload {
                exp: now() + chrono::Duration::days(365),
                ..issued.payload.clone()
            })
            .expect("serialises"),
        );
        parts[1] = &forged_payload;

        let error = signer
            .verify(&parts.join("."), AUDIENCE_STAMP, now())
            .expect_err("a rewritten expiry must not be honoured");
        assert_eq!(error.code, ErrorCode::QrTokenInvalid);
    }

    #[test]
    fn a_token_from_another_key_is_rejected() {
        let issued = QrSigner::from_seed([1u8; 32]).issue("subject-1", AUDIENCE_STAMP, now(), ttl());

        let error = signer()
            .verify(&issued.token, AUDIENCE_STAMP, now())
            .expect_err("another issuer's key must not be trusted");
        assert_eq!(error.code, ErrorCode::QrTokenInvalid);
    }

    #[test]
    fn a_token_for_another_audience_is_rejected() {
        let signer = signer();
        let issued = signer.issue("subject-1", "ddadan.redemption", now(), ttl());

        let error = signer
            .verify(&issued.token, AUDIENCE_STAMP, now())
            .expect_err("audience must match");
        assert_eq!(error.code, ErrorCode::QrTokenInvalid);
    }

    #[test]
    fn malformed_tokens_all_collapse_to_one_code() {
        let signer = signer();
        for malformed in ["", "a.b", "a.b.c.d", "not-base64!.x.y", "a.b.c"] {
            let error = signer
                .verify(malformed, AUDIENCE_STAMP, now())
                .expect_err("must reject");
            assert_eq!(
                error.code,
                ErrorCode::QrTokenInvalid,
                "{malformed} must not reveal why it failed"
            );
            assert!(
                !error.message.contains("signature") && !error.message.contains("key"),
                "the client-facing message must not describe the failure"
            );
        }
    }

    #[test]
    fn every_issued_token_has_its_own_nonce_and_code() {
        let signer = signer();
        let first = signer.issue("subject-1", AUDIENCE_STAMP, now(), ttl());
        let second = signer.issue("subject-1", AUDIENCE_STAMP, now(), ttl());

        // WALLET-004: two devices, two independent nonces.
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.token, second.token);
        assert_ne!(
            first.fallback_code, second.fallback_code,
            "the manual code must not be derived from anything shared"
        );
    }

    #[test]
    fn the_fallback_code_is_eight_digits() {
        for _ in 0..64 {
            let code = generate_fallback_code();
            assert_eq!(code.len(), 8);
            assert!(code.chars().all(|c| c.is_ascii_digit()), "{code}");
        }
    }

    #[test]
    fn the_key_id_follows_the_key() {
        assert_ne!(
            QrSigner::from_seed([1u8; 32]).key_id(),
            QrSigner::from_seed([2u8; 32]).key_id(),
        );
        assert_eq!(
            QrSigner::from_seed([1u8; 32]).key_id(),
            QrSigner::from_seed([1u8; 32]).key_id(),
            "the id must be stable for a given key"
        );
    }
}
