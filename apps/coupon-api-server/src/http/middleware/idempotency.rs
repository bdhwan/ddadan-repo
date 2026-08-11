//! Idempotency for state-changing requests (§11.1, §12.6-9).
//!
//! Every POST/PATCH/DELETE must carry an `Idempotency-Key` UUID. The key plus a hash of
//! the request body is recorded in `coupon.idempotency_requests` before the handler
//! runs:
//!
//! * same key, same body, completed → the stored response is replayed verbatim
//! * same key, same body, still running → 409, retryable
//! * same key, *different* body → 409, not retryable (§12.6-9)
//!
//! A failed attempt releases its key so the caller can correct the request and retry.

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::AuthContext;
use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;

pub const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
/// Set on a replayed response so a client can tell it did not cause a new change.
pub const REPLAY_HEADER: &str = "idempotent-replay";

/// Bodies larger than this are rejected before we try to hash or store them.
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// A previously recorded attempt under one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRequest {
    pub request_hash: String,
    pub status: RecordStatus,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordStatus {
    Processing,
    Completed,
    Failed,
}

impl RecordStatus {
    fn from_db(raw: &str) -> Self {
        match raw {
            "COMPLETED" => RecordStatus::Completed,
            "FAILED" => RecordStatus::Failed,
            _ => RecordStatus::Processing,
        }
    }
}

/// What to do about an incoming request, given whatever is already recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// No usable record: run the handler.
    Proceed,
    /// Identical request already succeeded: return the stored response.
    Replay { status: u16, body: String },
    /// Identical request is still in flight.
    InProgress,
    /// Same key, different body — the client reused a key it must not have (§12.6-9).
    KeyReused,
}

/// The whole idempotency rule, as one pure function so it can be tested exhaustively
/// without a database.
pub fn decide(existing: Option<&StoredRequest>, request_hash: &str) -> Decision {
    let Some(existing) = existing else {
        return Decision::Proceed;
    };

    // The body hash is checked before anything else: a mismatch is a client bug and
    // must never be served a response belonging to a different request.
    if existing.request_hash != request_hash {
        return Decision::KeyReused;
    }

    match existing.status {
        RecordStatus::Processing => Decision::InProgress,
        // A failed attempt left no response worth replaying, so let the retry through.
        RecordStatus::Failed => Decision::Proceed,
        RecordStatus::Completed => match (existing.response_status, &existing.response_body) {
            (Some(status), Some(body)) => Decision::Replay {
                status: status as u16,
                body: body.clone(),
            },
            // Marked complete without a stored response: nothing to replay, and
            // re-running is safer than inventing an answer.
            _ => Decision::Proceed,
        },
    }
}

pub async fn layer(State(state): State<AppState>, request: Request, next: Next) -> Response {
    match run(state, request, next).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn run(state: AppState, request: Request, next: Next) -> Result<Response, ApiError> {
    if !is_state_changing(request.method()) {
        return Ok(next.run(request).await);
    }

    let key = idempotency_key(&request)?;
    let operation = format!("{} {}", request.method(), request.uri().path());

    // The body has to be read to hash it, so put it back before the handler runs.
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|error| {
            ApiError::with_message(
                ErrorCode::InvalidRequest,
                "요청 본문이 너무 크거나 읽을 수 없습니다.",
            )
            .internal(error.to_string())
        })?;
    let request_hash = hash_body(&bytes);

    // `idempotency_requests.actor_user_id` references `users`, so a caller who has not
    // bootstrapped yet cannot be recorded. That is only `POST /users/bootstrap`, which
    // is idempotent by construction: `users.firebase_uid` is unique.
    let actor = parts
        .extensions
        .get::<AuthContext>()
        .and_then(|context| context.account.as_ref())
        .map(|account| account.user_id);

    let request = Request::from_parts(parts, Body::from(bytes));

    let Some(actor) = actor else {
        tracing::debug!(
            operation,
            "idempotency not persisted: caller has no account yet"
        );
        return Ok(next.run(request).await);
    };

    let existing = claim(
        &state.pool,
        actor,
        &operation,
        key,
        &request_hash,
        state.config.idempotency_ttl_hours,
    )
    .await?;

    match decide(existing.as_ref(), &request_hash) {
        Decision::Proceed => {}
        Decision::InProgress => return Err(ApiError::new(ErrorCode::IdempotencyRequestInProgress)),
        Decision::KeyReused => return Err(ApiError::new(ErrorCode::IdempotencyKeyReused)),
        Decision::Replay { status, body } => return Ok(replay(status, body)),
    }

    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY_BYTES)
        .await
        .unwrap_or_else(|_| Bytes::new());

    if parts.status.is_success() {
        finish(
            &state.pool,
            actor,
            &operation,
            key,
            parts.status.as_u16(),
            &bytes,
        )
        .await;
    } else {
        // Release the key so the caller can fix the request and reuse it.
        release(&state.pool, actor, &operation, key).await;
    }

    Ok(Response::from_parts(parts, Body::from(bytes)))
}

