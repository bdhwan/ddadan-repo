//! 회전형 QR 발급과 소비 (§10.2 `qr`, §16.2, WALLET-003/004, SEC-002).
//!
//! Owns `coupon.qr_nonces`. Two rules shape everything here:
//!
//! 1. **The database never sees a nonce or a manual code in the clear.** Both are stored
//!    as keyed hashes, so a database dump does not let anyone replay a live QR or brute
//!    force an eight-digit code offline.
//! 2. **A nonce is consumed by a conditional UPDATE, never by read-then-write.** That is
//!    what makes §12.6-7 hold when the same QR is scanned twice at once: the loser's
//!    UPDATE matches zero rows and gets `QR_ALREADY_USED`.

pub mod routes;
pub mod token;

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::config::Config;
use crate::crypto::LookupHash;
use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};

pub use routes::qr_router;
pub use token::{AUDIENCE_STAMP, QrPayload, QrSigner};

/// How often the consumer's screen swaps in a new QR (§23.1). The old one stays valid
/// until it expires — WALLET-003 is explicit that issuing a new QR does not invalidate
/// the previous one; only a successful transaction does.
const REFRESH_AFTER_SECONDS: i64 = 30;

/// Manual-code collisions are astronomically unlikely but not impossible, and a collision
/// must not surface as a failed scan attempt for the consumer.
const CODE_ATTEMPTS: usize = 5;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QrTokenResponse {
    /// The signed payload to render as a QR code.
    pub token: String,
    /// Eight digits for STORE-005, when the camera is unavailable.
    pub fallback_code: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Seconds the token remains valid.
    pub expires_in_seconds: i64,
    /// Seconds after which the screen should request a replacement.
    pub refresh_after_seconds: i64,
    /// Which signing key produced this token (§16.2).
    pub key_id: String,
}

