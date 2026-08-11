//! `/owner/loyalty-policies`, `/owner/scan/resolve` and `/owner/stamp-transactions`
//! (§11.4).

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;
use validator::Validate;

use crate::auth::extractors::StoreOwner;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::concurrency;
use crate::http::middleware::idempotency::IDEMPOTENCY_KEY_HEADER;
use crate::http::rate_limit::Bucket;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::loyalty::{
    ConfirmStampRequest, CreatePolicyRequest, LoyaltyPoliciesResponse, LoyaltyPolicy,
    PublishPolicyRequest, ScanRequest, ScanResolution, StampPreview, StampPreviewRequest,
    StampTransaction, UpdatePolicyRequest, VoidStampRequest,
};
use crate::state::AppState;

pub fn owner_loyalty_router() -> Router<AppState> {
    Router::new()
        .route(
            "/owner/loyalty-policies",
            get(list_policies).post(create_policy),
        )
        .route(
            "/owner/loyalty-policies/{policy_id}",
            axum::routing::patch(patch_policy),
        )
        .route(
            "/owner/loyalty-policies/{policy_id}/publish",
            post(publish_policy),
        )
        .route("/owner/scan/resolve", post(resolve_scan))
        .route(
            "/owner/stamp-transactions/preview",
            post(preview_stamp_transaction),
        )
        .route("/owner/stamp-transactions", post(confirm_stamp_transaction))
        .route(
            "/owner/stamp-transactions/{transaction_id}/void",
            post(void_stamp_transaction),
        )
}

/// Every version of my stamp policy.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/loyalty-policies",
    tag = "loyalty",
    responses((status = 200, description = "Policy versions", body = LoyaltyPoliciesResponse)),
    security(("firebase" = [])),
)]
pub async fn list_policies(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
) -> ApiResult<ApiOk<LoyaltyPoliciesResponse>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let policies = state.loyalty_policies.list(&state.pool, store.id).await?;

    Ok(ApiOk(policies))
}

/// Draft the next version.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/loyalty-policies",
    tag = "loyalty",
    request_body = CreatePolicyRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Draft created", body = LoyaltyPolicy),
        (status = 400, description = "A value is outside its §8.1 range"),
    ),
    security(("firebase" = [])),
)]
pub async fn create_policy(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Json(request): Json<CreatePolicyRequest>,
) -> ApiResult<ApiMutation<LoyaltyPolicy>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let policy = state
        .loyalty_policies
        .create(&state.pool, store.id, user.account.user_id, &request)
        .await?;

    tracing::info!(policy_id = %policy.id, version_no = policy.version_no, "loyalty.policy_drafted");
    Ok(ApiMutation::created(policy, TransactionId::new()))
}

/// Edit a draft. An active version is never edited in place (STAMP-008).
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/owner/loyalty-policies/{policy_id}",
    tag = "loyalty",
    request_body = UpdatePolicyRequest,
    params(
        ("policy_id" = Uuid, Path, description = "Policy version id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
        ("If-Match" = Option<String>, Header, description = "Expected version"),
    ),
    responses(
        (status = 200, description = "Updated draft", body = LoyaltyPolicy),
        (status = 422, description = "This version is not a draft"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_policy(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(policy_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<UpdatePolicyRequest>,
) -> ApiResult<ApiMutation<LoyaltyPolicy>> {
    request.validate()?;
    let expected_version = concurrency::expected_version(&headers, request.version)?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let policy = state
        .loyalty_policies
        .update(&state.pool, store.id, policy_id, &request, expected_version)
        .await?;

    tracing::info!(policy_id = %policy.id, "loyalty.policy_updated");
    Ok(ApiMutation::ok(policy, TransactionId::new()))
}

/// Publish a draft, now or at a chosen instant.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish",
    tag = "loyalty",
    request_body = PublishPolicyRequest,
    params(
        ("policy_id" = Uuid, Path, description = "Policy version id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Published or scheduled", body = LoyaltyPolicy),
        (status = 400, description = "The reward is missing wording customers must see"),
        (status = 409, description = "Another version is already scheduled"),
    ),
    security(("firebase" = [])),
)]
pub async fn publish_policy(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(policy_id): Path<Uuid>,
    request: Option<Json<PublishPolicyRequest>>,
) -> ApiResult<ApiMutation<LoyaltyPolicy>> {
    let request = request.map(|Json(body)| body).unwrap_or_default();

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let policy = state
        .loyalty_policies
        .publish(&state.pool, store.id, policy_id, &request)
        .await?;

    tracing::info!(
        policy_id = %policy.id,
        status = ?policy.status,
        "loyalty.policy_published"
    );
    Ok(ApiMutation::ok(policy, TransactionId::new()))
}

/// Read a customer's QR and describe who they are — pseudonymously (§11.4, WALLET-005).
///
/// Does not consume the QR: the owner may still abandon the sale.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/scan/resolve",
    tag = "loyalty",
    request_body = ScanRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 200, description = "Pseudonymous customer and stamp board", body = ScanResolution),
        (status = 409, description = "That QR has already been used"),
        (status = 422, description = "The QR could not be accepted"),
        (status = 429, description = "Too many failed scans"),
    ),
    security(("firebase" = [])),
)]
pub async fn resolve_scan(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Json(request): Json<ScanRequest>,
) -> ApiResult<ApiMutation<ScanResolution>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    let resolution = state
        .loyalty_stamps
        .resolve_scan(&state.pool, &store, &request)
        .await;

    // §16.4 limits *failed* resolutions, not successful ones: a busy lunch rush is not an
    // attack, but a stream of rejected tokens is worth slowing down (SEC-002).
    if resolution.is_err() {
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

    Ok(ApiMutation::ok(resolution?, TransactionId::new()))
}

