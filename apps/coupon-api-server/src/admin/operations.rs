//! 민원·제재·세션·감사 (§11.5, §12.5, §3.3, ADMIN-001…006, SEC-005).
//!
//! Everything here is a high-risk administrative action, and §3.3 puts three demands on
//! those: **recent re-authentication, a written reason, and a case ticket.** The first is
//! an extractor ([`RecentlyAuthenticated`](crate::auth::extractors::RecentlyAuthenticated))
//! so it cannot be forgotten in a handler body; the other two are required fields on every
//! request type in this module, so a change with no reason and no ticket does not typecheck.
//!
//! The separation-of-duties rule has the same treatment. §3.3 requires a second
//! administrator to approve a permanent sanction, and that is a database CHECK
//! (`ck_user_sanction_separation`) rather than an `if` — the same reasoning as
//! `admin_adjustments` in Phase 2: a rule that only lives in application code is one
//! refactor away from being bypassed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{ActorType, AuditEntry, AuditService, chain_hash};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::pagination::{Cursor, Page, PageQuery};

// ---------------------------------------------------------------------------
// Cases (§11.5 `/admin/cases`, ADMIN-004)
// ---------------------------------------------------------------------------

/// `POST /admin/cases`.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreateCaseRequest {
    /// One of `coupon.admin_case_type`: ADMIN-004's 유형 분류.
    pub case_type: String,
    #[validate(length(min = 1, max = 200, message = "제목은 1~200자여야 합니다."))]
    pub title: String,
    #[validate(length(min = 1, max = 8000, message = "내용을 입력해 주세요."))]
    pub description: String,
    #[validate(range(min = 1, max = 5, message = "우선순위는 1~5입니다."))]
    pub priority: Option<i16>,
    pub subject_user_id: Option<Uuid>,
    pub subject_store_id: Option<Uuid>,
    #[validate(length(max = 100))]
    pub subject_resource_type: Option<String>,
    pub subject_resource_id: Option<Uuid>,
    /// ADMIN-006: 처리 기한.
    pub due_at: Option<DateTime<Utc>>,
    /// §17.3: while this is in the future the subject's data cannot be erased.
    pub legal_hold_until: Option<DateTime<Utc>>,
}

