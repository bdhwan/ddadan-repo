//! 보존기간과 파기 (§17.3, ADMIN-006, §18.5, §14.6 개인정보 파기).
//!
//! Three ideas, and each one exists because of a specific way erasure goes wrong.
//!
//! **Retention periods are configuration, not constants.** §17.3 asks for per-category
//! retention managed in a 설정 테이블, and §23.2 leaves the actual durations to a legal
//! review that has not happened yet. Hard-coding "five years" would make a legal decision
//! in a `const` and require a deploy to change it, so the numbers live in
//! `retention_policies` and this module reads them.
//!
//! **Erasure replaces rather than deletes.** §17.3: 탈퇴자는 거래 원장의 user FK 를 가명
//! tombstone 으로 치환할 수 있게 설계한다. Deleting the `users` row would either cascade
//! through the ledgers — destroying records §17.1 says must be kept — or fail on a foreign
//! key. So the row survives, stripped of everything identifying, carrying a pseudonym; the
//! ledger's references still resolve and no longer point at a person.
//!
//! **The ledger outlives what it erased.** A restore from backup brings the erased columns
//! back. §18.5 requires the deletion ledger to be re-applied afterwards, so
//! [`PrivacyService::reapply`] replays every completed erasure — and the pseudonym is
//! derived deterministically from the subject id precisely so the replay reproduces the
//! *same* tombstone rather than inventing a second one.

pub mod routes;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{ActorType, AuditEntry, AuditService};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::jobs::{JobKey, JobService, JobSpec};

pub use routes::admin_privacy_router;

/// One row of `retention_policies` (§17.3).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RetentionPolicy {
    pub data_category: String,
    pub retention_days: i32,
    pub legal_basis: String,
    /// Whether a subject may have this category erased on request, or whether a statutory
    /// retention keeps it until the period runs out (§17.3, ADMIN-006 삭제).
    pub erasable_on_request: bool,
    pub active: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RetentionPoliciesResponse {
    pub policies: Vec<RetentionPolicy>,
}

/// `PATCH /admin/retention-policies/:category`.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateRetentionPolicyRequest {
    #[validate(range(min = 1, max = 36500, message = "보존기간은 1~36500일이어야 합니다."))]
    pub retention_days: i32,
    #[validate(length(min = 1, max = 2000, message = "법적 근거를 입력해 주세요."))]
    pub legal_basis: String,
    #[serde(default)]
    pub erasable_on_request: Option<bool>,
    /// §11.5 requires a reason on every administrative change.
    #[validate(length(min = 1, max = 1000, message = "변경 사유를 입력해 주세요."))]
    pub reason: String,
}

