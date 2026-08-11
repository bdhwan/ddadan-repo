//! `/admin/*` (§11.5).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::admin::{
    AdjustmentPreview, AdjustmentPreviewRequest, AdminTransactionDetail, ApprovedAdjustment,
    ApproveAdjustmentRequest,
};
use crate::auth::extractors::{RecentlyAuthenticated, SystemAdmin};
use crate::campaigns::Campaign;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::jobs::{EnqueuedJob, JobDetail, JobQuery, JobStatus, JobSummary};
use crate::state::AppState;
use crate::users::AccountRole;

/// The roles §3.3 lets change the ledger or stop a live campaign. `SUPPORT` is
/// deliberately absent: it reads, it does not act.
const CHANGE_ROLES: [AccountRole; 3] = [
    AccountRole::Operations,
    AccountRole::Security,
    AccountRole::SuperAdmin,
];

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/admin/transactions/{transaction_id}", get(get_transaction))
        .route("/admin/adjustments/preview", post(preview_adjustment))
        .route("/admin/adjustments", post(approve_adjustment))
        .route(
            "/admin/campaigns/{campaign_id}/emergency-stop",
            post(emergency_stop_campaign),
        )
        .route(
            "/admin/campaigns/{campaign_id}/revoke-job",
            post(revoke_campaign_coupons),
        )
        .route("/admin/jobs", get(list_jobs))
        .route("/admin/jobs/{job_id}", get(get_job))
        .route("/admin/jobs/{job_id}/retry", post(retry_job))
}

/// One transaction, its ledger and its whole event history (§2.1 목표 5).
///
/// Readable by every administrative role: answering "what happened to this customer" is
/// support's daily work, and the response carries no unmasked personal data.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/transactions/{transaction_id}",
    tag = "admin",
    params(("transaction_id" = Uuid, Path, description = "Stamp transaction id")),
    responses(
        (status = 200, description = "Ledger and timeline", body = AdminTransactionDetail),
        (status = 403, description = "Not an administrator"),
        (status = 404, description = "No such transaction"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_transaction(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Path(transaction_id): Path<Uuid>,
) -> ApiResult<ApiOk<AdminTransactionDetail>> {
    let detail = state
        .admin
        .transaction_detail(&state.pool, admin.user.account.user_id, transaction_id)
        .await?;

    tracing::info!(%transaction_id, "admin.transaction_viewed");
    Ok(ApiOk(detail))
}

/// Simulate a correction before anyone is allowed to run it (ADMIN-003, §13.4).
///
/// Restricted to the roles §3.3 lets request a correction; approving and executing one is
/// a separate act with its own separation-of-duties rule.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/adjustments/preview",
    tag = "admin",
    request_body = AdjustmentPreviewRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "A snapshot of what the correction would do", body = AdjustmentPreview),
        (status = 403, description = "This role may not request corrections"),
        (status = 404, description = "No such case or transaction"),
    ),
    security(("firebase" = [])),
)]
pub async fn preview_adjustment(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Json(request): Json<AdjustmentPreviewRequest>,
) -> ApiResult<ApiMutation<AdjustmentPreview>> {
    request.validate()?;
    admin.require_any(&CHANGE_ROLES)?;

    let preview = state
        .admin
        .preview_adjustment(&state.pool, admin.user.account.user_id, &request)
        .await?;

    tracing::info!(
        adjustment_id = %preview.adjustment_id,
        executable = preview.executable,
        "admin.adjustment_previewed"
    );

    Ok(ApiMutation::created(preview, TransactionId::new()))
}

/// Approve a previewed correction and queue it (§11.5, ADMIN-003).
///
/// Re-authentication is required: ADMIN-001 groups 원장 보정 with the actions that demand
/// it, and §3.3 additionally requires the approver to be somebody other than the person
/// who asked — which is why this endpoint exists at all rather than the preview simply
/// executing itself.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/adjustments",
    tag = "admin",
    request_body = ApproveAdjustmentRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Approved and queued", body = ApprovedAdjustment),
        (status = 403, description = "APPROVAL_SEPARATION_REQUIRED, or the role may not approve"),
        (status = 409, description = "The preview expired or its targets moved"),
    ),
    security(("firebase" = [])),
)]
pub async fn approve_adjustment(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Json(request): Json<ApproveAdjustmentRequest>,
) -> ApiResult<ApiMutation<ApprovedAdjustment>> {
    request.validate()?;
    let admin = require_admin(&user)?;
    admin.require_any(&CHANGE_ROLES)?;

    let approved = state
        .admin
        .approve_adjustment(&state.pool, user.account.user_id, &request)
        .await?;

    Ok(ApiMutation::created(
        approved.clone(),
        TransactionId(approved.execution_job_id),
    ))
}

/// Stop a campaign issuing, immediately (§11.5, ADMIN-005).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/campaigns/{campaign_id}/emergency-stop",
    tag = "admin",
    request_body = ReasonRequest,
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Stopped", body = Campaign),
        (status = 403, description = "This role may not stop a campaign"),
        (status = 404, description = "No such campaign"),
    ),
    security(("firebase" = [])),
)]
pub async fn emergency_stop_campaign(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(campaign_id): Path<Uuid>,
    Json(request): Json<ReasonRequest>,
) -> ApiResult<ApiMutation<Campaign>> {
    request.validate()?;
    let admin = require_admin(&user)?;
    admin.require_any(&CHANGE_ROLES)?;

    let campaign = state
        .campaigns
        .emergency_stop(
            &state.pool,
            user.account.user_id,
            campaign_id,
            &request.reason,
        )
        .await?;

    Ok(ApiMutation::ok(campaign, TransactionId::new()))
}

