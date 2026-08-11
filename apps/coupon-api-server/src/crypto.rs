//! Column-level protection for personal data (§16.5).
//!
//! Two primitives, deliberately kept apart:
//!
//! * [`Sealer`] encrypts values we only ever need to read back (AES-256-GCM).
//! * [`LookupHash`] produces a keyed hash for values we need to search by.
//!
//! Real key management (a KMS-backed data key per version) is a later phase. What is
//! fixed now is the *shape*: a version byte leads every ciphertext, so rotation is a
//! migration rather than a redesign, and no plaintext personal data reaches a column.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::config::Config;

/// Ciphertext layout version. Bump alongside the key derivation, never silently.
const ENVELOPE_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("COUPON_DATA_ENCRYPTION_KEY must be base64 for 32 bytes")]
    InvalidDataKey,
    #[error("COUPON_LOOKUP_HASH_SECRET must not be empty")]
    InvalidLookupSecret,
}

/// Seals values that are stored and later read back in full.
#[derive(Clone)]
pub struct Sealer {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for Sealer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Sealer(<redacted>)")
    }
}

impl Sealer {
    pub fn from_config(config: &Config) -> Result<Self, CryptoError> {
        let key_bytes: [u8; 32] = match config.data_encryption_key.as_deref() {
            Some(encoded) => BASE64
                .decode(encoded.trim())
                .map_err(|_| CryptoError::InvalidDataKey)?
                .try_into()
                .map_err(|_| CryptoError::InvalidDataKey)?,
            // Outside production a deterministic development key keeps local runs
            // working without anyone handling a real secret. `Config::validate` refuses
            // to boot production without an explicit key.
            None => Sha256::digest(b"coupon-development-data-key").into(),
        };

        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
        })
    }

    /// Encrypt for storage in a `*_ciphertext bytea` column.
    ///
    /// Layout: `version || nonce(12) || ciphertext+tag`. The version byte is
    /// authenticated as associated data, so it cannot be downgraded.
    pub fn seal(&self, plaintext: &str) -> Vec<u8> {
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: &[ENVELOPE_VERSION],
                },
            )
            .expect("AES-GCM encryption of an in-memory buffer cannot fail");

        let mut sealed = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        sealed.push(ENVELOPE_VERSION);
        sealed.extend_from_slice(&nonce_bytes);
        sealed.extend_from_slice(&ciphertext);
        sealed
    }

    /// Reverse [`Sealer::seal`]. Returns `None` for a truncated or tampered envelope.
    pub fn open(&self, sealed: &[u8]) -> Option<String> {
        if sealed.len() <= 1 + NONCE_LEN || sealed[0] != ENVELOPE_VERSION {
            return None;
        }

        let nonce = Nonce::from_slice(&sealed[1..1 + NONCE_LEN]);
        let plaintext = self
            .cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &sealed[1 + NONCE_LEN..],
                    aad: &[ENVELOPE_VERSION],
                },
            )
            .ok()?;

        String::from_utf8(plaintext).ok()
    }
}

/// Keyed hash for values that must remain searchable (email, phone, registration number)
/// without storing them in the clear.
#[derive(Clone)]
pub struct LookupHash {
    secret: Vec<u8>,
}

impl std::fmt::Debug for LookupHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LookupHash(<redacted>)")
    }
}

impl LookupHash {
    pub fn from_config(config: &Config) -> Result<Self, CryptoError> {
        match config.lookup_hash_secret.as_deref() {
            Some(secret) if !secret.trim().is_empty() => Ok(Self {
                secret: secret.as_bytes().to_vec(),
            }),
            Some(_) => Err(CryptoError::InvalidLookupSecret),
            None => Ok(Self {
                secret: b"coupon-development-lookup-secret".to_vec(),
            }),
        }
    }

    /// Hash a searchable value. `domain` separates namespaces so the same email cannot
    /// be correlated across columns.
    pub fn hash(&self, domain: &str, value: &str) -> Vec<u8> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.secret)
            .expect("hmac accepts any key length");
        mac.update(domain.as_bytes());
        mac.update(b"\0");
        mac.update(value.trim().to_ascii_lowercase().as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// Hash a client IP for consent evidence (§9.4). Keyed, so the record cannot be
    /// reversed by enumerating the IPv4 space.
    pub fn hash_ip(&self, ip: &str) -> Vec<u8> {
        self.hash("consent-ip", ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        serde_json::from_value(serde_json::json!({
            "database_url": "postgres://localhost/coupon",
            "firebase_project_id": "ddadan-test",
        }))
        .expect("test config")
    }

    #[test]
    fn sealed_values_round_trip() {
        let sealer = Sealer::from_config(&config()).expect("sealer");
        let sealed = sealer.seal("owner@example.com");

        assert_ne!(sealed.as_slice(), b"owner@example.com");
        assert_eq!(sealer.open(&sealed).as_deref(), Some("owner@example.com"));
    }

    #[test]
    fn each_sealing_uses_a_fresh_nonce() {
        let sealer = Sealer::from_config(&config()).expect("sealer");
        assert_ne!(
            sealer.seal("010-0000-0000"),
            sealer.seal("010-0000-0000"),
            "identical plaintext must not produce identical ciphertext"
        );
    }

    #[test]
    fn tampering_is_detected_rather_than_decrypted() {
        let sealer = Sealer::from_config(&config()).expect("sealer");
        let mut sealed = sealer.seal("123-45-67890");
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;

        assert_eq!(sealer.open(&sealed), None);
        assert_eq!(
            sealer.open(&[]),
            None,
            "a truncated envelope must not panic"
        );
    }

    #[test]
    fn an_explicit_key_must_be_thirty_two_bytes() {
        let mut config = config();
        config.data_encryption_key = Some(BASE64.encode([7u8; 16]));

        assert!(matches!(
            Sealer::from_config(&config),
            Err(CryptoError::InvalidDataKey)
        ));
    }

    #[test]
    fn lookup_hashes_are_stable_case_folded_and_domain_separated() {
        let hasher = LookupHash::from_config(&config()).expect("hasher");

        assert_eq!(
            hasher.hash("email", "Owner@Example.com "),
            hasher.hash("email", "owner@example.com")
        );
        assert_ne!(
            hasher.hash("email", "owner@example.com"),
            hasher.hash("phone", "owner@example.com"),
            "domains must not collide"
        );
        assert_eq!(hasher.hash("email", "a@b.c").len(), 32);
    }
}
