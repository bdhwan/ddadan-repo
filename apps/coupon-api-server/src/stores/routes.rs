//! `/owner/store` endpoints (§11.4).

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use validator::Validate;

use crate::auth::extractors::{CurrentUser, RecentlyAuthenticated};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::concurrency;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::state::AppState;
use crate::stores::{CreateStoreRequest, StoreResponse, SubmitReviewRequest, UpdateStoreRequest};

pub fn owner_store_router() -> Router<AppState> {
    Router::new()
        .route(
            "/owner/store",
            get(get_store).post(create_store).patch(patch_store),
        )
        .route("/owner/store/submit-review", post(submit_review))
}

/// Create my store draft. One per account (§12.6-1).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/store",
    tag = "stores",
    request_body = CreateStoreRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Store draft created", body = StoreResponse),
        (status = 409, description = "This account already has a store, or the slug is taken"),
    ),
    security(("firebase" = [])),
)]
pub async fn create_store(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(request): Json<CreateStoreRequest>,
) -> ApiResult<ApiMutation<StoreResponse>> {
    request.validate()?;

    let transaction_id = TransactionId::new();
    let store = state
        .stores
        .create(&state.pool, user.account.user_id, &request)
        .await?;

    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    tracing::info!(slug = %store.slug, "stores.created");

    Ok(ApiMutation::created(store, transaction_id))
}

/// My store.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/store",
    tag = "stores",
    responses(
        (status = 200, description = "The caller's store", body = StoreResponse),
        (status = 404, description = "No store yet"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_store(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<StoreResponse>> {
    let store = state
        .stores
        .find_by_owner(&state.pool, user.account.user_id)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::StoreNotFound))?;

    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    Ok(ApiOk(store))
}

/// Update my store.
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/owner/store",
    tag = "stores",
    request_body = UpdateStoreRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
        ("If-Match" = Option<String>, Header, description = "Expected version, e.g. \"3\""),
    ),
    responses(
        (status = 200, description = "Updated store", body = StoreResponse),
        (status = 409, description = "Someone else changed the store first"),
        (status = 422, description = "The store cannot be edited in its current state"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_store(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Json(request): Json<UpdateStoreRequest>,
) -> ApiResult<ApiMutation<StoreResponse>> {
    request.validate()?;

    let expected_version = concurrency::expected_version(&headers, request.version)?;
    let transaction_id = TransactionId::new();

    let store = state
        .stores
        .update(
            &state.pool,
            user.account.user_id,
            &request,
            expected_version,
        )
        .await?;

    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    tracing::info!("stores.updated");

    Ok(ApiMutation::ok(store, transaction_id))
}

/// Submit my store for review.
///
/// High-risk: it freezes the business identity a reviewer will act on, so it requires a
/// recent sign-in (§9.3).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/store/submit-review",
    tag = "stores",
    request_body = SubmitReviewRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Store queued for review", body = StoreResponse),
        (status = 403, description = "Sign-in is too old for this operation"),
        (status = 409, description = "A review is already pending"),
        (status = 422, description = "Required information is still missing"),
    ),
    security(("firebase" = [])),
)]
pub async fn submit_review(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    request: Option<Json<SubmitReviewRequest>>,
) -> ApiResult<ApiMutation<StoreResponse>> {
    let request = request.map(|Json(body)| body).unwrap_or_default();
    request.validate()?;

    let transaction_id = TransactionId::new();
    let store = state
        .stores
        .submit_for_review(&state.pool, user.account.user_id, &request)
        .await?;

    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    tracing::info!("stores.review_submitted");

    Ok(ApiMutation::ok(store, transaction_id))
}
