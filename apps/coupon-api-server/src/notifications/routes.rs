//! `/me/notifications`, `/me/push-subscriptions` (§11.3) and the provider callback
//! endpoint (§15.4).

use axum::body::Bytes;
use axum::extract::{Path, State};

use crate::http::query::Query;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::auth::extractors::CurrentUser;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::pagination::{Page, PageQuery};
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::notifications::delivery::{self, CallbackOutcome, ProviderCallback};
use crate::notifications::{
    Notification, NotificationChannel, NotificationUpdateResult, PushSubscription,
    PushSubscriptionsResponse, RegisterPushSubscriptionRequest, UpdateNotificationsRequest,
};
use crate::state::AppState;

/// How far a callback's own timestamp may be from ours before it is treated as a replay
/// rather than a slow network (§19.3 replay 방지).
pub const CALLBACK_MAX_SKEW_SECS: i64 = 300;

pub fn me_notifications_router() -> Router<AppState> {
    Router::new()
        .route("/me/notifications", get(list_notifications).patch(update_notifications))
        .route(
            "/me/push-subscriptions",
            get(list_push_subscriptions).post(register_push_subscription),
        )
        .route(
            "/me/push-subscriptions/{subscription_id}",
            axum::routing::delete(delete_push_subscription),
        )
}

/// Provider callbacks are **not** on the authenticated tree.
///
/// A delivery provider has no Firebase token and no browser origin, so it authenticates
/// with a signature over the body instead (§15.4). Mounting it beside the authenticated
/// routes and then exempting it would leave the exemption one refactor away from being
/// forgotten; a separate router makes the difference structural.
pub fn notification_webhook_router() -> Router<AppState> {
    Router::new().route(
        "/notifications/callbacks/{provider}",
        post(provider_callback),
    )
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct NotificationListQuery {
    #[serde(default)]
    pub unread_only: bool,
    /// Spelled out rather than flattened: `#[serde(flatten)]` forces `deserialize_any`, and
    /// a query string hands every value over as a string, so `?limit=20` would not parse.
    #[serde(default, deserialize_with = "crate::http::pagination::page_size")]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

/// The consumer's inbox (§11.3, §11.6: 30초 polling).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/notifications",
    tag = "notifications",
    params(
        ("unread_only" = Option<bool>, Query, description = "Only notifications not yet read"),
        ("limit" = Option<u32>, Query, description = "1–100, default 20"),
        ("cursor" = Option<String>, Query, description = "next_cursor from the previous page"),
    ),
    responses((status = 200, description = "Newest first", body = Page<Notification>)),
    security(("firebase" = [])),
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<NotificationListQuery>,
) -> ApiResult<ApiOk<Page<Notification>>> {
    Ok(ApiOk(
        state
            .notifications
            .list(
                &state.pool,
                user.account.user_id,
                &PageQuery {
                    limit: query.limit,
                    cursor: query.cursor,
                },
                query.unread_only,
            )
            .await?,
    ))
}

/// Mark read, mark unread, or clear (§11.3).
///
/// Clearing is a view-level act: §15.1 keeps the ledger as the record of what happened, so
/// an emptied inbox still leaves every coupon and every transaction exactly where it was.
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/me/notifications",
    tag = "notifications",
    request_body = UpdateNotificationsRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 200, description = "How many rows changed", body = NotificationUpdateResult),
        (status = 400, description = "No notification was named"),
    ),
    security(("firebase" = [])),
)]
pub async fn update_notifications(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(request): Json<UpdateNotificationsRequest>,
) -> ApiResult<ApiMutation<NotificationUpdateResult>> {
    request.validate()?;

    let result = state
        .notifications
        .update(&state.pool, user.account.user_id, &request)
        .await?;

    Ok(ApiMutation::ok(result, TransactionId::new()))
}

/// The browsers this account has registered for web push (§15.1-2).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/push-subscriptions",
    tag = "notifications",
    responses((status = 200, description = "Registered browsers", body = PushSubscriptionsResponse)),
    security(("firebase" = [])),
)]
pub async fn list_push_subscriptions(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<PushSubscriptionsResponse>> {
    Ok(ApiOk(PushSubscriptionsResponse {
        subscriptions: state
            .notifications
            .list_push_subscriptions(&state.pool, user.account.user_id)
            .await?,
    }))
}

/// Register this browser's FCM token.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/me/push-subscriptions",
    tag = "notifications",
    request_body = RegisterPushSubscriptionRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Registered", body = PushSubscription),
        (status = 400, description = "Malformed token"),
    ),
    security(("firebase" = [])),
)]
pub async fn register_push_subscription(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(request): Json<RegisterPushSubscriptionRequest>,
) -> ApiResult<ApiMutation<PushSubscription>> {
    request.validate()?;

    let subscription = state
        .notifications
        .register_push_subscription(&state.pool, user.account.user_id, &request)
        .await?;

    Ok(ApiMutation::created(subscription, TransactionId::new()))
}