/// Queue a bulk revocation (§11.5, ADMIN-005).
///
/// Unused coupons only. ADMIN-005 keeps `USED` ones in the statistics and attaches them
/// to the case instead of rewriting history.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/campaigns/{campaign_id}/revoke-job",
    tag = "admin",
    request_body = RevokeJobRequest,
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Revocation queued", body = EnqueuedJob),
        (status = 403, description = "This role may not revoke"),
        (status = 404, description = "No such campaign"),
    ),
    security(("firebase" = [])),
)]
pub async fn revoke_campaign_coupons(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(campaign_id): Path<Uuid>,
    Json(request): Json<RevokeJobRequest>,
) -> ApiResult<ApiMutation<EnqueuedJob>> {
    request.validate()?;
    let admin = require_admin(&user)?;
    admin.require_any(&CHANGE_ROLES)?;

    let job_id = state
        .campaigns
        .request_revocation(
            &state.pool,
            user.account.user_id,
            campaign_id,
            request.case_id,
            &request.reason,
        )
        .await?;

    Ok(ApiMutation::created(
        EnqueuedJob {
            job_id,
            status: JobStatus::PendingOutbox,
            generation: 1,
            deduplicated: false,
        },
        TransactionId(job_id),
    ))
}

/// The queue dashboard (§11.5 작업·시도·체크포인트).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/jobs",
    tag = "admin",
    params(
        ("job_type" = Option<String>, Query, description = "Filter by job type"),
        ("status" = Option<JobStatus>, Query, description = "Filter by status"),
        ("store_id" = Option<Uuid>, Query, description = "Filter by store"),
        ("resource_id" = Option<Uuid>, Query, description = "Filter by campaign or adjustment"),
    ),
    responses((status = 200, description = "Jobs, newest first", body = Vec<JobSummary>)),
    security(("firebase" = [])),
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    _admin: SystemAdmin,
    Query(query): Query<JobQuery>,
) -> ApiResult<ApiOk<Vec<JobSummary>>> {
    Ok(ApiOk(state.jobs.list(&state.pool, &query, 100).await?))
}

/// One job with every attempt it made.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/jobs/{job_id}",
    tag = "admin",
    params(("job_id" = Uuid, Path, description = "Job id")),
    responses(
        (status = 200, description = "Job, checkpoint and attempts", body = JobDetail),
        (status = 404, description = "No such job"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_job(
    State(state): State<AppState>,
    _admin: SystemAdmin,
    Path(job_id): Path<Uuid>,
) -> ApiResult<ApiOk<JobDetail>> {
    Ok(ApiOk(state.jobs.detail(&state.pool, job_id).await?))
}

/// Reprocess a dead-lettered job (§11.5, §14.7).
///
/// The reason is mandatory and the reprocess runs at a **new generation**, so the failed
/// run keeps its own attempt history and this one is separately attributable.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/jobs/{job_id}/retry",
    tag = "admin",
    request_body = ReasonRequest,
    params(
        ("job_id" = Uuid, Path, description = "Job id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Queued at a new generation", body = EnqueuedJob),
        (status = 403, description = "This role may not reprocess jobs"),
        (status = 422, description = "Only a dead-lettered job can be reprocessed"),
    ),
    security(("firebase" = [])),
)]
pub async fn retry_job(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Path(job_id): Path<Uuid>,
    Json(request): Json<ReasonRequest>,
) -> ApiResult<ApiMutation<EnqueuedJob>> {
    request.validate()?;
    admin.require_any(&CHANGE_ROLES)?;

    let enqueued = state
        .jobs
        .reprocess(
            &state.pool,
            job_id,
            admin.user.account.user_id,
            &request.reason,
        )
        .await?;

    Ok(ApiMutation::created(
        enqueued,
        TransactionId(enqueued.job_id),
    ))
}

/// The reason §11.5 requires on every administrative action.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ReasonRequest {
    #[validate(length(min = 1, max = 1000, message = "사유를 입력해 주세요."))]
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct RevokeJobRequest {
    #[validate(length(min = 1, max = 1000, message = "회수 사유를 입력해 주세요."))]
    pub reason: String,
    /// The case this revocation belongs to. It is also the job key's operation version, so
    /// two revocations of one campaign need two cases to authorise them (§14.3).
    pub case_id: Option<Uuid>,
}

/// [`RecentlyAuthenticated`] proves a fresh sign-in but says nothing about role, so the
/// high-risk endpoints re-derive the administrative roles from the account.
fn require_admin(user: &crate::auth::extractors::CurrentUser) -> ApiResult<SystemAdmin> {
    let roles: Vec<AccountRole> = SystemAdmin::ADMIN_ROLES
        .into_iter()
        .filter(|role| user.account.has_role(*role))
        .collect();

    if roles.is_empty() {
        return Err(ApiError::with_message(
            ErrorCode::RoleRequired,
            "관리자 권한이 필요합니다.",
        ));
    }

    Ok(SystemAdmin {
        user: user.clone(),
        roles,
    })
}