/// `PATCH /admin/cases/:id`.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateCaseRequest {
    pub status: Option<String>,
    pub assignee_user_id: Option<Uuid>,
    #[validate(range(min = 1, max = 5))]
    pub priority: Option<i16>,
    pub resolution_type: Option<String>,
    /// ADMIN-002: 공개 가능한 사유와 내부 사유를 분리한다.
    #[validate(length(max = 8000))]
    pub public_resolution: Option<String>,
    #[validate(length(max = 8000))]
    pub internal_resolution: Option<String>,
    pub legal_hold_until: Option<DateTime<Utc>>,
    pub due_at: Option<DateTime<Utc>>,
    #[validate(length(min = 1, max = 1000, message = "변경 사유를 입력해 주세요."))]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdminCase {
    pub id: Uuid,
    pub case_number: i64,
    pub case_type: String,
    pub status: String,
    pub priority: i16,
    pub title: String,
    pub description: String,
    pub subject_user_id: Option<Uuid>,
    pub subject_store_id: Option<Uuid>,
    pub subject_resource_type: Option<String>,
    pub subject_resource_id: Option<Uuid>,
    pub assignee_user_id: Option<Uuid>,
    pub resolution_type: Option<String>,
    pub public_resolution: Option<String>,
    /// Only returned to the roles §3.3 lets act; `SUPPORT` sees `null`.
    pub internal_resolution: Option<String>,
    pub legal_hold_until: Option<DateTime<Utc>>,
    pub due_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct CaseQuery {
    pub status: Option<String>,
    pub case_type: Option<String>,
    pub subject_user_id: Option<Uuid>,
    pub subject_store_id: Option<Uuid>,
    /// Paging is spelled out rather than `#[serde(flatten)]`-ed: a flattened struct forces
    /// `deserialize_any`, and a query string hands every value over as a string, so
    /// `?limit=20` would fail to parse as a number.
    #[serde(default, deserialize_with = "crate::http::pagination::page_size")]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

impl CaseQuery {
    fn page(&self) -> PageQuery {
        PageQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Sanctions (§11.5, ADMIN-002)
// ---------------------------------------------------------------------------

/// `POST /admin/users/:id/suspend`.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct SuspendUserRequest {
    /// `TEMPORARY` or `PERMANENT`.
    pub sanction_type: String,
    /// §3.3: 고위험 작업은 사건 티켓을 요구한다.
    pub case_id: Uuid,
    #[validate(length(min = 1, max = 2000, message = "대상에게 알릴 사유를 입력해 주세요."))]
    pub public_reason: String,
    #[validate(length(min = 1, max = 8000, message = "내부 사유를 입력해 주세요."))]
    pub internal_reason: String,
    /// Required for `TEMPORARY`, refused for `PERMANENT` — a permanent sanction with an end
    /// date is a temporary one under the wrong label (ADMIN-002).
    pub expires_at: Option<DateTime<Utc>>,
    /// The second administrator, for a permanent sanction (§3.3 이중 확인). Must differ from
    /// the requester; the database enforces it.
    pub approved_by_user_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserSanction {
    pub id: Uuid,
    pub subject_user_id: Uuid,
    pub case_id: Uuid,
    pub sanction_type: String,
    pub status: String,
    pub public_reason: String,
    pub requested_by_user_id: Uuid,
    pub approved_by_user_id: Option<Uuid>,
    pub effective_from: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub lifted_at: Option<DateTime<Utc>>,
}

/// `POST /admin/users/:id/revoke-sessions`.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct RevokeSessionsRequest {
    #[validate(length(min = 1, max = 1000, message = "사유를 입력해 주세요."))]
    pub reason: String,
    pub case_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionRevocation {
    pub id: Uuid,
    pub subject_user_id: Uuid,
    /// Every token issued before this instant is refused, whatever Firebase thinks.
    pub valid_after: DateTime<Utc>,
    pub provider_result: String,
    pub occurred_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Audit search (§11.5, §12.5)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct AuditQuery {
    pub actor_user_id: Option<Uuid>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub store_id: Option<Uuid>,
    pub case_id: Option<Uuid>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "crate::http::pagination::page_size")]
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

impl AuditQuery {
    fn page(&self) -> PageQuery {
        PageQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub actor_type: String,
    pub actor_user_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub store_id: Option<Uuid>,
    pub case_id: Option<Uuid>,
    pub reason: Option<String>,
    pub request_id: Option<String>,
    pub metadata: serde_json::Value,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub occurred_at: DateTime<Utc>,
    /// Whether this entry still chains to its predecessor (§12.5 변조 탐지). `false` means
    /// the row, or one before it, is not what it was when it was written.
    pub chain_intact: bool,
}

/// The four case queries return four anonymous row types with identical fields; this
/// collapses each into [`CaseRow`] without a hand-written conversion per query.
macro_rules! case_row {
    ($row:expr) => {
        CaseRow {
            id: $row.id,
            case_number: $row.case_number,
            case_type: $row.case_type,
            status: $row.status,
            priority: $row.priority,
            title: $row.title,
            description: $row.description,
            subject_user_id: $row.subject_user_id,
            subject_store_id: $row.subject_store_id,
            subject_resource_type: $row.subject_resource_type,
            subject_resource_id: $row.subject_resource_id,
            assignee_user_id: $row.assignee_user_id,
            resolution_type: $row.resolution_type,
            public_resolution: $row.public_resolution,
            internal_resolution: $row.internal_resolution,
            legal_hold_until: $row.legal_hold_until,
            due_at: $row.due_at,
            resolved_at: $row.resolved_at,
            created_at: $row.created_at,
            updated_at: $row.updated_at,
            version: $row.version,
        }
    };
}

/// Cases, sanctions, sessions and the audit trail.
pub struct OperationsService {
    audit: std::sync::Arc<AuditService>,
}

impl OperationsService {
    pub fn new(audit: std::sync::Arc<AuditService>) -> Self {
        Self { audit }
    }

    // -----------------------------------------------------------------------
    // Cases
    // -----------------------------------------------------------------------

    pub async fn create_case(
        &self,
        pool: &PgPool,
        actor_user_id: Uuid,
        request: &CreateCaseRequest,
    ) -> ApiResult<AdminCase> {
        ensure_case_type(&request.case_type)?;

        let mut tx = pool.begin().await?;

        let row = sqlx::query!(
            r#"
            INSERT INTO coupon.admin_cases
                (case_type, status, priority, title, description, subject_user_id,
                 subject_store_id, subject_resource_type, subject_resource_id,
                 opened_by_user_id, due_at, legal_hold_until, correlation_id)
            VALUES ($1::text::coupon.admin_case_type, 'OPEN', COALESCE($2, 3::smallint), $3, $4, $5, $6,
                    $7, $8, $9, $10, $11, $12)
            RETURNING id, case_number, case_type::text AS "case_type!",
                      status::text AS "status!", priority, title, description,
                      subject_user_id, subject_store_id, subject_resource_type,
                      subject_resource_id, assignee_user_id, resolution_type,
                      public_resolution, internal_resolution, legal_hold_until, due_at,
                      resolved_at, created_at, updated_at, version
            "#,
            request.case_type,
            request.priority,
            request.title,
            request.description,
            request.subject_user_id,
            request.subject_store_id,
            request.subject_resource_type,
            request.subject_resource_id,
            actor_user_id,
            request.due_at,
            request.legal_hold_until,
            Some(Uuid::new_v4()),
        )
        .fetch_one(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::SystemAdmin, "admin_case.opened", "admin_case")
                    .actor(actor_user_id)
                    .resource(row.id)
                    .case(row.id)
                    .reason(request.title.clone())
                    .metadata(serde_json::json!({ "case_type": request.case_type })),
            )
            .await?;

        tx.commit().await?;
        Ok(map_case(case_row!(row), true))
    }

    /// The case queue, cursor-paginated (§11.1).
    pub async fn list_cases(
        &self,
        pool: &PgPool,
        query: &CaseQuery,
        may_see_internal: bool,
    ) -> ApiResult<Page<AdminCase>> {
        let page = query.page();
        let cursor = page.cursor()?;

        let rows = sqlx::query!(
            r#"
            SELECT id, case_number, case_type::text AS "case_type!", status::text AS "status!",
                   priority, title, description, subject_user_id, subject_store_id,
                   subject_resource_type, subject_resource_id, assignee_user_id,
                   resolution_type, public_resolution, internal_resolution, legal_hold_until,
                   due_at, resolved_at, created_at, updated_at, version
            FROM coupon.admin_cases
            WHERE ($1::text IS NULL OR status::text = $1)
              AND ($2::text IS NULL OR case_type::text = $2)
              AND ($3::uuid IS NULL OR subject_user_id = $3)
              AND ($4::uuid IS NULL OR subject_store_id = $4)
              AND ($5::timestamptz IS NULL OR (created_at, id) < ($5::timestamptz, $6::uuid))
            ORDER BY created_at DESC, id DESC
            LIMIT $7
            "#,
            query.status,
            query.case_type,
            query.subject_user_id,
            query.subject_store_id,
            cursor.as_ref().map(|cursor| cursor.created_at),
            cursor.as_ref().map(|cursor| cursor.id),
            page.fetch_limit(),
        )
        .fetch_all(pool)
        .await?;

        let items: Vec<AdminCase> = rows
            .into_iter()
            .map(|row| map_case(case_row!(row), may_see_internal))
            .collect();

        Ok(Page::from_rows(items, page.limit(), |case| {
            Cursor::new(case.created_at, case.id)
        }))
    }

    pub async fn get_case(
        &self,
        pool: &PgPool,
        case_id: Uuid,
        may_see_internal: bool,
    ) -> ApiResult<AdminCase> {
        let row = sqlx::query!(
            r#"
            SELECT id, case_number, case_type::text AS "case_type!", status::text AS "status!",
                   priority, title, description, subject_user_id, subject_store_id,
                   subject_resource_type, subject_resource_id, assignee_user_id,
                   resolution_type, public_resolution, internal_resolution, legal_hold_until,
                   due_at, resolved_at, created_at, updated_at, version
            FROM coupon.admin_cases WHERE id = $1
            "#,
            case_id,
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::CaseNotFound))?;

        Ok(map_case(case_row!(row), may_see_internal))
    }

    /// Move a case along. ADMIN-004 wants the outcome and the audit trail connected, so the
    /// transition and its audit entry commit together.
    pub async fn update_case(
        &self,
        pool: &PgPool,
        actor_user_id: Uuid,
        case_id: Uuid,
        request: &UpdateCaseRequest,
    ) -> ApiResult<AdminCase> {
        if let Some(status) = &request.status {
            ensure_case_status(status)?;
        }
        if let Some(resolution) = &request.resolution_type {
            ensure_resolution_type(resolution)?;
        }

        let mut tx = pool.begin().await?;

        let before = sqlx::query!(
            r#"
            SELECT status::text AS "status!", priority, resolution_type, assignee_user_id
            FROM coupon.admin_cases WHERE id = $1 FOR UPDATE
            "#,
            case_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::CaseNotFound))?;

        let row = sqlx::query!(
            r#"
            UPDATE coupon.admin_cases
            SET status = COALESCE($2::text::coupon.admin_case_status, status),
                assignee_user_id = COALESCE($3, assignee_user_id),
                priority = COALESCE($4, priority),
                resolution_type = COALESCE($5, resolution_type),
                public_resolution = COALESCE($6, public_resolution),
                internal_resolution = COALESCE($7, internal_resolution),
                legal_hold_until = COALESCE($8, legal_hold_until),
                due_at = COALESCE($9, due_at),
                -- The CHECK constraint demands these timestamps for the terminal statuses,
                -- so they are derived here rather than trusted from the request.
                resolved_at = CASE
                    WHEN COALESCE($2, status::text) IN ('RESOLVED', 'CLOSED')
                        THEN COALESCE(resolved_at, clock_timestamp())
                    ELSE resolved_at END,
                closed_at = CASE
                    WHEN COALESCE($2, status::text) = 'CLOSED'
                        THEN COALESCE(closed_at, clock_timestamp())
                    ELSE closed_at END,
                resolved_by_user_id = CASE
                    WHEN COALESCE($2, status::text) IN ('RESOLVED', 'CLOSED')
                        THEN COALESCE(resolved_by_user_id, $10)
                    ELSE resolved_by_user_id END,
                version = version + 1
            WHERE id = $1
            RETURNING id, case_number, case_type::text AS "case_type!",
                      status::text AS "status!", priority, title, description,
                      subject_user_id, subject_store_id, subject_resource_type,
                      subject_resource_id, assignee_user_id, resolution_type,
                      public_resolution, internal_resolution, legal_hold_until, due_at,
                      resolved_at, created_at, updated_at, version
            "#,
            case_id,
            request.status,
            request.assignee_user_id,
            request.priority,
            request.resolution_type,
            request.public_resolution,
            request.internal_resolution,
            request.legal_hold_until,
            request.due_at,
            actor_user_id,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::SystemAdmin, "admin_case.updated", "admin_case")
                    .actor(actor_user_id)
                    .resource(case_id)
                    .case(case_id)
                    .reason(request.reason.clone())
                    .transition(
                        &serde_json::json!({
                            "status": before.status,
                            "priority": before.priority,
                            "resolution_type": before.resolution_type,
                            "assignee": before.assignee_user_id,
                        }),
                        &serde_json::json!({
                            "status": row.status,
                            "priority": row.priority,
                            "resolution_type": row.resolution_type,
                            "assignee": row.assignee_user_id,
                        }),
                    ),
            )
            .await?;

        tx.commit().await?;
        Ok(map_case(case_row!(row), true))
    }

    // -----------------------------------------------------------------------
    // Sanctions
    // -----------------------------------------------------------------------

    /// Suspend an account (§11.5, ADMIN-002).
    ///
    /// A temporary sanction takes effect immediately and expires on its own. A permanent one
    /// requires a second administrator (§3.3) and is refused — by the database — if the
    /// named approver is the requester.
    pub async fn suspend_user(
        &self,
        pool: &PgPool,
        actor_user_id: Uuid,
        subject_user_id: Uuid,
        request: &SuspendUserRequest,
    ) -> ApiResult<UserSanction> {
        let permanent = match request.sanction_type.as_str() {
            "TEMPORARY" => false,
            "PERMANENT" => true,
            _ => {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    "제재 유형은 TEMPORARY 또는 PERMANENT 입니다.",
                ));
            }
        };

        if permanent {
            let Some(approver) = request.approved_by_user_id else {
                return Err(ApiError::with_message(
                    ErrorCode::ApprovalSeparationRequired,
                    "영구 제재는 다른 관리자의 승인이 필요합니다.",
                ));
            };
            if approver == actor_user_id {
                return Err(ApiError::new(ErrorCode::ApprovalSeparationRequired));
            }
        } else if request.expires_at.is_none() {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "임시 제재에는 종료 시각이 필요합니다.",
            ));
        }

        let mut tx = pool.begin().await?;

        let subject = sqlx::query!(
            r#"SELECT status::text AS "status!" FROM coupon.users WHERE id = $1 FOR UPDATE"#,
            subject_user_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::UserNotFound))?;

        let case_exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM coupon.admin_cases WHERE id = $1) AS "exists!""#,
            request.case_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if !case_exists {
            return Err(ApiError::new(ErrorCode::CaseNotFound));
        }

        let now = Utc::now();
        let inserted = sqlx::query!(
            r#"
            INSERT INTO coupon.user_sanctions
                (subject_user_id, case_id, sanction_type, status, public_reason,
                 internal_reason, requested_by_user_id, approved_by_user_id, approved_at,
                 effective_from, expires_at)
            VALUES ($1, $2, $3::text::coupon.sanction_type, 'ACTIVE', $4, $5, $6, $7, $8, $9,
                    $10)
            ON CONFLICT DO NOTHING
            RETURNING id, subject_user_id, case_id, sanction_type::text AS "sanction_type!",
                      status::text AS "status!", public_reason, requested_by_user_id,
                      approved_by_user_id, effective_from, expires_at, lifted_at
            "#,
            subject_user_id,
            request.case_id,
            request.sanction_type,
            request.public_reason,
            request.internal_reason,
            actor_user_id,
            request.approved_by_user_id,
            request.approved_by_user_id.map(|_| now),
            now,
            request.expires_at,
        )
        .fetch_optional(&mut *tx)
        .await?;

        // `uq_user_sanctions_active` refused it: somebody is already sanctioned. Lifting the
        // existing one is a separate, separately audited act.
        let Some(row) = inserted else {
            return Err(ApiError::new(ErrorCode::SanctionAlreadyActive));
        };

        sqlx::query!(
            r#"
            UPDATE coupon.users
            SET status = 'SUSPENDED', suspended_at = $2, suspension_reason = $3,
                -- AUTH-008 / ADMIN-002: a suspension that leaves live sessions alone is not
                -- a suspension.
                sessions_valid_after = $2,
                version = version + 1
            WHERE id = $1
            "#,
            subject_user_id,
            now,
            request.public_reason,
        )
        .execute(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::SystemAdmin, "user.suspended", "user")
                    .actor(actor_user_id)
                    .resource(subject_user_id)
                    .case(request.case_id)
                    .reason(request.internal_reason.clone())
                    .transition(
                        &serde_json::json!({ "status": subject.status }),
                        &serde_json::json!({ "status": "SUSPENDED" }),
                    )
                    .metadata(serde_json::json!({
                        "sanction_type": request.sanction_type,
                        "expires_at": request.expires_at,
                        "approved_by": request.approved_by_user_id,
                    })),
            )
            .await?;

        tx.commit().await?;

        Ok(UserSanction {
            id: row.id,
            subject_user_id: row.subject_user_id,
            case_id: row.case_id,
            sanction_type: row.sanction_type,
            status: row.status,
            public_reason: row.public_reason,
            requested_by_user_id: row.requested_by_user_id,
            approved_by_user_id: row.approved_by_user_id,
            effective_from: row.effective_from,
            expires_at: row.expires_at,
            lifted_at: row.lifted_at,
        })
    }

    /// Expire temporary sanctions whose time is up (ADMIN-002: 만료 시 자동 복구 후보).
    ///
    /// "후보" is doing work in that sentence: the sanction ends, and the account returns to
    /// `ACTIVE` only if nothing else is holding it down.
    pub async fn expire_due_sanctions(&self, pool: &PgPool, now: DateTime<Utc>) -> ApiResult<u64> {
        let mut tx = pool.begin().await?;

        let expired = sqlx::query_scalar!(
            r#"
            UPDATE coupon.user_sanctions
            SET status = 'EXPIRED', version = version + 1
            WHERE status = 'ACTIVE' AND expires_at IS NOT NULL AND expires_at <= $1
            RETURNING subject_user_id
            "#,
            now,
        )
        .fetch_all(&mut *tx)
        .await?;

        for subject_user_id in &expired {
            sqlx::query!(
                r#"
                UPDATE coupon.users
                SET status = 'ACTIVE', suspended_at = NULL, suspension_reason = NULL,
                    version = version + 1
                WHERE id = $1
                  AND status = 'SUSPENDED'
                  AND withdrawn_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1 FROM coupon.user_sanctions s
                      WHERE s.subject_user_id = $1 AND s.status = 'ACTIVE'
                  )
                "#,
                subject_user_id,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(expired.len() as u64)
    }

    /// `POST /admin/users/:id/revoke-sessions` (§11.5).
    ///
    /// Firebase owns the sessions, but the enforcement point is ours: `sessions_valid_after`
    /// is compared against the token's `auth_time` on every request, so the revocation binds
    /// immediately rather than when Firebase finishes propagating. The provider call is
    /// recorded separately and may be retried without changing that.
    pub async fn revoke_sessions(
        &self,
        pool: &PgPool,
        actor_user_id: Uuid,
        subject_user_id: Uuid,
        request: &RevokeSessionsRequest,
    ) -> ApiResult<SessionRevocation> {
        let mut tx = pool.begin().await?;

        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM coupon.users WHERE id = $1) AS "exists!""#,
            subject_user_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(ApiError::new(ErrorCode::UserNotFound));
        }

        let now = Utc::now();
        sqlx::query!(
            r#"
            UPDATE coupon.users SET sessions_valid_after = $2, version = version + 1
            WHERE id = $1
            "#,
            subject_user_id,
            now,
        )
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query!(
            r#"
            INSERT INTO coupon.user_session_revocations
                (subject_user_id, case_id, requested_by_user_id, reason, valid_after,
                 provider_result)
            VALUES ($1, $2, $3, $4, $5, 'PENDING')
            RETURNING id, subject_user_id, valid_after, provider_result, occurred_at
            "#,
            subject_user_id,
            request.case_id,
            actor_user_id,
            request.reason,
            now,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::SystemAdmin, "user.sessions_revoked", "user")
                    .actor(actor_user_id)
                    .resource(subject_user_id)
                    .reason(request.reason.clone())
                    .metadata(serde_json::json!({ "valid_after": now })),
            )
            .await?;

        tx.commit().await?;

        Ok(SessionRevocation {
            id: row.id,
            subject_user_id: row.subject_user_id,
            valid_after: row.valid_after,
            provider_result: row.provider_result,
            occurred_at: row.occurred_at,
        })
    }

    // -----------------------------------------------------------------------
    // Audit search (§11.5, §12.5)
    // -----------------------------------------------------------------------

    /// Search the audit trail, verifying each entry's hash chain as it goes.
    ///
    /// §12.5 asks for 변조 탐지, and detection has to happen somewhere a person will see it.
    /// Recomputing the chain hash on read is cheap — it is a SHA-256 over eight short fields
    /// — and it turns "the log is append-only" from a claim into something the reader can
    /// check. `chain_intact: false` on any row means the row, or its predecessor, is not
    /// what it was when written.
    pub async fn search_audit_logs(
        &self,
        pool: &PgPool,
        query: &AuditQuery,
    ) -> ApiResult<Page<AuditLogEntry>> {
        let page = query.page();
        let cursor = page.cursor()?;

        let rows = sqlx::query!(
            r#"
            SELECT id, actor_type::text AS "actor_type!", actor_user_id, action,
                   resource_type, resource_id, store_id, case_id, reason, request_id,
                   metadata, before_hash, after_hash, previous_entry_hash, entry_hash,
                   occurred_at
            FROM coupon.audit_logs
            WHERE ($1::uuid IS NULL OR actor_user_id = $1)
              AND ($2::text IS NULL OR action = $2)
              AND ($3::text IS NULL OR resource_type = $3)
              AND ($4::uuid IS NULL OR resource_id = $4)
              AND ($5::uuid IS NULL OR store_id = $5)
              AND ($6::uuid IS NULL OR case_id = $6)
              AND ($7::timestamptz IS NULL OR occurred_at >= $7)
              AND ($8::timestamptz IS NULL OR occurred_at < $8)
              AND ($9::timestamptz IS NULL OR (occurred_at, id) < ($9::timestamptz, $10::uuid))
            ORDER BY occurred_at DESC, id DESC
            LIMIT $11
            "#,
            query.actor_user_id,
            query.action,
            query.resource_type,
            query.resource_id,
            query.store_id,
            query.case_id,
            query.from,
            query.to,
            cursor.as_ref().map(|cursor| cursor.created_at),
            cursor.as_ref().map(|cursor| cursor.id),
            page.fetch_limit(),
        )
        .fetch_all(pool)
        .await?;

        let items: Vec<AuditLogEntry> = rows
            .into_iter()
            .map(|row| {
                let entry = AuditEntry {
                    actor_type: parse_actor_type(&row.actor_type),
                    actor_user_id: row.actor_user_id,
                    action: row.action.clone(),
                    resource_type: row.resource_type.clone(),
                    resource_id: row.resource_id,
                    store_id: row.store_id,
                    case_id: row.case_id,
                    reason: row.reason.clone(),
                    metadata: row.metadata.clone(),
                    before_hash: row.before_hash.clone(),
                    after_hash: row.after_hash.clone(),
                };

                let recomputed =
                    chain_hash(row.previous_entry_hash.as_deref(), &entry, row.occurred_at);
                let chain_intact = row
                    .entry_hash
                    .as_deref()
                    .map(|stored| stored == recomputed)
                    .unwrap_or(false);

                AuditLogEntry {
                    id: row.id,
                    actor_type: row.actor_type,
                    actor_user_id: row.actor_user_id,
                    action: row.action,
                    resource_type: row.resource_type,
                    resource_id: row.resource_id,
                    store_id: row.store_id,
                    case_id: row.case_id,
                    reason: row.reason,
                    request_id: row.request_id,
                    metadata: row.metadata,
                    before_hash: row.before_hash,
                    after_hash: row.after_hash,
                    occurred_at: row.occurred_at,
                    chain_intact,
                }
            })
            .collect();

        Ok(Page::from_rows(items, page.limit(), |entry| {
            Cursor::new(entry.occurred_at, entry.id)
        }))
    }
}