/// Stop pushing to one browser (NOTIFY-001 특정 채널 철회).
#[utoipa::path(
    delete,
    path = "/api/coupon/v1/me/push-subscriptions/{subscription_id}",
    tag = "notifications",
    params(
        ("subscription_id" = Uuid, Path, description = "Subscription id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Revoked", body = PushSubscriptionsResponse),
        (status = 404, description = "No such subscription for this account"),
    ),
    security(("firebase" = [])),
)]
pub async fn delete_push_subscription(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(subscription_id): Path<Uuid>,
) -> ApiResult<ApiMutation<PushSubscriptionsResponse>> {
    state
        .notifications
        .revoke_push_subscription(&state.pool, user.account.user_id, subscription_id)
        .await?;

    Ok(ApiMutation::ok(
        PushSubscriptionsResponse {
            subscriptions: state
                .notifications
                .list_push_subscriptions(&state.pool, user.account.user_id)
                .await?,
        },
        TransactionId::new(),
    ))
}

/// What a provider posts back (§15.4).
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProviderCallbackBody {
    /// The provider's identifier for *this callback*. Its uniqueness is what makes a
    /// replay recognisable.
    pub event_id: String,
    /// The identifier we stored when the send was accepted.
    pub provider_reference: String,
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CallbackAck {
    pub outcome: CallbackOutcome,
}

/// Accept a delivery callback.
///
/// Three checks, in this order, because each one narrows what the next may trust:
///
/// 1. the timestamp is inside the skew window — an old signed body cannot be replayed;
/// 2. the HMAC over `timestamp.body` verifies in constant time;
/// 3. the `provider_reference` names a delivery of ours on this channel.
///
/// Failing 1 or 2 answers 401 without saying which: distinguishing "your signature is
/// wrong" from "your timestamp is stale" tells an attacker which half to work on. Failing 3
/// is a 200 with `IGNORED` — the provider did nothing wrong, and retrying will not help it.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/notifications/callbacks/{provider}",
    tag = "notifications",
    request_body = ProviderCallbackBody,
    params(
        ("provider" = String, Path, description = "Provider slug, e.g. fcm or alimtalk"),
        ("X-Signature" = String, Header, description = "Hex HMAC-SHA256 over `<timestamp>.<body>`"),
        ("X-Signature-Timestamp" = String, Header, description = "RFC 3339, within 5 minutes"),
    ),
    responses(
        (status = 200, description = "Recorded", body = CallbackAck),
        (status = 401, description = "Signature or timestamp rejected"),
        (status = 503, description = "No callback secret is configured"),
    ),
)]
pub async fn provider_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<ApiOk<CallbackAck>> {
    let Some(secret) = state.config.notification_callback_secret.as_deref() else {
        // Refusing is the only safe answer: with no secret there is no way to tell a
        // provider from anyone else, and applying an unauthenticated status change would
        // let a stranger mark every message delivered.
        return Err(ApiError::with_message(
            ErrorCode::DependencyUnavailable,
            "알림 콜백을 처리할 수 없습니다.",
        )
        .internal("COUPON_NOTIFICATION_CALLBACK_SECRET is not configured"));
    };

    let signature = header(&headers, "x-signature")?;
    let timestamp = header(&headers, "x-signature-timestamp")?;
    let signed_at = DateTime::parse_from_rfc3339(&timestamp)
        .map_err(|_| ApiError::new(ErrorCode::WebhookSignatureInvalid))?
        .with_timezone(&Utc);

    if !verify_callback(secret, &timestamp, &body, &signature, Utc::now(), signed_at) {
        return Err(ApiError::new(ErrorCode::WebhookSignatureInvalid));
    }

    let parsed: ProviderCallbackBody = serde_json::from_slice(&body)
        .map_err(|error| ApiError::new(ErrorCode::InvalidRequest).internal(error.to_string()))?;

    let channel = match provider.as_str() {
        "fcm" | "fcm-web-push" => NotificationChannel::FcmWebPush,
        "alimtalk" | "kakao-alimtalk" => NotificationChannel::KakaoAlimtalk,
        _ => return Err(ApiError::new(ErrorCode::NotFound)),
    };

    let outcome = delivery::record_callback(
        &state.pool,
        &ProviderCallback {
            channel,
            provider: provider.clone(),
            provider_event_id: parsed.event_id,
            provider_reference: parsed.provider_reference,
            reported_status: parsed.status,
            signed_at,
            payload: serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
            signature_valid: true,
        },
    )
    .await?;

    tracing::info!(provider, ?outcome, "notifications.callback");
    Ok(ApiOk(CallbackAck { outcome }))
}