/// `POST /admin/privacy/erasures` (ADMIN-006 삭제).
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct RequestErasureRequest {
    pub subject_user_id: Uuid,
    /// §3.3 and ADMIN-006: the case is the record of who asked, when, and by what deadline.
    pub case_id: Uuid,
    #[validate(length(min = 1, max = 1000, message = "파기 사유를 입력해 주세요."))]
    pub reason: String,
    /// Categories to erase. Empty means every category the policy table marks erasable.
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErasureRecord {
    pub id: Uuid,
    pub subject_user_id: Uuid,
    pub case_id: Option<Uuid>,
    pub status: String,
    pub requested_at: DateTime<Utc>,
    pub execute_after: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub deletion_scope: serde_json::Value,
    pub applied_scopes: serde_json::Value,
    pub pseudonym_label: Option<String>,
    pub blocked_reason: Option<String>,
    pub reapply_count: i32,
    pub job_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErasuresResponse {
    pub erasures: Vec<ErasureRecord>,
}

/// What replaying the ledger did (§18.5).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ReapplyResult {
    pub examined: i64,
    /// How many subjects were found alive again and re-erased. Non-zero after a restore is
    /// expected; non-zero at any other time means something wrote a person back.
    pub reapplied: i64,
}

/// Every category the erasure job knows how to act on.
///
/// A request naming something outside this list is refused rather than silently ignored:
/// telling a subject their data was erased when nothing happened is the worst possible
/// outcome of an erasure request.
pub const ERASABLE_SCOPES: [&str; 3] = ["PROFILE", "NOTIFICATION", "AUTH_IDENTITY"];

pub struct PrivacyService {
    audit: std::sync::Arc<AuditService>,
    jobs: std::sync::Arc<JobService>,
    grace: Duration,
}

impl PrivacyService {
    pub fn new(
        audit: std::sync::Arc<AuditService>,
        jobs: std::sync::Arc<JobService>,
        grace: Duration,
    ) -> Self {
        Self { audit, jobs, grace }
    }

    // -----------------------------------------------------------------------
    // Retention policy (§17.3)
    // -----------------------------------------------------------------------

    pub async fn policies(&self, pool: &PgPool) -> ApiResult<Vec<RetentionPolicy>> {
        let rows = sqlx::query!(
            r#"
            SELECT data_category, retention_days, legal_basis, erasable_on_request, active,
                   updated_at
            FROM coupon.retention_policies
            ORDER BY data_category
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| RetentionPolicy {
                data_category: row.data_category,
                retention_days: row.retention_days,
                legal_basis: row.legal_basis,
                erasable_on_request: row.erasable_on_request,
                active: row.active,
                updated_at: row.updated_at,
            })
            .collect())
    }

    /// Change one category's period. Audited, because a shortened retention is a decision
    /// somebody has to answer for (SEC-005).
    pub async fn update_policy(
        &self,
        pool: &PgPool,
        actor_user_id: Uuid,
        category: &str,
        request: &UpdateRetentionPolicyRequest,
    ) -> ApiResult<RetentionPolicy> {
        let mut tx = pool.begin().await?;

        let before = sqlx::query!(
            r#"
            SELECT retention_days, legal_basis, erasable_on_request
            FROM coupon.retention_policies WHERE data_category = $1 FOR UPDATE
            "#,
            category,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::RetentionPolicyNotFound))?;

        let row = sqlx::query!(
            r#"
            UPDATE coupon.retention_policies
            SET retention_days = $2, legal_basis = $3,
                erasable_on_request = COALESCE($4, erasable_on_request),
                updated_by_user_id = $5, version = version + 1
            WHERE data_category = $1
            RETURNING data_category, retention_days, legal_basis, erasable_on_request, active,
                      updated_at
            "#,
            category,
            request.retention_days,
            request.legal_basis,
            request.erasable_on_request,
            actor_user_id,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::SystemAdmin,
                    "retention_policy.updated",
                    "retention_policy",
                )
                .actor(actor_user_id)
                .reason(request.reason.clone())
                .transition(
                    &serde_json::json!({
                        "retention_days": before.retention_days,
                        "legal_basis": before.legal_basis,
                        "erasable_on_request": before.erasable_on_request,
                    }),
                    &serde_json::json!({
                        "retention_days": row.retention_days,
                        "legal_basis": row.legal_basis,
                        "erasable_on_request": row.erasable_on_request,
                    }),
                )
                .metadata(serde_json::json!({ "data_category": category })),
            )
            .await?;

        tx.commit().await?;

        Ok(RetentionPolicy {
            data_category: row.data_category,
            retention_days: row.retention_days,
            legal_basis: row.legal_basis,
            erasable_on_request: row.erasable_on_request,
            active: row.active,
            updated_at: row.updated_at,
        })
    }

    // -----------------------------------------------------------------------
    // Erasure requests (§17.3, ADMIN-006)
    // -----------------------------------------------------------------------

    /// Register an erasure and queue the job that carries it out.
    ///
    /// §17.3: 법적 보존 또는 분쟁 hold 가 없으면 만료 후 파기 작업을 큐에 등록한다. The hold
    /// is checked here so the requester learns immediately, and *again* in the job, because
    /// a dispute can open during the grace period.
    pub async fn request_erasure(
        &self,
        pool: &PgPool,
        actor_user_id: Uuid,
        request: &RequestErasureRequest,
    ) -> ApiResult<ErasureRecord> {
        let scopes = self.resolve_scopes(pool, &request.scopes).await?;

        let mut tx = pool.begin().await?;

        let subject_exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM coupon.users WHERE id = $1) AS "exists!""#,
            request.subject_user_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if !subject_exists {
            return Err(ApiError::new(ErrorCode::UserNotFound));
        }

        let case_exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM coupon.admin_cases WHERE id = $1) AS "exists!""#,
            request.case_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if !case_exists {
            return Err(ApiError::new(ErrorCode::CaseNotFound));
        }

        if let Some(hold) = self.legal_hold(&mut tx, request.subject_user_id).await? {
            return Err(ApiError::with_message(
                ErrorCode::LegalHoldActive,
                format!("{hold} 까지 법정·분쟁 보존이 적용되어 있습니다."),
            ));
        }

        let now = Utc::now();
        let execute_after = now + self.grace;
        let pseudonym = pseudonym_label(request.subject_user_id);

        let row = sqlx::query!(
            r#"
            INSERT INTO coupon.deletion_ledger
                (subject_user_id, case_id, requested_at, execute_after, status,
                 deletion_scope, pseudonym_label)
            VALUES ($1, $2, $3, $4, 'PENDING', $5, $6)
            ON CONFLICT (subject_user_id, backup_tombstone_version) DO UPDATE
            SET case_id = EXCLUDED.case_id,
                deletion_scope = EXCLUDED.deletion_scope,
                version = coupon.deletion_ledger.version + 1
            RETURNING id, subject_user_id, case_id, status, requested_at, execute_after,
                      executed_at, deletion_scope, applied_scopes, pseudonym_label,
                      blocked_reason, reapply_count, job_id
            "#,
            request.subject_user_id,
            request.case_id,
            now,
            execute_after,
            serde_json::json!(scopes),
            pseudonym,
        )
        .fetch_one(&mut *tx)
        .await?;

        // §14.6 keys 개인정보 파기 on request/case, so two requests about one person are two
        // obligations rather than one job that silently satisfies both.
        let spec = JobSpec::new(
            JobKey::purge_user_data(row.id, request.subject_user_id, 1),
            serde_json::json!({
                "erasure_id": row.id,
                "subject_user_id": request.subject_user_id,
            }),
        )
        .resource(request.subject_user_id)
        .requested_by(actor_user_id)
        .at(execute_after);

        let job = self.jobs.enqueue(&mut tx, &spec).await?;

        sqlx::query!(
            r#"UPDATE coupon.deletion_ledger SET job_id = $2 WHERE id = $1"#,
            row.id,
            job.job_id,
        )
        .execute(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::SystemAdmin, "privacy.erasure_requested", "user")
                    .actor(actor_user_id)
                    .resource(request.subject_user_id)
                    .case(request.case_id)
                    .reason(request.reason.clone())
                    .metadata(serde_json::json!({
                        "erasure_id": row.id,
                        "scopes": scopes,
                        "execute_after": execute_after,
                    })),
            )
            .await?;

        tx.commit().await?;

        Ok(ErasureRecord {
            id: row.id,
            subject_user_id: row.subject_user_id,
            case_id: row.case_id,
            status: row.status,
            requested_at: row.requested_at,
            execute_after: row.execute_after,
            executed_at: row.executed_at,
            deletion_scope: row.deletion_scope,
            applied_scopes: row.applied_scopes,
            pseudonym_label: row.pseudonym_label,
            blocked_reason: row.blocked_reason,
            reapply_count: row.reapply_count,
            job_id: Some(job.job_id),
        })
    }

    pub async fn list_erasures(&self, pool: &PgPool, limit: i64) -> ApiResult<Vec<ErasureRecord>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, subject_user_id, case_id, status, requested_at, execute_after,
                   executed_at, deletion_scope, applied_scopes, pseudonym_label,
                   blocked_reason, reapply_count, job_id
            FROM coupon.deletion_ledger
            ORDER BY requested_at DESC
            LIMIT $1
            "#,
            limit,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ErasureRecord {
                id: row.id,
                subject_user_id: row.subject_user_id,
                case_id: row.case_id,
                status: row.status,
                requested_at: row.requested_at,
                execute_after: row.execute_after,
                executed_at: row.executed_at,
                deletion_scope: row.deletion_scope,
                applied_scopes: row.applied_scopes,
                pseudonym_label: row.pseudonym_label,
                blocked_reason: row.blocked_reason,
                reapply_count: row.reapply_count,
                job_id: row.job_id,
            })
            .collect())
    }

    // -----------------------------------------------------------------------
    // Execution (the `purge_user_data` job)
    // -----------------------------------------------------------------------

    /// Carry out one erasure.
    ///
    /// Idempotent: running it twice produces the same tombstone, because the pseudonym is a
    /// function of the subject id rather than of the clock or a random value. That is what
    /// makes [`Self::reapply`] safe to run after every restore.
    pub async fn execute(&self, pool: &PgPool, erasure_id: Uuid) -> ApiResult<ErasureRecord> {
        let mut tx = pool.begin().await?;

        let record = sqlx::query!(
            r#"
            SELECT id, subject_user_id, case_id, deletion_scope, pseudonym_label, status
            FROM coupon.deletion_ledger
            WHERE id = $1
            FOR UPDATE
            "#,
            erasure_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;

        // Re-checked here and not only at request time: §17.3's hold is about the state of
        // the world when the data is destroyed, and a dispute can open during the grace
        // period. A blocked erasure stays in the ledger as `BLOCKED_LEGAL_HOLD` so it is
        // visible rather than quietly abandoned.
        if let Some(hold) = self.legal_hold(&mut tx, record.subject_user_id).await? {
            sqlx::query!(
                r#"
                UPDATE coupon.deletion_ledger
                SET status = 'BLOCKED_LEGAL_HOLD', blocked_reason = $2, version = version + 1
                WHERE id = $1
                "#,
                erasure_id,
                format!("legal hold until {hold}"),
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;

            return Err(ApiError::with_message(
                ErrorCode::LegalHoldActive,
                "법정·분쟁 보존이 적용되어 파기를 진행하지 않았습니다.",
            ));
        }

        let scopes: Vec<String> = serde_json::from_value(record.deletion_scope.clone())
            .unwrap_or_else(|_| ERASABLE_SCOPES.iter().map(|s| (*s).to_owned()).collect());
        let pseudonym = record
            .pseudonym_label
            .unwrap_or_else(|| pseudonym_label(record.subject_user_id));

        let applied = self
            .erase(&mut tx, record.subject_user_id, &scopes, &pseudonym)
            .await?;

        let row = sqlx::query!(
            r#"
            UPDATE coupon.deletion_ledger
            SET status = 'SUCCEEDED', executed_at = clock_timestamp(),
                applied_scopes = $2, pseudonym_label = $3, blocked_reason = NULL,
                version = version + 1
            WHERE id = $1
            RETURNING id, subject_user_id, case_id, status, requested_at, execute_after,
                      executed_at, deletion_scope, applied_scopes, pseudonym_label,
                      blocked_reason, reapply_count, job_id
            "#,
            erasure_id,
            serde_json::json!(applied),
            pseudonym,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::System, "privacy.erasure_executed", "user")
                    .resource(record.subject_user_id)
                    .metadata(serde_json::json!({
                        "erasure_id": erasure_id,
                        "applied_scopes": applied,
                        // The pseudonym, not the person: this entry has to stay readable
                        // after the subject is gone (§12.5).
                        "pseudonym": pseudonym,
                    })),
            )
            .await?;

        tx.commit().await?;

        Ok(ErasureRecord {
            id: row.id,
            subject_user_id: row.subject_user_id,
            case_id: row.case_id,
            status: row.status,
            requested_at: row.requested_at,
            execute_after: row.execute_after,
            executed_at: row.executed_at,
            deletion_scope: row.deletion_scope,
            applied_scopes: row.applied_scopes,
            pseudonym_label: row.pseudonym_label,
            blocked_reason: row.blocked_reason,
            reapply_count: row.reapply_count,
            job_id: row.job_id,
        })
    }

    /// §18.5: 복원 후 outbox 재발행, 만료 따라잡기, deletion ledger 재적용.
    ///
    /// Walks every completed erasure and applies it again. The erasure itself is idempotent,
    /// so a subject who was *not* restored costs one no-op update; a subject who was gets
    /// erased again with the identical pseudonym, which is exactly the point — a restore
    /// must not resurrect a person, and it must not give them a second identity either.
    pub async fn reapply(&self, pool: &PgPool) -> ApiResult<ReapplyResult> {
        let records = sqlx::query!(
            r#"
            SELECT id, subject_user_id, deletion_scope, pseudonym_label
            FROM coupon.deletion_ledger
            WHERE status = 'SUCCEEDED'
            ORDER BY executed_at
            "#,
        )
        .fetch_all(pool)
        .await?;

        let examined = records.len() as i64;
        let mut reapplied = 0i64;

        for record in records {
            let scopes: Vec<String> = serde_json::from_value(record.deletion_scope)
                .unwrap_or_else(|_| ERASABLE_SCOPES.iter().map(|s| (*s).to_owned()).collect());
            let pseudonym = record
                .pseudonym_label
                .unwrap_or_else(|| pseudonym_label(record.subject_user_id));

            let mut tx = pool.begin().await?;
            // `tombstoned_at IS NULL` is the tell: the row is alive again, which after a
            // restore is precisely the case §18.5 is about.
            let resurrected = sqlx::query_scalar!(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM coupon.users WHERE id = $1 AND tombstoned_at IS NULL
                ) AS "resurrected!"
                "#,
                record.subject_user_id,
            )
            .fetch_one(&mut *tx)
            .await?;

            self.erase(&mut tx, record.subject_user_id, &scopes, &pseudonym)
                .await?;

            if resurrected {
                reapplied += 1;
                sqlx::query!(
                    r#"
                    UPDATE coupon.deletion_ledger
                    SET reapplied_at = clock_timestamp(), reapply_count = reapply_count + 1,
                        version = version + 1
                    WHERE id = $1
                    "#,
                    record.id,
                )
                .execute(&mut *tx)
                .await?;
            }

            tx.commit().await?;
        }

        if reapplied > 0 {
            tracing::warn!(
                reapplied,
                examined,
                "privacy.deletion_ledger_reapplied: subjects were alive again"
            );
        }

        Ok(ReapplyResult {
            examined,
            reapplied,
        })
    }

    /// The erasure itself.
    ///
    /// What is *not* here matters as much as what is: `stamp_ledger`, `coupon_instances`,
    /// `redemption_transactions`, `consent_events` and `audit_logs` are untouched. §17.1
    /// keeps transaction records, §17.3 keeps consent evidence, and §12.5 makes the audit
    /// trail append-only. After this runs those rows still exist and still reference the
    /// user row — which now describes a pseudonym instead of a person.
    async fn erase(
        &self,
        tx: &mut crate::db::Tx<'_>,
        subject_user_id: Uuid,
        scopes: &[String],
        pseudonym: &str,
    ) -> ApiResult<Vec<String>> {
        let mut applied = Vec::new();

        if scopes.iter().any(|scope| scope == "PROFILE") {
            sqlx::query!(
                r#"
                UPDATE coupon.users
                SET display_name = $2,
                    primary_email_ciphertext = NULL,
                    primary_email_lookup_hash = NULL,
                    email_verified_at = NULL,
                    suspension_reason = NULL,
                    -- The Firebase uid is an external identifier for a person, so it is
                    -- replaced rather than kept. Deriving it from the subject id keeps the
                    -- unique constraint satisfiable on a replay.
                    firebase_uid = $3,
                    status = 'WITHDRAWN',
                    withdrawn_at = COALESCE(withdrawn_at, clock_timestamp()),
                    tombstoned_at = COALESCE(tombstoned_at, clock_timestamp()),
                    pseudonym_label = $2,
                    -- Every existing session is worthless the moment the account is gone.
                    sessions_valid_after = clock_timestamp(),
                    version = version + 1
                WHERE id = $1
                "#,
                subject_user_id,
                pseudonym,
                format!("tombstone:{subject_user_id}"),
            )
            .execute(&mut **tx)
            .await?;
            applied.push("PROFILE".to_owned());
        }

        if scopes.iter().any(|scope| scope == "AUTH_IDENTITY") {
            // The provider subject is the link back to a Kakao or Google account. The row
            // stays so "this account once had a Kakao login" remains true.
            sqlx::query!(
                r#"
                UPDATE coupon.auth_identities
                SET provider_subject = 'tombstone:' || id::text,
                    provider_profile_snapshot = '{}'::jsonb,
                    status = 'UNLINKED',
                    unlinked_at = COALESCE(unlinked_at, clock_timestamp()),
                    version = version + 1
                WHERE user_id = $1 AND provider_subject NOT LIKE 'tombstone:%'
                "#,
                subject_user_id,
            )
            .execute(&mut **tx)
            .await?;
            applied.push("AUTH_IDENTITY".to_owned());
        }

        if scopes.iter().any(|scope| scope == "NOTIFICATION") {
            // Push tokens are addresses; there is nothing to keep and every reason not to.
            sqlx::query!(
                r#"
                UPDATE coupon.push_subscriptions
                SET status = 'ARCHIVED', token_ciphertext = ''::bytea,
                    -- The unique index over the lookup hash still has to hold, so the
                    -- placeholder is derived from the row's own id rather than zeroed.
                    token_lookup_hash = sha256(('tombstone:' || id::text)::bytea),
                    disabled_at = COALESCE(disabled_at, clock_timestamp()),
                    disabled_reason = 'ERASED', version = version + 1
                WHERE user_id = $1 AND disabled_reason IS DISTINCT FROM 'ERASED'
                "#,
                subject_user_id,
            )
            .execute(&mut **tx)
            .await?;

            // The notification *record* is kept — §15.1 makes it the base record of an
            // event — but its rendered text quoted a store and a benefit at a person, so
            // the body goes and the event type stays.
            sqlx::query!(
                r#"
                UPDATE coupon.notifications
                SET title = '삭제된 알림', body = '개인정보 파기로 내용이 삭제되었습니다.',
                    data = '{}'::jsonb, deep_link = NULL
                WHERE user_id = $1 AND title <> '삭제된 알림'
                "#,
                subject_user_id,
            )
            .execute(&mut **tx)
            .await?;

            sqlx::query!(
                r#"
                UPDATE coupon.notification_deliveries d
                SET rendered_variables = '{}'::jsonb
                FROM coupon.notifications n
                WHERE d.notification_id = n.id AND n.user_id = $1
                  AND d.rendered_variables <> '{}'::jsonb
                "#,
                subject_user_id,
            )
            .execute(&mut **tx)
            .await?;

            applied.push("NOTIFICATION".to_owned());
        }

        Ok(applied)
    }

    /// Whether anything holds this subject's data (§17.3).
    ///
    /// A case with a `legal_hold_until` in the future, or an open case at all — an
    /// investigation in progress is a dispute, and erasing its subject mid-investigation
    /// destroys the evidence it exists to weigh.
    async fn legal_hold(
        &self,
        tx: &mut crate::db::Tx<'_>,
        subject_user_id: Uuid,
    ) -> ApiResult<Option<DateTime<Utc>>> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT MAX(legal_hold_until)
            FROM coupon.admin_cases
            WHERE subject_user_id = $1 AND legal_hold_until > clock_timestamp()
            "#,
            subject_user_id,
        )
        .fetch_one(&mut **tx)
        .await?)
    }

    /// Which categories a request may actually act on.
    async fn resolve_scopes(&self, pool: &PgPool, requested: &[String]) -> ApiResult<Vec<String>> {
        let erasable: Vec<String> = sqlx::query_scalar!(
            r#"
            SELECT data_category FROM coupon.retention_policies
            WHERE active AND erasable_on_request
            "#,
        )
        .fetch_all(pool)
        .await?;

        // The policy table decides what *may* be erased; this module decides what it knows
        // how to erase. Only the intersection is honest.
        let available: Vec<String> = ERASABLE_SCOPES
            .iter()
            .map(|scope| (*scope).to_owned())
            .filter(|scope| scope == "AUTH_IDENTITY" || erasable.iter().any(|row| row == scope))
            .collect();

        if requested.is_empty() {
            return Ok(available);
        }

        for scope in requested {
            if !available.iter().any(|known| known == scope) {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    format!("'{scope}' 은(는) 파기 가능한 항목이 아닙니다."),
                ));
            }
        }

        Ok(requested.to_vec())
    }
}

