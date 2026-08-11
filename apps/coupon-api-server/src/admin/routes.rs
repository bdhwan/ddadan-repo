//! `/admin/*` (§11.5).

use axum::extract::{Path, State};

use crate::http::query::Query;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::admin::operations::{
    AdminCase, AuditLogEntry, AuditQuery, CaseQuery, CreateCaseRequest, RevokeSessionsRequest,
    SessionRevocation, SuspendUserRequest, UpdateCaseRequest, UserSanction,
};
use crate::admin::{
    AdjustmentPreview, AdjustmentPreviewRequest, AdminTransactionDetail, ApprovedAdjustment,
    ApproveAdjustmentRequest,
};
use crate::http::metrics::OperationalMetrics;
use crate::http::pagination::Page;
use crate::stores::{ReviewStatus, StoreReviewQueueEntry};
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
        // Phase 4 (§11.5): 민원·제재·세션·감사·검수.
        .route("/admin/cases", get(list_cases).post(create_case))
        .route("/admin/cases/{case_id}", get(get_case).patch(patch_case))
        .route("/admin/users/{user_id}/suspend", post(suspend_user))
        .route(
            "/admin/users/{user_id}/revoke-sessions",
            post(revoke_sessions),
        )
        .route("/admin/audit-logs", get(list_audit_logs))
        .route("/admin/store-reviews", get(list_store_reviews))
        .route(
            "/admin/store-reviews/{review_id}/decision",
            post(decide_store_review),
        )
        .route("/admin/metrics", get(get_metrics))
}

/// Roles allowed to see internal case notes and security material.
///
/// §3.3 separates read scope from change scope, and ADMIN-002 separates the reason the
/// subject may see from the investigation behind it. `SUPPORT` answers customers; it does
/// not need the investigator's notes to do that.
const INVESTIGATION_ROLES: [AccountRole; 3] = [
    AccountRole::Operations,
    AccountRole::Security,
    AccountRole::SuperAdmin,
];

/// The case queue (§11.5, ADMIN-004).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/cases",
    tag = "admin",
    params(
        ("status" = Option<String>, Query, description = "OPEN, INVESTIGATING, RESOLVED, …"),
        ("case_type" = Option<String>, Query, description = "QR_ABUSE, PRIVACY_REQUEST, …"),
        ("subject_user_id" = Option<Uuid>, Query, description = "Cases about one member"),
        ("subject_store_id" = Option<Uuid>, Query, description = "Cases about one store"),
        ("limit" = Option<u32>, Query, description = "1–100, default 20"),
        ("cursor" = Option<String>, Query, description = "next_cursor from the previous page"),
    ),
    responses((status = 200, description = "Newest first", body = Page<AdminCase>)),
    security(("firebase" = [])),
)]
pub async fn list_cases(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Query(query): Query<CaseQuery>,
) -> ApiResult<ApiOk<Page<AdminCase>>> {
    let may_see_internal = admin.require_any(&INVESTIGATION_ROLES).is_ok();

    Ok(ApiOk(
        state
            .operations
            .list_cases(&state.pool, &query, may_see_internal)
            .await?,
    ))
}

/// One case (§11.5).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/cases/{case_id}",
    tag = "admin",
    params(("case_id" = Uuid, Path, description = "Case id")),
    responses(
        (status = 200, description = "Case", body = AdminCase),
        (status = 404, description = "No such case"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_case(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Path(case_id): Path<Uuid>,
) -> ApiResult<ApiOk<AdminCase>> {
    let may_see_internal = admin.require_any(&INVESTIGATION_ROLES).is_ok();

    Ok(ApiOk(
        state
            .operations
            .get_case(&state.pool, case_id, may_see_internal)
            .await?,
    ))
}

/// Open a case (ADMIN-002 신고·자동 위험 신호·외부 요청을 사건 티켓으로 만든다).
///
/// Every administrative role may open one, including `SUPPORT`: a complaint arriving at
/// the support desk is exactly the thing that should become a ticket, and making that
/// require a higher role would push it into an email thread instead.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/cases",
    tag = "admin",
    request_body = CreateCaseRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Opened", body = AdminCase),
        (status = 400, description = "Unknown case type"),
    ),
    security(("firebase" = [])),
)]
pub async fn create_case(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Json(request): Json<CreateCaseRequest>,
) -> ApiResult<ApiMutation<AdminCase>> {
    request.validate()?;

    let case = state
        .operations
        .create_case(&state.pool, admin.user.account.user_id, &request)
        .await?;

    Ok(ApiMutation::created(case.clone(), TransactionId(case.id)))
}