/// Signature and freshness, together.
///
/// Split out so §19.3's 서명 실패와 replay 방지 cases can be exercised without a server.
pub fn verify_callback(
    secret: &str,
    timestamp: &str,
    body: &[u8],
    signature_hex: &str,
    now: DateTime<Utc>,
    signed_at: DateTime<Utc>,
) -> bool {
    if (now - signed_at).num_seconds().abs() > CALLBACK_MAX_SKEW_SECS {
        return false;
    }

    let Ok(expected) = hex::decode(signature_hex.trim()) else {
        return false;
    };

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    // The timestamp is *inside* the signed material. Signing only the body would let a
    // captured payload be replayed with a fresh header, which is exactly what the skew
    // window is meant to prevent.
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);

    // `verify_slice` is constant-time; comparing hex strings would not be.
    mac.verify_slice(&expected).is_ok()
}

fn header(headers: &HeaderMap, name: &str) -> ApiResult<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::new(ErrorCode::WebhookSignatureInvalid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, timestamp: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn a_correctly_signed_fresh_callback_verifies() {
        let body = br#"{"event_id":"e-1"}"#;
        let timestamp = "2026-08-12T00:00:00Z";
        let signature = sign("s3cret", timestamp, body);

        assert!(verify_callback(
            "s3cret",
            timestamp,
            body,
            &signature,
            at("2026-08-12T00:00:10Z"),
            at(timestamp),
        ));
    }

    #[test]
    fn a_tampered_body_fails_even_with_a_valid_looking_signature() {
        let timestamp = "2026-08-12T00:00:00Z";
        let signature = sign("s3cret", timestamp, br#"{"status":"FAILED"}"#);

        assert!(!verify_callback(
            "s3cret",
            timestamp,
            br#"{"status":"DELIVERED"}"#,
            &signature,
            at("2026-08-12T00:00:10Z"),
            at(timestamp),
        ));
    }

    #[test]
    fn a_replayed_callback_falls_outside_the_window() {
        // §19.3: webhook replay 방지. The signature is still valid — that is the point.
        let body = br#"{"event_id":"e-1"}"#;
        let timestamp = "2026-08-12T00:00:00Z";
        let signature = sign("s3cret", timestamp, body);

        assert!(!verify_callback(
            "s3cret",
            timestamp,
            body,
            &signature,
            at("2026-08-12T00:10:00Z"),
            at(timestamp),
        ));
    }

    #[test]
    fn a_future_timestamp_is_rejected_too() {
        let body = br#"{"event_id":"e-1"}"#;
        let timestamp = "2026-08-12T01:00:00Z";
        let signature = sign("s3cret", timestamp, body);

        assert!(!verify_callback(
            "s3cret",
            timestamp,
            body,
            &signature,
            at("2026-08-12T00:00:00Z"),
            at(timestamp),
        ));
    }

    #[test]
    fn the_wrong_secret_fails() {
        let body = br#"{"event_id":"e-1"}"#;
        let timestamp = "2026-08-12T00:00:00Z";
        let signature = sign("other", timestamp, body);

        assert!(!verify_callback(
            "s3cret",
            timestamp,
            body,
            &signature,
            at("2026-08-12T00:00:01Z"),
            at(timestamp),
        ));
    }

    #[test]
    fn a_non_hex_signature_is_rejected_rather_than_panicking() {
        assert!(!verify_callback(
            "s3cret",
            "2026-08-12T00:00:00Z",
            b"{}",
            "not-hex!!",
            at("2026-08-12T00:00:01Z"),
            at("2026-08-12T00:00:00Z"),
        ));
    }

    #[test]
    fn the_timestamp_is_part_of_the_signed_material() {
        // A signature made over one timestamp must not verify under another, even when the
        // replacement is inside the freshness window.
        let body = br#"{"event_id":"e-1"}"#;
        let signature = sign("s3cret", "2026-08-12T00:00:00Z", body);

        assert!(!verify_callback(
            "s3cret",
            "2026-08-12T00:00:30Z",
            body,
            &signature,
            at("2026-08-12T00:00:31Z"),
            at("2026-08-12T00:00:30Z"),
        ));
    }
}