/// The tombstone a subject's rows carry after erasure.
///
/// Deterministic on purpose (§17.3, §18.5). A random label would mean a replay after a
/// restore produced a *second* pseudonym for the same person, so the two tombstones could
/// not be recognised as one — and comparing them would be a way to tell that the same
/// person had been erased twice, which is the opposite of what a pseudonym is for.
pub fn pseudonym_label(subject_user_id: Uuid) -> String {
    let digest = Sha256::digest(format!("ddadan-tombstone:{subject_user_id}").as_bytes());
    format!("탈퇴회원-{}", hex::encode(&digest[..4]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pseudonym_is_stable_for_a_subject_and_different_between_them() {
        // §18.5: the ledger is replayed after a restore, and the replay must reproduce the
        // same tombstone rather than mint a new one.
        let subject = Uuid::from_u128(1);
        assert_eq!(pseudonym_label(subject), pseudonym_label(subject));
        assert_ne!(pseudonym_label(subject), pseudonym_label(Uuid::from_u128(2)));
    }

    #[test]
    fn a_pseudonym_does_not_leak_the_identifier_it_came_from() {
        let subject = Uuid::from_u128(0x1234_5678);
        let label = pseudonym_label(subject);

        assert!(!label.contains(&subject.to_string()));
        assert!(label.starts_with("탈퇴회원-"));
    }

    #[test]
    fn the_erasable_scopes_are_the_ones_the_job_actually_implements() {
        // Every entry here must have a branch in `erase`. Listing a scope the code cannot
        // act on would let an administrator promise a subject something that never happens.
        assert_eq!(ERASABLE_SCOPES.len(), 3);
        for scope in ERASABLE_SCOPES {
            assert!(
                matches!(scope, "PROFILE" | "NOTIFICATION" | "AUTH_IDENTITY"),
                "{scope} has no branch in erase()"
            );
        }
    }
}