/// Work a case (ADMIN-004).
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/admin/cases/{case_id}",
    tag = "admin",
    request_body = UpdateCaseRequest,
    params(
        ("case_id" = Uuid, Path, description = "Case id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Updated", body = AdminCase),
        (status = 403, description = "This role may not change a case"),
        (status = 404, description = "No such case"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_case(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Path(case_id): Path<Uuid>,
    Json(request): Json<UpdateCaseRequest>,
) -> ApiResult<ApiMutation<AdminCase>> {
    request.validate()?;
    admin.require_any(&CHANGE_ROLES)?;

    let case = state
        .operations
        .update_case(&state.pool, admin.user.account.user_id, case_id, &request)
        .await?;

    Ok(ApiMutation::ok(case, TransactionId::new()))
}

/// 임시/영구 제재 (§11.5, ADMIN-002).
///
/// Re-authentication is required, and a permanent sanction additionally names a second
/// administrator: §3.3's 이중 확인. The database refuses an approver who is the requester,
/// so the rule holds even if this handler is bypassed.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/users/{user_id}/suspend",
    tag = "admin",
    request_body = SuspendUserRequest,
    params(
        ("user_id" = Uuid, Path, description = "Subject member id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Sanctioned", body = UserSanction),
        (status = 403, description = "APPROVAL_SEPARATION_REQUIRED, or the role may not sanction"),
        (status = 404, description = "No such member or case"),
        (status = 409, description = "SANCTION_ALREADY_ACTIVE"),
    ),
    security(("firebase" = [])),
)]
pub async fn suspend_user(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(user_id): Path<Uuid>,
    Json(request): Json<SuspendUserRequest>,
) -> ApiResult<ApiMutation<UserSanction>> {
    request.validate()?;
    let admin = require_admin(&user)?;
    admin.require_any(&[AccountRole::Security, AccountRole::SuperAdmin])?;

    let sanction = state
        .operations
        .suspend_user(&state.pool, user.account.user_id, user_id, &request)
        .await?;

    Ok(ApiMutation::created(
        sanction.clone(),
        TransactionId(sanction.id),
    ))
}

/// Firebase 세션 폐기 (§11.5).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/users/{user_id}/revoke-sessions",
    tag = "admin",
    request_body = RevokeSessionsRequest,
    params(
        ("user_id" = Uuid, Path, description = "Subject member id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Revoked", body = SessionRevocation),
        (status = 403, description = "This role may not revoke sessions"),
        (status = 404, description = "No such member"),
    ),
    security(("firebase" = [])),
)]
pub async fn revoke_sessions(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(user_id): Path<Uuid>,
    Json(request): Json<RevokeSessionsRequest>,
) -> ApiResult<ApiMutation<SessionRevocation>> {
    request.validate()?;
    let admin = require_admin(&user)?;
    admin.require_any(&[AccountRole::Security, AccountRole::SuperAdmin])?;

    let revocation = state
        .operations
        .revoke_sessions(&state.pool, user.account.user_id, user_id, &request)
        .await?;

    Ok(ApiMutation::created(
        revocation.clone(),
        TransactionId(revocation.id),
    ))
}

/// 감사 검색 (§11.5, §12.5).
///
/// Each row carries `chain_intact`, recomputed on read. §12.5 asks for 변조 탐지 and a flag
/// nobody looks at is not detection — so it travels with the data an investigator is
/// already reading.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/audit-logs",
    tag = "admin",
    params(
        ("actor_user_id" = Option<Uuid>, Query, description = "Who acted"),
        ("action" = Option<String>, Query, description = "Exact action verb"),
        ("resource_type" = Option<String>, Query, description = "e.g. stamp_transaction"),
        ("resource_id" = Option<Uuid>, Query, description = "One resource's history"),
        ("store_id" = Option<Uuid>, Query, description = "Scoped to a store"),
        ("case_id" = Option<Uuid>, Query, description = "Everything attached to a case"),
        ("from" = Option<String>, Query, description = "RFC 3339, inclusive"),
        ("to" = Option<String>, Query, description = "RFC 3339, exclusive"),
        ("limit" = Option<u32>, Query, description = "1–100, default 20"),
        ("cursor" = Option<String>, Query, description = "next_cursor from the previous page"),
    ),
    responses(
        (status = 200, description = "Newest first", body = Page<AuditLogEntry>),
        (status = 403, description = "This role may not read the audit trail"),
    ),
    security(("firebase" = [])),
)]
pub async fn list_audit_logs(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Query(query): Query<AuditQuery>,
) -> ApiResult<ApiOk<Page<AuditLogEntry>>> {
    // SEC-005: 감사 로그는 오남용 조사 도구다. Reading it is itself a privileged act.
    admin.require_any(&INVESTIGATION_ROLES)?;

    Ok(ApiOk(
        state.operations.search_audit_logs(&state.pool, &query).await?,
    ))
}