fn is_state_changing(method: &axum::http::Method) -> bool {
    use axum::http::Method;
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn idempotency_key(request: &Request) -> Result<Uuid, ApiError> {
    let raw = request
        .headers()
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(ErrorCode::IdempotencyKeyRequired))?;

    Uuid::parse_str(raw.trim()).map_err(|error| {
        ApiError::new(ErrorCode::IdempotencyKeyInvalid).internal(error.to_string())
    })
}

/// Hex SHA-256, sized to fit the `char(64)` column.
pub fn hash_body(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

fn replay(status: u16, body: String) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
        .headers_mut()
        .insert(REPLAY_HEADER, HeaderValue::from_static("true"));
    response
}

/// Try to take ownership of the key. Returns the existing record when someone already
/// has it — the insert and the read are one statement, so two concurrent requests cannot
/// both win.
async fn claim(
    pool: &PgPool,
    actor: Uuid,
    operation: &str,
    key: Uuid,
    request_hash: &str,
    ttl_hours: i64,
) -> Result<Option<StoredRequest>, ApiError> {
    let expires_at = Utc::now() + Duration::hours(ttl_hours.max(1));

    let inserted = sqlx::query!(
        r#"
        INSERT INTO coupon.idempotency_requests
            (actor_user_id, operation, idempotency_key, request_hash, status, expires_at)
        VALUES ($1, $2, $3, $4, 'PROCESSING', $5)
        ON CONFLICT (actor_user_id, operation, idempotency_key) DO NOTHING
        RETURNING id
        "#,
        actor,
        operation,
        key,
        request_hash,
        expires_at,
    )
    .fetch_optional(pool)
    .await?;

    if inserted.is_some() {
        return Ok(None);
    }

    let row = sqlx::query!(
        r#"
        SELECT
            request_hash,
            status,
            response_status,
            response_body
        FROM coupon.idempotency_requests
        WHERE actor_user_id = $1 AND operation = $2 AND idempotency_key = $3
        "#,
        actor,
        operation,
        key,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| StoredRequest {
        request_hash: row.request_hash,
        status: RecordStatus::from_db(&row.status),
        response_status: row.response_status,
        response_body: row.response_body.map(|body| body.to_string()),
    }))
}

/// Record the successful response so an identical retry replays it.
async fn finish(pool: &PgPool, actor: Uuid, operation: &str, key: Uuid, status: u16, body: &[u8]) {
    let json: Option<serde_json::Value> = serde_json::from_slice(body).ok();

    let result = sqlx::query!(
        r#"
        UPDATE coupon.idempotency_requests
        SET status = 'COMPLETED',
            response_status = $4,
            response_body = $5,
            completed_at = clock_timestamp()
        WHERE actor_user_id = $1 AND operation = $2 AND idempotency_key = $3
        "#,
        actor,
        operation,
        key,
        i32::from(status),
        json,
    )
    .execute(pool)
    .await;

    // A bookkeeping failure must not turn a successful change into an error response;
    // the worst case is that a retry re-runs a handler that is already idempotent.
    if let Err(error) = result {
        tracing::error!(%error, operation, "failed to record idempotent response");
    }
}

