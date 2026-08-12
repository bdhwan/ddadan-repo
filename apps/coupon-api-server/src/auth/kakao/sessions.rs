//! The two short-lived stores the login flow needs (§9.2-2, §9.2-3).
//!
//! Both hold a bearer secret — a `state` that lets its holder finish somebody's login,
//! an exchange code that lets its holder collect somebody's Firebase token — so both are
//! stored as a keyed hash rather than in the clear, and both are consumed by an atomic
//! `UPDATE ... WHERE consumed_at IS NULL RETURNING`.
//!
//! That `UPDATE` is the single-use guarantee. Doing the check and the mark in one
//! statement means two requests presenting the same code cannot both find it unused: one
//! updates the row, the other matches nothing and is refused.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::crypto::{LookupHash, Sealer};
use crate::error::{ApiError, ApiResult, ErrorCode};

use super::KakaoIdentity;

/// Domain separator for the keyed hash of a `state`.
const STATE_DOMAIN: &str = "oauth-login-state";
/// Domain separator for the keyed hash of an exchange code.
const CODE_DOMAIN: &str = "oauth-exchange-code";

/// What the callback needs to recover from the authorize leg.
#[derive(Debug, Clone)]
pub struct LoginSession {
    pub id: Uuid,
    pub nonce: String,
    pub code_verifier: String,
    pub redirect_uri: String,
}

/// Record an authorize request so its callback can be matched to it.
#[allow(clippy::too_many_arguments)]
pub async fn create_login_session(
    pool: &sqlx::PgPool,
    lookup_hash: &LookupHash,
    sealer: &Sealer,
    provider: &str,
    state: &str,
    nonce: &str,
    code_verifier: &str,
    redirect_uri: &str,
    expires_at: DateTime<Utc>,
) -> ApiResult<Uuid> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO coupon.oauth_login_sessions
            (provider, state_hash, nonce, code_verifier_ciphertext, redirect_uri, expires_at)
        VALUES ($1::text::coupon.auth_provider, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
        provider,
        lookup_hash.hash(STATE_DOMAIN, state),
        nonce,
        sealer.seal(code_verifier),
        redirect_uri,
        expires_at,
    )
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Claim the session belonging to `state`, or `None` if there is no live one.
///
/// `None` covers every way this can go wrong — never issued, already used, expired,
/// forged — because the caller must not report which (§6.1 collapses them all into
/// 보안 검증 실패).
pub async fn consume_login_session(
    pool: &sqlx::PgPool,
    lookup_hash: &LookupHash,
    sealer: &Sealer,
    provider: &str,
    state: &str,
    now: DateTime<Utc>,
) -> ApiResult<Option<LoginSession>> {
    let row = sqlx::query!(
        r#"
        UPDATE coupon.oauth_login_sessions
        SET consumed_at = $4
        WHERE provider = $1::text::coupon.auth_provider
          AND state_hash = $2
          AND consumed_at IS NULL
          AND expires_at > $3
        RETURNING id, nonce, code_verifier_ciphertext, redirect_uri
        "#,
        provider,
        lookup_hash.hash(STATE_DOMAIN, state),
        now,
        now,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let code_verifier = sealer.open(&row.code_verifier_ciphertext).ok_or_else(|| {
        // The row is ours and unexpired but will not open: the encryption key changed
        // under a login in flight. Not the caller's fault, and not a security event.
        ApiError::new(ErrorCode::DependencyUnavailable)
            .internal("stored PKCE verifier could not be opened")
    })?;

    Ok(Some(LoginSession {
        id: row.id,
        nonce: row.nonce,
        code_verifier,
        redirect_uri: row.redirect_uri,
    }))
}

/// Hand out the single-use code the callback redirects with (§9.2-3).
pub async fn issue_exchange_code(
    pool: &sqlx::PgPool,
    lookup_hash: &LookupHash,
    sealer: &Sealer,
    provider: &str,
    code: &str,
    identity: &KakaoIdentity,
    expires_at: DateTime<Utc>,
) -> ApiResult<Uuid> {
    let id = sqlx::query_scalar!(
        r#"
        INSERT INTO coupon.oauth_exchange_codes
            (provider, code_hash, provider_subject, email_ciphertext, email_verified, expires_at)
        VALUES ($1::text::coupon.auth_provider, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
        provider,
        lookup_hash.hash(CODE_DOMAIN, code),
        identity.provider_subject,
        identity.email.as_deref().map(|email| sealer.seal(email)),
        identity.email_verified,
        expires_at,
    )
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Spend an exchange code. The second attempt with the same code gets `None`.
pub async fn consume_exchange_code(
    pool: &sqlx::PgPool,
    lookup_hash: &LookupHash,
    sealer: &Sealer,
    provider: &str,
    code: &str,
    now: DateTime<Utc>,
) -> ApiResult<Option<KakaoIdentity>> {
    let row = sqlx::query!(
        r#"
        UPDATE coupon.oauth_exchange_codes
        SET consumed_at = $4
        WHERE provider = $1::text::coupon.auth_provider
          AND code_hash = $2
          AND consumed_at IS NULL
          AND expires_at > $3
        RETURNING provider_subject, email_ciphertext, email_verified
        "#,
        provider,
        lookup_hash.hash(CODE_DOMAIN, code),
        now,
        now,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| KakaoIdentity {
        provider_subject: row.provider_subject,
        email: row
            .email_ciphertext
            .as_deref()
            .and_then(|sealed| sealer.open(sealed)),
        email_verified: row.email_verified,
    }))
}

/// Drop rows whose window has closed.
///
/// Expiry is enforced by the `WHERE` clauses above, so this is housekeeping rather than a
/// security control — but `oauth_login_sessions` gains a row on every authorize, and a
/// table that only ever grows is its own kind of problem. No scheduler calls this yet; it
/// stands where `QrService::purge_expired` does, as the operation a sweep will perform
/// once there is one to hang it on.
pub async fn purge_expired(pool: &sqlx::PgPool, before: DateTime<Utc>) -> ApiResult<u64> {
    let sessions = sqlx::query!(
        "DELETE FROM coupon.oauth_login_sessions WHERE expires_at < $1",
        before
    )
    .execute(pool)
    .await?
    .rows_affected();

    let codes = sqlx::query!(
        "DELETE FROM coupon.oauth_exchange_codes WHERE expires_at < $1",
        before
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(sessions + codes)
}