/// 검수 큐 (§11.5, STORE-002).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/store-reviews",
    tag = "admin",
    params(
        ("status" = Option<String>, Query, description = "PENDING, APPROVED, REJECTED, …"),
        ("limit" = Option<u32>, Query, description = "1–100, default 20"),
        ("cursor" = Option<String>, Query, description = "next_cursor from the previous page"),
    ),
    responses((status = 200, description = "Oldest first", body = Page<StoreReviewQueueEntry>)),
    security(("firebase" = [])),
)]
pub async fn list_store_reviews(
    State(state): State<AppState>,
    _admin: SystemAdmin,
    Query(query): Query<StoreReviewQuery>,
) -> ApiResult<ApiOk<Page<StoreReviewQueueEntry>>> {
    Ok(ApiOk(
        state
            .stores
            .review_queue(
                &state.pool,
                query.status.as_deref(),
                &crate::http::pagination::PageQuery {
                    limit: query.limit,
                    cursor: query.cursor.clone(),
                },
            )
            .await?,
    ))
}

/// 승인·보완·거절 (§11.5, STORE-002).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/store-reviews/{review_id}/decision",
    tag = "admin",
    request_body = ReviewDecisionRequest,
    params(
        ("review_id" = Uuid, Path, description = "Review id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Decided", body = StoreReviewQueueEntry),
        (status = 403, description = "This role may not decide reviews"),
        (status = 404, description = "No such review"),
        (status = 422, description = "The review is not pending"),
    ),
    security(("firebase" = [])),
)]
pub async fn decide_store_review(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Path(review_id): Path<Uuid>,
    Json(request): Json<ReviewDecisionRequest>,
) -> ApiResult<ApiMutation<StoreReviewQueueEntry>> {
    request.validate()?;
    admin.require_any(&CHANGE_ROLES)?;

    let decided = state
        .stores
        .decide_review(
            &state.pool,
            admin.user.account.user_id,
            review_id,
            request.decision,
            request.public_reason.as_deref(),
            &request.reason,
        )
        .await?;

    Ok(ApiMutation::ok(decided, TransactionId::new()))
}

/// §18.4's alert inputs, as numbers (§18.3, §18.4).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/metrics",
    tag = "admin",
    responses((status = 200, description = "Operational metrics", body = OperationalMetrics)),
    security(("firebase" = [])),
)]
pub async fn get_metrics(
    State(state): State<AppState>,
    _admin: SystemAdmin,
) -> ApiResult<ApiOk<OperationalMetrics>> {
    Ok(ApiOk(crate::http::metrics::collect(&state).await?))
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct StoreReviewQuery {
    pub status: Option<String>,
    /// Spelled out rather than flattened: `#[serde(flatten)]` forces `deserialize_any`, and
    /// a query string hands every value over as a string, so `?limit=20` would not parse.
    #[serde(default, deserialize_with = "crate::http::pagination::page_size")]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ReviewDecisionRequest {
    pub decision: ReviewStatus,
    /// Shown to the owner (STORE-002 보완 요청 사유).
    #[validate(length(max = 2000))]
    pub public_reason: Option<String>,
    /// Internal reviewer note. §11.5 requires a reason on every administrative change.
    #[validate(length(min = 1, max = 2000, message = "검수 사유를 입력해 주세요."))]
    pub reason: String,
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
pub fn require_admin(user: &crate::auth::extractors::CurrentUser) -> ApiResult<SystemAdmin> {
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
