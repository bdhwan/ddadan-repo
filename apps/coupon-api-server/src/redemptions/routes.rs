//! `/owner/redemptions` (§11.4, REDEEM-001…004).

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use uuid::Uuid;
use validator::Validate;

use crate::auth::extractors::StoreOwner;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::middleware::idempotency::IDEMPOTENCY_KEY_HEADER;
use crate::http::rate_limit::Bucket;
use crate::http::response::{ApiMutation, TransactionId};
use crate::redemptions::{
    CancelRedemptionRequest, ConfirmRedemptionRequest, Redemption, Reservation, ReservationRequest,
};
use crate::state::AppState;

pub fn owner_redemption_router() -> Router<AppState> {
    Router::new()
        .route("/owner/redemptions/preview", post(preview_redemption))
        .route(
            "/owner/redemptions/{reservation_id}/confirm",
            post(confirm_redemption),
        )
        .route(
            "/owner/redemptions/{reservation_id}/cancel",
            post(cancel_redemption),
        )
}

/// Check the coupon against the order and hold it for two minutes (§13.3).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/redemptions/preview",
    tag = "redemptions",
    request_body = ReservationRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Reserved, with the expected discount", body = Reservation),
        (status = 404, description = "No such coupon for this customer in this store"),
        (status = 409, description = "COUPON_NOT_AVAILABLE or RESERVATION_ALREADY_ACTIVE"),
        (status = 422, description = "A usage condition is not met"),
    ),
    security(("firebase" = [])),
)]
pub async fn preview_redemption(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    headers: HeaderMap,
    Json(request): Json<ReservationRequest>,
) -> ApiResult<ApiMutation<Reservation>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    let reservation = state
        .redemptions
        .reserve(
            &state.pool,
            &store,
            user.account.user_id,
            idempotency_key(&headers)?,
            &request,
        )
        .await;

    // §16.4 counts *failed* scans, exactly as the accrual path does: a busy till is not
    // an attack, a stream of rejected coupons is worth slowing down (SEC-002).
    if reservation.is_err() {
        state
            .rate_limiter
            .check(
                Bucket::QrResolveFailure,
                &format!("{}:{}", store.id, user.account.user_id),
                state.config.rate_limit_qr_resolve_failure_per_min,
                chrono::Utc::now(),
            )
            .await?;
    }

    let reservation = reservation?;
    Ok(ApiMutation::created(
        reservation.clone(),
        TransactionId(reservation.reservation_id),
    ))
}

/// Approve the use (§13.3 승인, REDEEM-001 step 6–7).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/redemptions/{reservation_id}/confirm",
    tag = "redemptions",
    request_body = ConfirmRedemptionRequest,
    params(
        ("reservation_id" = Uuid, Path, description = "The reservation to approve"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Used", body = Redemption),
        (status = 403, description = "A different owner or session made this reservation"),
        (status = 409, description = "RESERVATION_EXPIRED, or the order changed since the preview"),
        (status = 422, description = "A usage condition is no longer met"),
    ),
    security(("firebase" = [])),
)]
pub async fn confirm_redemption(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(reservation_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<ConfirmRedemptionRequest>,
) -> ApiResult<ApiMutation<Redemption>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    state
        .rate_limiter
        .check(
            Bucket::StampApproval,
            &format!("{}:{}", store.id, user.account.user_id),
            state.config.rate_limit_stamp_approval_per_min,
            chrono::Utc::now(),
        )
        .await?;

    let redemption = state
        .redemptions
        .confirm(
            &state.pool,
            &store,
            user.account.user_id,
            reservation_id,
            idempotency_key(&headers)?,
            &request,
        )
        .await?;

    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    tracing::Span::current().record(
        "transaction_id",
        tracing::field::display(redemption.redemption_id),
    );

    Ok(ApiMutation::created(
        redemption.clone(),
        TransactionId(redemption.redemption_id),
    ))
}

/// Release the hold, or undo the use within ten minutes (REDEEM-002, REDEEM-004).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/redemptions/{reservation_id}/cancel",
    tag = "redemptions",
    request_body = CancelRedemptionRequest,
    params(
        ("reservation_id" = Uuid, Path, description = "The reservation to release or undo"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Released or voided", body = Redemption),
        (status = 404, description = "No such reservation in this store"),
        (status = 422, description = "Past the ten-minute window, or already settled"),
    ),
    security(("firebase" = [])),
)]
pub async fn cancel_redemption(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(reservation_id): Path<Uuid>,
    request: Option<Json<CancelRedemptionRequest>>,
) -> ApiResult<ApiMutation<Redemption>> {
    let request = request.map(|Json(body)| body).unwrap_or_default();
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let redemption = state
        .redemptions
        .cancel(
            &state.pool,
            &store,
            user.account.user_id,
            reservation_id,
            &request,
        )
        .await?;

    Ok(ApiMutation::ok(redemption.clone(), TransactionId::new()))
}

/// Written to `redemption_reservations`/`redemption_transactions`, where a unique
/// constraint makes one key produce at most one hold and one use (§12.6-9).
fn idempotency_key(headers: &HeaderMap) -> ApiResult<Uuid> {
    headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(ErrorCode::IdempotencyKeyRequired))
        .and_then(|raw| {
            Uuid::parse_str(raw.trim()).map_err(|_| ApiError::new(ErrorCode::IdempotencyKeyInvalid))
        })
}