/// Drop the reservation after a failure so the key can be reused with a corrected body.
async fn release(pool: &PgPool, actor: Uuid, operation: &str, key: Uuid) {
    let result = sqlx::query!(
        r#"
        DELETE FROM coupon.idempotency_requests
        WHERE actor_user_id = $1 AND operation = $2 AND idempotency_key = $3
          AND status = 'PROCESSING'
        "#,
        actor,
        operation,
        key,
    )
    .execute(pool)
    .await;

    if let Err(error) = result {
        tracing::error!(%error, operation, "failed to release idempotency key");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(status: RecordStatus, hash: &str) -> StoredRequest {
        StoredRequest {
            request_hash: hash.to_owned(),
            status,
            response_status: Some(201),
            response_body: Some(r#"{"data":{"id":"u1"}}"#.to_owned()),
        }
    }

    #[test]
    fn an_unseen_key_proceeds() {
        assert_eq!(decide(None, "abc"), Decision::Proceed);
    }

    #[test]
    fn the_same_key_and_body_replays_the_stored_response() {
        let decision = decide(Some(&stored(RecordStatus::Completed, "abc")), "abc");

        assert_eq!(
            decision,
            Decision::Replay {
                status: 201,
                body: r#"{"data":{"id":"u1"}}"#.to_owned(),
            }
        );
    }

    #[test]
    fn the_same_key_with_a_different_body_is_a_conflict() {
        let decision = decide(Some(&stored(RecordStatus::Completed, "abc")), "different");

        assert_eq!(decision, Decision::KeyReused);
        assert_eq!(
            ApiError::new(ErrorCode::IdempotencyKeyReused)
                .status()
                .as_u16(),
            409
        );
    }

    #[test]
    fn a_mismatched_body_outranks_the_record_state() {
        // Even mid-flight or after a failure, a different body must never be allowed to
        // borrow another request's key.
        for status in [
            RecordStatus::Processing,
            RecordStatus::Failed,
            RecordStatus::Completed,
        ] {
            assert_eq!(
                decide(Some(&stored(status, "abc")), "different"),
                Decision::KeyReused
            );
        }
    }

    #[test]
    fn an_in_flight_duplicate_is_told_to_wait() {
        assert_eq!(
            decide(Some(&stored(RecordStatus::Processing, "abc")), "abc"),
            Decision::InProgress
        );
        assert!(
            ErrorCode::IdempotencyRequestInProgress.retryable(),
            "the client should be told this one is worth retrying"
        );
    }

    #[test]
    fn a_failed_attempt_frees_the_key_for_an_identical_retry() {
        assert_eq!(
            decide(Some(&stored(RecordStatus::Failed, "abc")), "abc"),
            Decision::Proceed
        );
    }

    #[test]
    fn a_completed_record_without_a_response_reruns_rather_than_inventing_one() {
        let partial = StoredRequest {
            response_status: None,
            response_body: None,
            ..stored(RecordStatus::Completed, "abc")
        };

        assert_eq!(decide(Some(&partial), "abc"), Decision::Proceed);
    }

    #[test]
    fn body_hashes_are_hex_sha256_and_content_sensitive() {
        let hash = hash_body(r#"{"name":"가게"}"#.as_bytes());

        assert_eq!(hash.len(), 64, "must fit the char(64) column");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash, hash_body(r#"{"name":"가게"}"#.as_bytes()));
        assert_ne!(hash, hash_body(r#"{"name":"다른가게"}"#.as_bytes()));
        assert_eq!(hash_body(b"").len(), 64, "an empty body still hashes");
    }
}