/// A nonce that passed every check short of being consumed.
#[derive(Debug, Clone)]
pub struct ResolvedNonce {
    pub nonce_id: Uuid,
    pub user_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// How the owner presented the customer's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented<'a> {
    /// Scanned from the camera.
    Token(&'a str),
    /// Typed in from the customer's screen (STORE-005).
    FallbackCode(&'a str),
}

pub struct QrService {
    signer: QrSigner,
    lookup_hash: Arc<LookupHash>,
    config: Arc<Config>,
}

impl QrService {
    pub fn new(config: Arc<Config>, lookup_hash: Arc<LookupHash>) -> ApiResult<Self> {
        Ok(Self {
            signer: QrSigner::from_config(&config)?,
            lookup_hash,
            config,
        })
    }

    pub fn key_id(&self) -> &str {
        self.signer.key_id()
    }

    /// The opaque subject that goes in the payload.
    ///
    /// A keyed hash of the consumer key, not the key itself: it is stable enough to
    /// correlate a token with its issuance in a log, and useless to anyone who
    /// photographs the QR (§16.2).
    fn subject_for(&self, consumer_key: Uuid) -> String {
        B64.encode(
            &self
                .lookup_hash
                .hash("qr-subject", &consumer_key.to_string())[..16],
        )
    }

    fn nonce_hash(&self, nonce: &str) -> Vec<u8> {
        self.lookup_hash.hash("qr-nonce", nonce)
    }

    fn code_hash(&self, code: &str) -> Vec<u8> {
        self.lookup_hash.hash("qr-fallback-code", code)
    }

    /// Issue a rotating token for a consumer (§11.3 `POST /me/qr-tokens`).
    pub async fn issue(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        consumer_key: Uuid,
        audience: &str,
    ) -> ApiResult<QrTokenResponse> {
        // The database clock decides validity, so it must also decide issuance (§5.2).
        let now = database_now(pool).await?;
        let ttl = self.config.qr_token_ttl();
        let subject = self.subject_for(consumer_key);

        for attempt in 0..CODE_ATTEMPTS {
            let issued = self.signer.issue(&subject, audience, now, ttl);

            let result = sqlx::query!(
                r#"
                INSERT INTO coupon.qr_nonces
                    (nonce_hash, fallback_code_hash, user_id, audience, key_id,
                     issued_at, expires_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
                self.nonce_hash(&issued.nonce),
                self.code_hash(&issued.fallback_code),
                user_id,
                audience,
                issued.key_id,
                issued.payload.iat,
                issued.payload.exp,
            )
            .execute(pool)
            .await;

            match result {
                Ok(_) => {
                    return Ok(QrTokenResponse {
                        token: issued.token,
                        fallback_code: issued.fallback_code,
                        issued_at: issued.payload.iat,
                        expires_at: issued.payload.exp,
                        expires_in_seconds: ttl.num_seconds(),
                        refresh_after_seconds: REFRESH_AFTER_SECONDS.min(ttl.num_seconds()),
                        key_id: issued.key_id,
                    });
                }
                // The manual code is only eight digits, so over the lifetime of the table
                // a collision with a historical row is possible. Draw again rather than
                // failing a customer's scan.
                Err(sqlx::Error::Database(db))
                    if db.code().as_deref() == Some("23505") && attempt + 1 < CODE_ATTEMPTS =>
                {
                    tracing::warn!(constraint = ?db.constraint(), "qr nonce collision; retrying");
                }
                Err(error) => return Err(ApiError::from(error)),
            }
        }

        Err(ApiError::new(ErrorCode::ServiceUnavailable)
            .internal("could not allocate a unique QR fallback code"))
    }

    /// Verify what the owner presented and find the nonce behind it, without consuming it.
    ///
    /// Used by both `scan/resolve` and the accrual preview, neither of which may burn the
    /// customer's QR just by looking at it.
    pub async fn resolve<'e, E>(
        &self,
        executor: E,
        presented: Presented<'_>,
        audience: &str,
        now: DateTime<Utc>,
    ) -> ApiResult<ResolvedNonce>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let (column_hash, by_code) = match presented {
            Presented::Token(token) => {
                let payload = self.signer.verify(token, audience, now)?;
                (self.nonce_hash(&payload.nonce), false)
            }
            Presented::FallbackCode(code) => {
                let digits: String = code.chars().filter(char::is_ascii_digit).collect();
                if digits.len() != 8 {
                    return Err(ApiError::new(ErrorCode::QrTokenInvalid)
                        .internal("fallback code is not eight digits"));
                }
                (self.code_hash(&digits), true)
            }
        };

        let row = sqlx::query!(
            r#"
            SELECT id, user_id, audience, issued_at, expires_at, consumed_at, revoked_at
            FROM coupon.qr_nonces
            WHERE ($2::boolean AND fallback_code_hash = $1)
               OR (NOT $2::boolean AND nonce_hash = $1)
            "#,
            column_hash,
            by_code,
        )
        .fetch_optional(executor)
        .await?
        // A signature can be valid for a nonce we have no record of — that is what a
        // replay of a token minted before a database restore looks like. Same generic
        // code as a forgery (SEC-002).
        .ok_or_else(|| {
            ApiError::new(ErrorCode::QrTokenInvalid).internal("nonce is not on record")
        })?;

        if row.audience != audience {
            return Err(ApiError::new(ErrorCode::QrTokenInvalid)
                .internal("nonce was issued for a different audience"));
        }
        if row.revoked_at.is_some() {
            return Err(
                ApiError::new(ErrorCode::QrTokenInvalid).internal("nonce has been revoked")
            );
        }
        // A consumed nonce is called out specifically: the owner needs to know to ask for
        // a fresh QR rather than suspect the customer (§15).
        if row.consumed_at.is_some() {
            return Err(ApiError::new(ErrorCode::QrAlreadyUsed).internal("nonce already consumed"));
        }
        if now >= row.expires_at {
            return Err(ApiError::new(ErrorCode::QrTokenExpired).internal("nonce has expired"));
        }

        Ok(ResolvedNonce {
            nonce_id: row.id,
            user_id: row.user_id,
            issued_at: row.issued_at,
            expires_at: row.expires_at,
        })
    }

    /// Re-check a nonce's state inside the accrual transaction, without locking it.
    ///
    /// §13.1 puts the nonce *last* in the lock order but validates it *first*, and this is
    /// what reconciles the two. By the time an accrual reaches here it holds the store
    /// lock, so a competing scan of the same QR has either already committed — in which
    /// case this read sees it and the owner is told the QR is spent — or has not started.
    ///
    /// Without this, a duplicate scan would run the whole business assessment first and
    /// fail on some incidental rule (a near-duplicate amount, say) rather than on the
    /// reason that actually applies.
    pub async fn ensure_unconsumed(
        &self,
        tx: &mut Tx<'_>,
        nonce_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        let row = sqlx::query!(
            r#"
            SELECT consumed_at, revoked_at, expires_at
            FROM coupon.qr_nonces
            WHERE id = $1
            "#,
            nonce_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::QrTokenInvalid).internal("nonce disappeared"))?;

        if row.consumed_at.is_some() {
            return Err(ApiError::new(ErrorCode::QrAlreadyUsed).internal("nonce already consumed"));
        }
        if row.revoked_at.is_some() {
            return Err(
                ApiError::new(ErrorCode::QrTokenInvalid).internal("nonce has been revoked")
            );
        }
        if now >= row.expires_at {
            return Err(ApiError::new(ErrorCode::QrTokenExpired).internal("nonce has expired"));
        }

        Ok(())
    }