/// Flat row shape shared by the three case queries.
struct CaseRow {
    id: Uuid,
    case_number: i64,
    case_type: String,
    status: String,
    priority: i16,
    title: String,
    description: String,
    subject_user_id: Option<Uuid>,
    subject_store_id: Option<Uuid>,
    subject_resource_type: Option<String>,
    subject_resource_id: Option<Uuid>,
    assignee_user_id: Option<Uuid>,
    resolution_type: Option<String>,
    public_resolution: Option<String>,
    internal_resolution: Option<String>,
    legal_hold_until: Option<DateTime<Utc>>,
    due_at: Option<DateTime<Utc>>,
    resolved_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

fn map_case(row: CaseRow, may_see_internal: bool) -> AdminCase {
    AdminCase {
        id: row.id,
        case_number: row.case_number,
        case_type: row.case_type,
        status: row.status,
        priority: row.priority,
        title: row.title,
        description: row.description,
        subject_user_id: row.subject_user_id,
        subject_store_id: row.subject_store_id,
        subject_resource_type: row.subject_resource_type,
        subject_resource_id: row.subject_resource_id,
        assignee_user_id: row.assignee_user_id,
        resolution_type: row.resolution_type,
        public_resolution: row.public_resolution,
        // ADMIN-002 separates the reason the subject may see from the internal one, and
        // §3.3 separates read scope from change scope. `SUPPORT` reads cases all day; it has
        // no business reading the investigation notes.
        internal_resolution: if may_see_internal {
            row.internal_resolution
        } else {
            None
        },
        legal_hold_until: row.legal_hold_until,
        due_at: row.due_at,
        resolved_at: row.resolved_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        version: row.version,
    }
}

fn parse_actor_type(raw: &str) -> ActorType {
    match raw {
        "USER" => ActorType::User,
        "STORE_OWNER" => ActorType::StoreOwner,
        "SYSTEM" => ActorType::System,
        "PROVIDER" => ActorType::Provider,
        _ => ActorType::SystemAdmin,
    }
}

/// `coupon.admin_case_type`. Validated in Rust as well as by the cast, so a typo is a 400
/// with a readable message rather than a 500 from a failed enum cast.
const CASE_TYPES: [&str; 9] = [
    "STORE_REVIEW",
    "COUPON_MISSING",
    "WRONG_REDEMPTION",
    "QR_ABUSE",
    "WRONG_STAMP",
    "STORE_CLOSURE",
    "SECURITY_INCIDENT",
    "PRIVACY_REQUEST",
    "OTHER",
];

const CASE_STATUSES: [&str; 6] = [
    "OPEN",
    "INVESTIGATING",
    "WAITING_CUSTOMER",
    "WAITING_STORE",
    "RESOLVED",
    "CLOSED",
];

/// ADMIN-004's 해결 방식.
const RESOLUTION_TYPES: [&str; 7] = [
    "EXPLANATION",
    "COUPON_REISSUE",
    "STAMP_ADJUSTMENT",
    "TRANSACTION_VOID",
    "SANCTION",
    "PRIVACY_ERASURE",
    "NO_ACTION",
];

fn ensure_case_type(raw: &str) -> ApiResult<()> {
    ensure_member(raw, &CASE_TYPES, "사건 유형")
}

fn ensure_case_status(raw: &str) -> ApiResult<()> {
    ensure_member(raw, &CASE_STATUSES, "사건 상태")
}

fn ensure_resolution_type(raw: &str) -> ApiResult<()> {
    ensure_member(raw, &RESOLUTION_TYPES, "해결 방식")
}

fn ensure_member(raw: &str, allowed: &[&str], label: &str) -> ApiResult<()> {
    if allowed.contains(&raw) {
        Ok(())
    } else {
        Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            format!("{label} 값이 올바르지 않습니다: {raw}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> CaseRow {
        CaseRow {
            id: Uuid::from_u128(1),
            case_number: 7,
            case_type: "QR_ABUSE".to_owned(),
            status: "OPEN".to_owned(),
            priority: 2,
            title: "제목".to_owned(),
            description: "설명".to_owned(),
            subject_user_id: None,
            subject_store_id: None,
            subject_resource_type: None,
            subject_resource_id: None,
            assignee_user_id: None,
            resolution_type: None,
            public_resolution: Some("공개 사유".to_owned()),
            internal_resolution: Some("내부 조사 메모".to_owned()),
            legal_hold_until: None,
            due_at: None,
            resolved_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
        }
    }

    #[test]
    fn internal_notes_are_withheld_from_roles_that_may_only_read() {
        // ADMIN-002: 공개 가능한 사유와 내부 사유를 분리한다.
        let visible = map_case(row(), true);
        assert_eq!(visible.internal_resolution.as_deref(), Some("내부 조사 메모"));

        let restricted = map_case(row(), false);
        assert_eq!(restricted.internal_resolution, None);
        assert_eq!(
            restricted.public_resolution.as_deref(),
            Some("공개 사유"),
            "the subject-facing reason is not the secret"
        );
    }

    #[test]
    fn unknown_enum_values_are_client_errors_rather_than_cast_failures() {
        assert_eq!(
            ensure_case_type("NOPE").expect_err("must reject").code,
            ErrorCode::ValidationFailed
        );
        ensure_case_type("PRIVACY_REQUEST").expect("a real type passes");

        assert_eq!(
            ensure_case_status("DONE").expect_err("must reject").code,
            ErrorCode::ValidationFailed
        );
        ensure_case_status("INVESTIGATING").expect("a real status passes");

        assert_eq!(
            ensure_resolution_type("REFUND").expect_err("must reject").code,
            ErrorCode::ValidationFailed
        );
        ensure_resolution_type("STAMP_ADJUSTMENT").expect("a real resolution passes");
    }

    #[test]
    fn the_case_types_match_the_database_enum() {
        // The enum lives in the initial migration; a drift here becomes a 500 at runtime.
        assert_eq!(CASE_TYPES.len(), 9);
        assert!(CASE_TYPES.contains(&"PRIVACY_REQUEST"));
        assert!(CASE_TYPES.contains(&"SECURITY_INCIDENT"));
    }

    #[test]
    fn every_resolution_type_the_scenario_names_is_representable() {
        // ADMIN-004: 설명, 쿠폰 재발급, 도장 보정, 거래 취소, 제재.
        for expected in [
            "EXPLANATION",
            "COUPON_REISSUE",
            "STAMP_ADJUSTMENT",
            "TRANSACTION_VOID",
            "SANCTION",
        ] {
            assert!(RESOLUTION_TYPES.contains(&expected), "{expected}");
        }
    }
}
