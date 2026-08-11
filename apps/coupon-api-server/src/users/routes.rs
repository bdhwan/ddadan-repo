//! `/users/*` and `/me` endpoints (§11.2).

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::auth::extractors::{Authenticated, CurrentUser};
use crate::error::ApiResult;
use crate::http::concurrency;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::state::AppState;
use crate::users::{BootstrapRequest, RolesResponse, UpdateProfileRequest, UserProfile};
use validator::Validate;

pub fn users_router() -> Router<AppState> {
    Router::new().route("/users/bootstrap", post(bootstrap))
}

pub fn me_router() -> Router<AppState> {
    Router::new()
        .route("/me", get(get_me).patch(patch_me))
        .route("/me/roles", get(get_roles))
}

/// Create the internal account for a signed-in Firebase user.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/users/bootstrap",
    tag = "users",
    request_body = BootstrapRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Account created", body = UserProfile),
        (status = 200, description = "Account already existed", body = UserProfile),
        (status = 401, description = "Missing or invalid Firebase ID token"),
    ),
    security(("firebase" = [])),
)]
pub async fn bootstrap(
    State(state): State<AppState>,
    Authenticated(context): Authenticated,
    Json(request): Json<BootstrapRequest>,
) -> ApiResult<ApiMutation<UserProfile>> {
    request.validate()?;

    let transaction_id = TransactionId::new();
    let (profile, created) = state
        .users
        .bootstrap(&state.pool, &context.token, &request)
        .await?;

    tracing::Span::current().record("actor_id", tracing::field::display(profile.id));
    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    tracing::info!(created, "users.bootstrap");

    // 201 only when this request created the account; a repeat is a plain 200.
    Ok(if created {
        ApiMutation::created(profile, transaction_id)
    } else {
        ApiMutation::ok(profile, transaction_id)
    })
}

/// My profile.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me",
    tag = "users",
    responses(
        (status = 200, description = "Current profile", body = UserProfile),
        (status = 404, description = "Signed in but not bootstrapped yet"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_me(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<UserProfile>> {
    let profile = state
        .users
        .find_by_id(&state.pool, user.account.user_id)
        .await?
        .ok_or_else(crate::error::ApiError::not_found)?;

    Ok(ApiOk(profile))
}

/// Update my profile.
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/me",
    tag = "users",
    request_body = UpdateProfileRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
        ("If-Match" = Option<String>, Header, description = "Expected version, e.g. \"3\""),
    ),
    responses(
        (status = 200, description = "Updated profile", body = UserProfile),
        (status = 409, description = "Someone else changed the profile first"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_me(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Json(request): Json<UpdateProfileRequest>,
) -> ApiResult<ApiMutation<UserProfile>> {
    request.validate()?;

    let expected_version = concurrency::expected_version(&headers, request.version)?;
    let transaction_id = TransactionId::new();

    let profile = state
        .users
        .update_profile(
            &state.pool,
            user.account.user_id,
            &request,
            expected_version,
        )
        .await?;

    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    tracing::info!("users.profile_updated");

    Ok(ApiMutation::ok(profile, transaction_id))
}

/// My active roles, with the store each store-scoped role refers to.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/roles",
    tag = "users",
    responses((status = 200, description = "Active roles", body = RolesResponse)),
    security(("firebase" = [])),
)]
pub async fn get_roles(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<RolesResponse>> {
    let roles = state.users.roles(&state.pool, user.account.user_id).await?;
    Ok(ApiOk(RolesResponse { roles }))
}