/// Check an order against the active policy and show what would happen.
///
/// Returns 200 with `approvable: false` and the reasons when the accrual would be
/// refused, because the scan screen has to display them (§6.3).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/stamp-transactions/preview",
    tag = "loyalty",
    request_body = StampPreviewRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 200, description = "What approving would do", body = StampPreview),
        (status = 422, description = "The QR or the store state makes a preview impossible"),
    ),
    security(("firebase" = [])),
)]
pub async fn preview_stamp_transaction(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Json(request): Json<StampPreviewRequest>,
) -> ApiResult<ApiMutation<StampPreview>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let preview = state
        .loyalty_stamps
        .preview(&state.pool, &store, &request)
        .await?;

    Ok(ApiMutation::ok(preview, TransactionId::new()))
}

/// Approve the accrual (§13.1).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/stamp-transactions",
    tag = "loyalty",
    request_body = ConfirmStampRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Stamps granted, and any reward earned", body = StampTransaction),
        (status = 409, description = "The QR was already used, or this looks like a duplicate"),
        (status = 422, description = "A policy condition is not met"),
        (status = 429, description = "Too many approvals"),
    ),
    security(("firebase" = [])),
)]
pub async fn confirm_stamp_transaction(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    headers: HeaderMap,
    Json(request): Json<ConfirmStampRequest>,
) -> ApiResult<ApiMutation<StampTransaction>> {
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

    let transaction = state
        .loyalty_stamps
        .confirm(
            &state.pool,
            &store,
            user.account.user_id,
            idempotency_key(&headers)?,
            &request,
        )
        .await?;

    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    tracing::Span::current().record(
        "transaction_id",
        tracing::field::display(transaction.transaction_id),
    );

    Ok(ApiMutation::created(
        transaction.clone(),
        TransactionId(transaction.transaction_id),
    ))
}

/// Reverse an accrual within the owner's own window (STAMP-007).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/stamp-transactions/{transaction_id}/void",
    tag = "loyalty",
    request_body = VoidStampRequest,
    params(
        ("transaction_id" = Uuid, Path, description = "The accrual to reverse"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Reversed", body = StampTransaction),
        (status = 404, description = "No such accrual in this store"),
        (status = 422, description = "Past the window, or a reward has moved on"),
    ),
    security(("firebase" = [])),
)]
pub async fn void_stamp_transaction(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(transaction_id): Path<Uuid>,
    request: Option<Json<VoidStampRequest>>,
) -> ApiResult<ApiMutation<StampTransaction>> {
    let request = request.map(|Json(body)| body).unwrap_or_default();
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let transaction = state
        .loyalty_stamps
        .void(
            &state.pool,
            &store,
            user.account.user_id,
            transaction_id,
            &request,
        )
        .await?;

    Ok(ApiMutation::ok(
        transaction.clone(),
        TransactionId(transaction.transaction_id),
    ))
}

/// The accrual needs the key itself, not just the middleware's bookkeeping: the same key
/// is written to `stamp_transactions`, where a unique constraint makes one key produce at
/// most one accrual even if the middleware is bypassed (§12.6-9).
fn idempotency_key(headers: &HeaderMap) -> ApiResult<Uuid> {
    headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(ErrorCode::IdempotencyKeyRequired))
        .and_then(|raw| {
            Uuid::parse_str(raw.trim())
                .map_err(|_| ApiError::new(ErrorCode::IdempotencyKeyInvalid))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn the_accrual_reads_the_same_key_the_middleware_recorded() {
        let key = Uuid::new_v4();
        let mut headers = HeaderMap::new();
        headers.insert(
            IDEMPOTENCY_KEY_HEADER,
            HeaderValue::from_str(&key.to_string()).expect("valid header"),
        );

        assert_eq!(idempotency_key(&headers).expect("parses"), key);
    }

    #[test]
    fn a_missing_or_malformed_key_is_a_client_error() {
        assert_eq!(
            idempotency_key(&HeaderMap::new()).expect_err("missing").code,
            ErrorCode::IdempotencyKeyRequired
        );

        let mut headers = HeaderMap::new();
        headers.insert(IDEMPOTENCY_KEY_HEADER, HeaderValue::from_static("not-a-uuid"));
        assert_eq!(
            idempotency_key(&headers).expect_err("malformed").code,
            ErrorCode::IdempotencyKeyInvalid
        );
    }
}