    /// Take the row lock the accrual transaction holds while it commits (§13.1 step 2).
    ///
    /// Called *late* in the transaction, after store, policy and customer, because §13.1
    /// fixes that lock order to avoid deadlocks. Validity was already established by
    /// [`QrService::resolve`]; this re-reads it under the lock so a nonce consumed in the
    /// meantime is caught.
    pub async fn lock(
        &self,
        tx: &mut Tx<'_>,
        nonce_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<ResolvedNonce> {
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, issued_at, expires_at, consumed_at, revoked_at
            FROM coupon.qr_nonces
            WHERE id = $1
            FOR UPDATE
            "#,
            nonce_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::QrTokenInvalid).internal("nonce disappeared"))?;

        if row.consumed_at.is_some() {
            return Err(ApiError::new(ErrorCode::QrAlreadyUsed).internal("nonce already consumed"));
        }
        if row.revoked_at.is_some() {
            return Err(
                ApiError::new(ErrorCode::QrTokenInvalid).internal("nonce has been revoked")
            );
        }
        if now >= row.expires_at {
            return Err(ApiError::new(ErrorCode::QrTokenExpired).internal("nonce has expired"));
        }

        Ok(ResolvedNonce {
            nonce_id: row.id,
            user_id: row.user_id,
            issued_at: row.issued_at,
            expires_at: row.expires_at,
        })
    }

    /// Link the nonce to the transaction that used it (§12.6-7).
    ///
    /// Conditional on the nonce still being unconsumed, so this is the single point where
    /// a concurrent duplicate is decided even if the lock above were somehow skipped.
    pub async fn consume(
        &self,
        tx: &mut Tx<'_>,
        nonce_id: Uuid,
        transaction_type: &str,
        transaction_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        let result = sqlx::query!(
            r#"
            UPDATE coupon.qr_nonces
            SET consumed_at = $2,
                consumed_transaction_type = $3,
                consumed_transaction_id = $4
            WHERE id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
            "#,
            nonce_id,
            now,
            transaction_type,
            transaction_id,
        )
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(ApiError::new(ErrorCode::QrAlreadyUsed).internal("nonce was consumed concurrently"))
        }
    }

    /// Invalidate every unused nonce for a user (WALLET-004: reported account takeover).
    pub async fn revoke_unused(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<u64> {
        let result = sqlx::query!(
            r#"
            UPDATE coupon.qr_nonces
            SET revoked_at = clock_timestamp()
            WHERE user_id = $1 AND consumed_at IS NULL AND revoked_at IS NULL
            "#,
            user_id,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Drop nonces that are long past their usefulness.
    ///
    /// Keeps the eight-digit code space sparse so [`QrService::issue`] rarely has to draw
    /// twice. Consumed nonces are kept — they are the evidence that a transaction had a
    /// genuine QR behind it.
    pub async fn purge_expired(&self, pool: &PgPool, older_than: DateTime<Utc>) -> ApiResult<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM coupon.qr_nonces
            WHERE expires_at < $1 AND consumed_at IS NULL
            "#,
            older_than,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

/// The database's clock. Every deadline in this system is compared against it, so the
/// process clock drifting must not change what is valid (§5.2).
pub async fn database_now(pool: &PgPool) -> ApiResult<DateTime<Utc>> {
    Ok(sqlx::query_scalar!(r#"SELECT clock_timestamp() AS "now!""#)
        .fetch_one(pool)
        .await?)
}

/// Same, inside a transaction.
pub async fn transaction_now(tx: &mut Tx<'_>) -> ApiResult<DateTime<Utc>> {
    Ok(sqlx::query_scalar!(r#"SELECT clock_timestamp() AS "now!""#)
        .fetch_one(&mut **tx)
        .await?)
}
