//! `/admin/retention-policies` and `/admin/privacy/*` (§11.5, §17.3, ADMIN-006).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use validator::Validate;

use crate::auth::extractors::RecentlyAuthenticated;
use crate::error::ApiResult;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::privacy::{
    ErasureRecord, ErasuresResponse, ReapplyResult, RequestErasureRequest,
    RetentionPoliciesResponse, RetentionPolicy, UpdateRetentionPolicyRequest,
};
use crate::state::AppState;
use crate::users::AccountRole;

/// How many erasure records the operations view returns.
const ERASURE_PAGE: i64 = 100;

pub fn admin_privacy_router() -> Router<AppState> {
    Router::new()
        .route("/admin/retention-policies", get(list_retention_policies))
        .route(
            "/admin/retention-policies/{data_category}",
            axum::routing::patch(patch_retention_policy),
        )
        .route(
            "/admin/privacy/erasures",
            get(list_erasures).post(request_erasure),
        )
        .route("/admin/privacy/erasures/reapply", post(reapply_erasures))
}

/// The configured retention periods (§17.3).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/retention-policies",
    tag = "admin",
    responses((status = 200, description = "Per-category retention", body = RetentionPoliciesResponse)),
    security(("firebase" = [])),
)]
pub async fn list_retention_policies(
    State(state): State<AppState>,
    _admin: crate::auth::extractors::SystemAdmin,
) -> ApiResult<ApiOk<RetentionPoliciesResponse>> {
    Ok(ApiOk(RetentionPoliciesResponse {
        policies: state.privacy.policies(&state.pool).await?,
    }))
}

/// Change one category's retention period.
///
/// §17.3 makes these configuration rather than constants precisely so they can change
/// without a deploy — but a shortened period destroys evidence, so it demands a fresh
/// sign-in, a written legal basis and a reason, and it lands in the audit trail.
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/admin/retention-policies/{data_category}",
    tag = "admin",
    request_body = UpdateRetentionPolicyRequest,
    params(
        ("data_category" = String, Path, description = "PROFILE, CONSENT, TRANSACTION, …"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Updated", body = RetentionPolicy),
        (status = 403, description = "This role may not change retention"),
        (status = 404, description = "No such category"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_retention_policy(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(data_category): Path<String>,
    Json(request): Json<UpdateRetentionPolicyRequest>,
) -> ApiResult<ApiMutation<RetentionPolicy>> {
    request.validate()?;
    let admin = crate::admin::routes::require_admin(&user)?;
    admin.require_any(&[AccountRole::Security, AccountRole::SuperAdmin])?;

    let policy = state
        .privacy
        .update_policy(
            &state.pool,
            user.account.user_id,
            &data_category,
            &request,
        )
        .await?;

    Ok(ApiMutation::ok(policy, TransactionId::new()))
}

/// Erasure requests and their outcomes (ADMIN-006).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/privacy/erasures",
    tag = "admin",
    responses((status = 200, description = "Newest first", body = ErasuresResponse)),
    security(("firebase" = [])),
)]
pub async fn list_erasures(
    State(state): State<AppState>,
    _admin: crate::auth::extractors::SystemAdmin,
) -> ApiResult<ApiOk<ErasuresResponse>> {
    Ok(ApiOk(ErasuresResponse {
        erasures: state.privacy.list_erasures(&state.pool, ERASURE_PAGE).await?,
    }))
}

/// Queue an erasure (ADMIN-006 삭제).
///
/// The request does not erase anything: it registers the obligation, records the case that
/// authorises it, and queues a job that runs after the grace period. §17.3's hold check
/// happens both here and in the job, because a dispute can open in between.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/privacy/erasures",
    tag = "admin",
    request_body = RequestErasureRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Queued", body = ErasureRecord),
        (status = 403, description = "This role may not request erasure"),
        (status = 404, description = "No such subject or case"),
        (status = 422, description = "LEGAL_HOLD_ACTIVE"),
    ),
    security(("firebase" = [])),
)]
pub async fn request_erasure(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Json(request): Json<RequestErasureRequest>,
) -> ApiResult<ApiMutation<ErasureRecord>> {
    request.validate()?;
    let admin = crate::admin::routes::require_admin(&user)?;
    admin.require_any(&[AccountRole::Security, AccountRole::SuperAdmin])?;

    let record = state
        .privacy
        .request_erasure(&state.pool, user.account.user_id, &request)
        .await?;

    Ok(ApiMutation::created(record.clone(), TransactionId(record.id)))
}

/// Re-apply every completed erasure (§18.5).
///
/// The runbook step after a restore. Safe to run at any time — the erasure is idempotent —
/// and `reapplied > 0` outside a restore means something wrote an erased subject back,
/// which is worth an alert rather than a shrug.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/privacy/erasures/reapply",
    tag = "admin",
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 200, description = "How many subjects had to be erased again", body = ReapplyResult),
        (status = 403, description = "This role may not replay the deletion ledger"),
    ),
    security(("firebase" = [])),
)]
pub async fn reapply_erasures(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
) -> ApiResult<ApiMutation<ReapplyResult>> {
    let admin = crate::admin::routes::require_admin(&user)?;
    admin.require_any(&[AccountRole::Security, AccountRole::SuperAdmin])?;

    let result = state.privacy.reapply(&state.pool).await?;
    tracing::info!(
        examined = result.examined,
        reapplied = result.reapplied,
        "privacy.reapply"
    );

    Ok(ApiMutation::ok(result, TransactionId::new()))
}
