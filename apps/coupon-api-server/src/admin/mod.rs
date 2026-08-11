//! 검수·제재·보정·민원 (§10.2 `admin`, §11.5, ADMIN-003).
//!
//! Phase 2 covers the half of §11.5 an investigation needs before anything can be
//! corrected: **find the whole story from one transaction id**, and **simulate a
//! correction before anyone is allowed to run it**.
//!
//! Two rules apply to everything here:
//!
//! * **Reads are audited too.** §11.5 and SEC-005 treat an administrator looking at a
//!   customer's transaction as an event worth recording, not just changing it.
//! * **A correction is never an edit.** The preview describes what new events *would* be
//!   appended; it never proposes rewriting a ledger row (§13.4, product principle 2).

pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{ActorType, AuditEntry, AuditService};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::jobs::{JobKey, JobService, JobSpec};
use crate::loyalty::StampService;
use crate::loyalty::stamps::{LotBalance, available_stamps};

pub use routes::admin_router;

/// One entry of the append-only stamp ledger, as an investigator reads it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdminLedgerEntry {
    pub id: Uuid,
    pub lot_id: Uuid,
    pub event_type: String,
    pub quantity_delta: i64,
    pub reason_code: String,
    pub reward_coupon_id: Option<Uuid>,
    pub source_stamp_transaction_id: Option<Uuid>,
    pub actor_type: String,
    pub actor_user_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
}

/// A reward this transaction created, and where it ended up.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdminRewardSummary {
    pub coupon_id: Uuid,
    pub status: String,
    pub title: String,
    pub issued_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One thing that happened, from whichever table recorded it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TimelineEvent {
    pub occurred_at: DateTime<Utc>,
    /// `ledger`, `coupon_status`, `qr`, `audit` or `transaction`.
    pub source: String,
    pub event: String,
    pub detail: serde_json::Value,
}

/// `GET /admin/transactions/:id` — one id, the whole story (§2.1 목표 5).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdminTransactionDetail {
    pub transaction_id: Uuid,
    pub store_id: Uuid,
    pub store_name: String,
    /// The customer's pseudonymous key. Never their email, phone or name (§11.5 마스킹).
    pub customer_key: Uuid,
    pub customer_masked_name: String,
    pub policy_id: Uuid,
    pub business_day: NaiveDate,
    pub quantity: i64,
    pub status: String,
    pub external_order_ref: Option<String>,
    pub order_snapshot: serde_json::Value,
    pub approved_by_user_id: Uuid,
    pub confirmed_at: DateTime<Utc>,
    pub voided_at: Option<DateTime<Utc>>,
    pub void_reason: Option<String>,
    pub version: i64,
    pub ledger: Vec<AdminLedgerEntry>,
    pub rewards: Vec<AdminRewardSummary>,
    /// Everything above, merged in time order.
    pub timeline: Vec<TimelineEvent>,
}

/// The corrections Phase 2 can simulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AdjustmentType {
    /// Reverse an accrual the owner can no longer reverse themselves (§8.6).
    StampTransactionVoid,
    /// Put stamps back on a customer's board without touching an accrual (ADMIN-004
    /// 도장 보정).
    StampGrant,
}

impl AdjustmentType {
    pub fn as_db(self) -> &'static str {
        match self {
            AdjustmentType::StampTransactionVoid => "STAMP_TRANSACTION_VOID",
            AdjustmentType::StampGrant => "STAMP_GRANT",
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct AdjustmentPreviewRequest {
    /// The case this correction belongs to. §11.5 requires an adjustment to hang off a
    /// case, not to appear from nowhere.
    pub case_id: Uuid,
    pub adjustment_type: AdjustmentType,
    /// Required by `STAMP_TRANSACTION_VOID`.
    pub transaction_id: Option<Uuid>,
    /// Required by `STAMP_GRANT`.
    pub store_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    #[validate(range(min = 1, max = 100, message = "보정 수량은 1~100개여야 합니다."))]
    pub quantity: Option<i64>,
    #[validate(length(min = 1, max = 1000, message = "보정 사유를 입력해 주세요."))]
    pub reason: String,
}

/// What running the correction would do — and what must not have changed by then.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AdjustmentPreview {
    pub adjustment_id: Uuid,
    pub case_id: Uuid,
    pub adjustment_type: AdjustmentType,
    pub status: String,
    /// Taken inside a `REPEATABLE READ` transaction, so every number below is one
    /// consistent picture rather than a series of separate reads (§13.4).
    pub snapshot_taken_at: DateTime<Utc>,
    /// After this, the preview must be taken again (§13.4).
    pub preview_expires_at: DateTime<Utc>,
    pub stamps_before: i64,
    pub stamps_after: i64,
    /// Rewards the correction would have to take back.
    pub affected_coupon_ids: Vec<Uuid>,
    /// The ledger rows the correction would append. Nothing is ever rewritten.
    pub ledger_entries_to_append: Vec<ProposedLedgerEntry>,
    /// The `version` of every mutable row this correction depends on. If any of them has
    /// moved when execution is attempted, the preview is stale and must be retaken.
    pub observed_versions: serde_json::Value,
    /// Reasons the correction cannot be run as described.
    pub blockers: Vec<String>,
    pub executable: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProposedLedgerEntry {
    pub lot_id: Uuid,
    pub event_type: String,
    pub quantity_delta: i64,
    pub reason_code: String,
}

/// `POST /admin/adjustments` — approve a previewed correction and queue it (§11.5).
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ApproveAdjustmentRequest {
    /// The preview to execute. §13.4 requires the execution to be of a specific snapshot,
    /// not of a freshly recomputed plan.
    pub adjustment_id: Uuid,
    #[validate(length(min = 1, max = 1000, message = "승인 사유를 입력해 주세요."))]
    pub approval_reason: String,
}

/// The queued correction.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApprovedAdjustment {
    pub adjustment_id: Uuid,
    pub case_id: Uuid,
    pub status: String,
    pub requested_by_user_id: Uuid,
    pub approved_by_user_id: Uuid,
    pub approved_at: DateTime<Utc>,
    /// ADMIN-003: 대량 보정은 동기 API 가 아니라 검토 가능한 큐 작업으로 실행한다.
    pub execution_job_id: Uuid,
}

pub struct AdminService {
    audit: Arc<AuditService>,
    jobs: Arc<JobService>,
    stamps: Arc<StampService>,
    preview_ttl: chrono::Duration,
}

impl AdminService {
    pub fn new(
        audit: Arc<AuditService>,
        jobs: Arc<JobService>,
        stamps: Arc<StampService>,
        preview_ttl: chrono::Duration,
    ) -> Self {
        Self {
            audit,
            jobs,
            stamps,
            preview_ttl,
        }
    }

    /// `GET /admin/transactions/:id`.
    pub async fn transaction_detail(
        &self,
        pool: &PgPool,
        admin_user_id: Uuid,
        transaction_id: Uuid,
    ) -> ApiResult<AdminTransactionDetail> {
        let transaction = sqlx::query!(
            r#"
            SELECT
                t.id,
                t.store_id,
                s.name AS store_name,
                u.consumer_key,
                u.display_name,
                t.policy_id,
                t.business_day,
                t.quantity,
                t.status::text AS "status!",
                t.external_order_ref,
                t.order_snapshot,
                t.approved_by_user_id,
                t.confirmed_at,
                t.voided_at,
                t.void_reason,
                t.version
            FROM coupon.stamp_transactions t
            JOIN coupon.stores s ON s.id = t.store_id
            JOIN coupon.users u ON u.id = t.user_id
            WHERE t.id = $1
            "#,
            transaction_id,
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::TransactionNotFound))?;

        let ledger = sqlx::query!(
            r#"
            SELECT
                id,
                lot_id,
                event_type::text AS "event_type!",
                quantity_delta,
                reason_code,
                reward_coupon_id,
                source_stamp_transaction_id,
                actor_type::text AS "actor_type!",
                actor_user_id,
                occurred_at
            FROM coupon.stamp_ledger
            WHERE source_stamp_transaction_id = $1
               OR lot_id IN (SELECT id FROM coupon.stamp_lots WHERE source_transaction_id = $1)
            ORDER BY occurred_at, id
            "#,
            transaction_id,
        )
        .fetch_all(pool)
        .await?;

        let ledger: Vec<AdminLedgerEntry> = ledger
            .into_iter()
            .map(|row| AdminLedgerEntry {
                id: row.id,
                lot_id: row.lot_id,
                event_type: row.event_type,
                quantity_delta: i64::from(row.quantity_delta),
                reason_code: row.reason_code,
                reward_coupon_id: row.reward_coupon_id,
                source_stamp_transaction_id: row.source_stamp_transaction_id,
                actor_type: row.actor_type,
                actor_user_id: row.actor_user_id,
                occurred_at: row.occurred_at,
            })
            .collect();

        let coupon_ids: Vec<Uuid> = {
            let mut ids: Vec<Uuid> = ledger.iter().filter_map(|row| row.reward_coupon_id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };

        let rewards = sqlx::query!(
            r#"
            SELECT id, status::text AS "status!", title, issued_at, expires_at, used_at, revoked_at
            FROM coupon.coupon_instances
            WHERE id = ANY($1)
            ORDER BY created_at
            "#,
            &coupon_ids,
        )
        .fetch_all(pool)
        .await?;

        let rewards: Vec<AdminRewardSummary> = rewards
            .into_iter()
            .map(|row| AdminRewardSummary {
                coupon_id: row.id,
                status: row.status,
                title: row.title,
                issued_at: row.issued_at,
                expires_at: row.expires_at,
                used_at: row.used_at,
                revoked_at: row.revoked_at,
            })
            .collect();

        let coupon_events = sqlx::query!(
            r#"
            SELECT coupon_id, from_status::text AS "from_status?", to_status::text AS "to_status!",
                   reason_code, occurred_at
            FROM coupon.coupon_status_events
            WHERE coupon_id = ANY($1) OR transaction_id = $2
            ORDER BY occurred_at, id
            "#,
            &coupon_ids,
            transaction_id,
        )
        .fetch_all(pool)
        .await?;

        // The QR is the evidence that a real customer presented a real token, which is the
        // first thing a "I never got this stamp" complaint needs (§16.2).
        let nonce = sqlx::query!(
            r#"
            SELECT id, issued_at, consumed_at
            FROM coupon.qr_nonces
            WHERE consumed_transaction_id = $1
            "#,
            transaction_id,
        )
        .fetch_optional(pool)
        .await?;

        let audit_entries = sqlx::query!(
            r#"
            SELECT action, actor_type::text AS "actor_type!", reason, occurred_at
            FROM coupon.audit_logs
            WHERE resource_type = 'stamp_transaction' AND resource_id = $1
            ORDER BY occurred_at, id
            "#,
            transaction_id,
        )
        .fetch_all(pool)
        .await?;

        let mut timeline = vec![TimelineEvent {
            occurred_at: transaction.confirmed_at,
            source: "transaction".to_owned(),
            event: "CONFIRMED".to_owned(),
            detail: serde_json::json!({ "quantity": transaction.quantity }),
        }];

        if let Some(voided_at) = transaction.voided_at {
            timeline.push(TimelineEvent {
                occurred_at: voided_at,
                source: "transaction".to_owned(),
                event: "VOIDED".to_owned(),
                detail: serde_json::json!({ "reason": transaction.void_reason }),
            });
        }

        if let Some(nonce) = nonce {
            timeline.push(TimelineEvent {
                occurred_at: nonce.consumed_at.unwrap_or(nonce.issued_at),
                source: "qr".to_owned(),
                event: "NONCE_CONSUMED".to_owned(),
                detail: serde_json::json!({ "nonce_id": nonce.id, "issued_at": nonce.issued_at }),
            });
        }

        timeline.extend(ledger.iter().map(|entry| TimelineEvent {
            occurred_at: entry.occurred_at,
            source: "ledger".to_owned(),
            event: entry.event_type.clone(),
            detail: serde_json::json!({
                "lot_id": entry.lot_id,
                "quantity_delta": entry.quantity_delta,
                "reason_code": entry.reason_code,
            }),
        }));

        timeline.extend(coupon_events.into_iter().map(|event| TimelineEvent {
            occurred_at: event.occurred_at,
            source: "coupon_status".to_owned(),
            event: format!(
                "{} → {}",
                event.from_status.as_deref().unwrap_or("—"),
                event.to_status
            ),
            detail: serde_json::json!({
                "coupon_id": event.coupon_id,
                "reason_code": event.reason_code,
            }),
        }));

        timeline.extend(audit_entries.into_iter().map(|entry| TimelineEvent {
            occurred_at: entry.occurred_at,
            source: "audit".to_owned(),
            event: entry.action,
            detail: serde_json::json!({
                "actor_type": entry.actor_type,
                "reason": entry.reason,
            }),
        }));

        timeline.sort_by_key(|event| event.occurred_at);

        // SEC-005: looking is itself an auditable act.
        self.audit
            .record_standalone(
                pool,
                AuditEntry::new(
                    ActorType::SystemAdmin,
                    "stamp_transaction.viewed",
                    "stamp_transaction",
                )
                .actor(admin_user_id)
                .resource(transaction_id)
                .store(transaction.store_id),
            )
            .await?;

        Ok(AdminTransactionDetail {
            transaction_id: transaction.id,
            store_id: transaction.store_id,
            store_name: transaction.store_name,
            customer_key: transaction.consumer_key,
            customer_masked_name: crate::loyalty::stamps::mask_display_name(
                &transaction.display_name,
            ),
            policy_id: transaction.policy_id,
            business_day: transaction.business_day,
            quantity: i64::from(transaction.quantity),
            status: transaction.status,
            external_order_ref: transaction.external_order_ref,
            order_snapshot: transaction.order_snapshot,
            approved_by_user_id: transaction.approved_by_user_id,
            confirmed_at: transaction.confirmed_at,
            voided_at: transaction.voided_at,
            void_reason: transaction.void_reason,
            version: transaction.version,
            ledger,
            rewards,
            timeline,
        })
    }

    /// `POST /admin/adjustments/preview` (§11.5, §13.4).
    pub async fn preview_adjustment(
        &self,
        pool: &PgPool,
        admin_user_id: Uuid,
        request: &AdjustmentPreviewRequest,
    ) -> ApiResult<AdjustmentPreview> {
        let mut tx = pool.begin().await?;
        // §13.4 asks for a repeatable-read snapshot: every read below has to describe one
        // moment, or the "before" and "after" numbers would not belong to each other.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;

        let snapshot_taken_at = crate::qr::transaction_now(&mut tx).await?;

        sqlx::query_scalar!(
            "SELECT id FROM coupon.admin_cases WHERE id = $1",
            request.case_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            ApiError::with_message(ErrorCode::NotFound, "사건을 찾을 수 없습니다.")
        })?;

        let plan = match request.adjustment_type {
            AdjustmentType::StampTransactionVoid => {
                self.plan_transaction_void(&mut tx, request, snapshot_taken_at)
                    .await?
            }
            AdjustmentType::StampGrant => self.plan_stamp_grant(request).await?,
        };

        let preview_expires_at = snapshot_taken_at + self.preview_ttl;

        let preview_snapshot = serde_json::json!({
            "schema_version": 1,
            "adjustment_type": request.adjustment_type.as_db(),
            "snapshot_taken_at": snapshot_taken_at,
            "stamps_before": plan.stamps_before,
            "stamps_after": plan.stamps_after,
            "affected_coupon_ids": plan.affected_coupon_ids,
            "ledger_entries_to_append": plan.ledger_entries_to_append,
            "observed_versions": plan.observed_versions,
            "blockers": plan.blockers,
        });

        let adjustment_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.admin_adjustments
                (case_id, status, adjustment_type, target_snapshot, preview_snapshot,
                 preview_expires_at, requested_by_user_id, requested_at)
            VALUES ($1, 'DRAFT', $2, $3, $4, $5, $6, $7)
            RETURNING id
            "#,
            request.case_id,
            request.adjustment_type.as_db(),
            serde_json::json!({
                "transaction_id": request.transaction_id,
                "store_id": request.store_id,
                "user_id": request.user_id,
                "quantity": request.quantity,
            }),
            preview_snapshot,
            preview_expires_at,
            admin_user_id,
            snapshot_taken_at,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::SystemAdmin,
                    "admin_adjustment.previewed",
                    "admin_adjustment",
                )
                .actor(admin_user_id)
                .resource(adjustment_id)
                .case(request.case_id)
                .reason(request.reason.clone())
                .metadata(serde_json::json!({
                    "adjustment_type": request.adjustment_type.as_db(),
                    "transaction_id": request.transaction_id,
                })),
            )
            .await?;

        tx.commit().await?;

        Ok(AdjustmentPreview {
            adjustment_id,
            case_id: request.case_id,
            adjustment_type: request.adjustment_type,
            status: "DRAFT".to_owned(),
            snapshot_taken_at,
            preview_expires_at,
            stamps_before: plan.stamps_before,
            stamps_after: plan.stamps_after,
            affected_coupon_ids: plan.affected_coupon_ids,
            ledger_entries_to_append: plan.ledger_entries_to_append,
            observed_versions: plan.observed_versions,
            executable: plan.blockers.is_empty(),
            blockers: plan.blockers,
        })
    }

    /// `POST /admin/adjustments` — approve a preview and queue its execution (§11.5,
    /// ADMIN-003).
    ///
    /// Three rules meet here, and all three are refusals rather than warnings:
    ///
    /// * §3.3 — 원장 보정은 요청자와 승인자가 달라야 한다. The database carries the same
    ///   rule as `ck_admin_adjustment_separation`; this is the copy that can explain it.
    /// * §13.4 — a preview past its expiry, or one whose observed versions have moved,
    ///   must be taken again rather than executed against a world that has changed.
    /// * ADMIN-003 — the execution itself is a queue job, so it is reviewable, resumable
    ///   and attributable rather than a synchronous write nobody can audit afterwards.
    pub async fn approve_adjustment(
        &self,
        pool: &PgPool,
        approver_user_id: Uuid,
        request: &ApproveAdjustmentRequest,
    ) -> ApiResult<ApprovedAdjustment> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        let adjustment = sqlx::query!(
            r#"
            SELECT id, case_id, status::text AS "status!", adjustment_type, target_snapshot,
                   preview_snapshot, preview_expires_at, requested_by_user_id
            FROM coupon.admin_adjustments
            WHERE id = $1
            FOR UPDATE
            "#,
            request.adjustment_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::with_message(ErrorCode::NotFound, "보정 내역을 찾을 수 없습니다."))?;

        if adjustment.status != "DRAFT" && adjustment.status != "PENDING_APPROVAL" {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "이미 승인되었거나 종료된 보정입니다.",
            ));
        }

        // §3.3. Checked before anything else because it is the rule that most needs to be
        // impossible to work around by retrying.
        if adjustment.requested_by_user_id == approver_user_id {
            return Err(ApiError::new(ErrorCode::ApprovalSeparationRequired));
        }

        if now >= adjustment.preview_expires_at {
            return Err(ApiError::new(ErrorCode::PreviewExpired));
        }

        let blockers = adjustment
            .preview_snapshot
            .get("blockers")
            .and_then(serde_json::Value::as_array)
            .map(|list| list.len())
            .unwrap_or(0);
        if blockers > 0 {
            return Err(ApiError::with_message(
                ErrorCode::UnprocessableRequest,
                "실행할 수 없는 보정입니다. 미리보기를 다시 확인해 주세요.",
            ));
        }

        // §13.4: 실행 시 version 이 달라졌으면 다시 preview 한다.
        self.ensure_versions_unchanged(&mut tx, &adjustment.preview_snapshot)
            .await?;

        let spec = JobSpec::new(
            JobKey::execute_adjustment(adjustment.case_id, adjustment.id, 1),
            serde_json::json!({
                "adjustment_id": adjustment.id,
                "case_id": adjustment.case_id,
                "adjustment_type": adjustment.adjustment_type,
            }),
        )
        .resource(adjustment.id)
        .requested_by(approver_user_id);

        let job = self.jobs.enqueue(&mut tx, &spec).await?;

        sqlx::query!(
            r#"
            UPDATE coupon.admin_adjustments
            SET status = 'APPROVED', approved_by_user_id = $2, approved_at = $3,
                approval_reason = $4, execution_job_id = $5
            WHERE id = $1
            "#,
            adjustment.id,
            approver_user_id,
            now,
            request.approval_reason.trim(),
            job.job_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match &error {
            // `ck_admin_adjustment_separation`, in case the check above were ever removed.
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23514") => {
                ApiError::new(ErrorCode::ApprovalSeparationRequired).internal(db.to_string())
            }
            _ => ApiError::from(error),
        })?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::SystemAdmin,
                    "admin_adjustment.approved",
                    "admin_adjustment",
                )
                .actor(approver_user_id)
                .resource(adjustment.id)
                .case(adjustment.case_id)
                .reason(request.approval_reason.clone())
                .metadata(serde_json::json!({
                    "requested_by_user_id": adjustment.requested_by_user_id,
                    "execution_job_id": job.job_id,
                })),
            )
            .await?;

        tx.commit().await?;

        tracing::info!(
            adjustment_id = %adjustment.id,
            job_id = %job.job_id,
            "admin.adjustment_approved"
        );

        Ok(ApprovedAdjustment {
            adjustment_id: adjustment.id,
            case_id: adjustment.case_id,
            status: "APPROVED".to_owned(),
            requested_by_user_id: adjustment.requested_by_user_id,
            approved_by_user_id: approver_user_id,
            approved_at: now,
            execution_job_id: job.job_id,
        })
    }

    /// Apply an approved correction. Called only from the queue worker (ADMIN-003).
    ///
    /// Returns how many ledger events it appended. Nothing is ever rewritten: §13.4 and
    /// product principle 2 make a correction a new event, and that is what lets an
    /// investigator later see both what happened and what was done about it.
    pub async fn execute_adjustment(
        &self,
        pool: &PgPool,
        adjustment_id: Uuid,
        job_id: Uuid,
    ) -> ApiResult<i64> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        let adjustment = sqlx::query!(
            r#"
            SELECT id, case_id, status::text AS "status!", adjustment_type, target_snapshot,
                   preview_snapshot, approved_by_user_id
            FROM coupon.admin_adjustments
            WHERE id = $1
            FOR UPDATE
            "#,
            adjustment_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::with_message(ErrorCode::NotFound, "보정 내역을 찾을 수 없습니다."))?;

        match adjustment.status.as_str() {
            "APPROVED" | "EXECUTING" => {}
            // Re-running a finished correction must be a no-op, not a second correction.
            "SUCCEEDED" => {
                tx.commit().await?;
                return Ok(0);
            }
            other => {
                return Err(ApiError::with_message(
                    ErrorCode::InvalidStateTransition,
                    "승인되지 않은 보정은 실행할 수 없습니다.",
                )
                .internal(format!("adjustment is {other}")));
            }
        }

        let approver = adjustment.approved_by_user_id.ok_or_else(|| {
            ApiError::new(ErrorCode::ApprovalSeparationRequired)
                .internal("an approved adjustment has no approver")
        })?;

        // The world may have moved between approval and execution too.
        self.ensure_versions_unchanged(&mut tx, &adjustment.preview_snapshot)
            .await?;

        let target: AdjustmentTarget =
            serde_json::from_value(adjustment.target_snapshot.clone()).map_err(|error| {
                ApiError::new(ErrorCode::UnprocessableRequest)
                    .internal(format!("malformed adjustment target: {error}"))
            })?;

        let applied = match adjustment.adjustment_type.as_str() {
            "STAMP_TRANSACTION_VOID" => {
                let transaction_id = target.transaction_id.ok_or_else(|| {
                    ApiError::new(ErrorCode::UnprocessableRequest)
                        .internal("a void adjustment names no transaction")
                })?;
                self.stamps
                    .admin_void_transaction(&mut tx, transaction_id, approver, adjustment.id, now)
                    .await?
            }
            "STAMP_GRANT" => {
                let (store_id, user_id, quantity) =
                    match (target.store_id, target.user_id, target.quantity) {
                        (Some(store), Some(user), Some(quantity)) => (store, user, quantity),
                        _ => {
                            return Err(ApiError::new(ErrorCode::UnprocessableRequest)
                                .internal("a grant adjustment is missing its target"));
                        }
                    };
                self.stamps
                    .admin_grant_stamps(
                        &mut tx,
                        store_id,
                        user_id,
                        quantity,
                        approver,
                        adjustment.id,
                        now,
                    )
                    .await?
            }
            other => {
                return Err(ApiError::new(ErrorCode::UnprocessableRequest)
                    .internal(format!("unknown adjustment type {other}")));
            }
        };

        sqlx::query!(
            r#"
            UPDATE coupon.admin_adjustments
            SET status = 'SUCCEEDED', executed_at = $2, execution_job_id = $3,
                execution_result = $4
            WHERE id = $1
            "#,
            adjustment.id,
            now,
            job_id,
            serde_json::json!({ "ledger_events_appended": applied, "executed_at": now }),
        )
        .execute(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::SystemAdmin,
                    "admin_adjustment.executed",
                    "admin_adjustment",
                )
                .actor(approver)
                .resource(adjustment.id)
                .case(adjustment.case_id)
                .metadata(serde_json::json!({
                    "job_id": job_id,
                    "ledger_events_appended": applied,
                })),
            )
            .await?;

        tx.commit().await?;

        tracing::info!(
            adjustment_id = %adjustment.id,
            applied,
            "admin.adjustment_executed"
        );

        Ok(applied)
    }

    /// §13.4: 실행 시 version 이 달라졌으면 다시 preview 한다.
    ///
    /// Compared row by row rather than as one digest, so an unrelated change elsewhere
    /// does not force an administrator to redo a preview that is still accurate.
    async fn ensure_versions_unchanged(
        &self,
        tx: &mut crate::db::Tx<'_>,
        preview_snapshot: &serde_json::Value,
    ) -> ApiResult<()> {
        let observed = preview_snapshot
            .get("observed_versions")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        for (table, rows) in observed.as_object().into_iter().flatten() {
            let Some(rows) = rows.as_object() else {
                continue;
            };

            for (id, expected) in rows {
                let Ok(id) = Uuid::parse_str(id) else {
                    continue;
                };
                let expected = expected.as_i64().unwrap_or_default();

                let current = match table.as_str() {
                    "stamp_transactions" => sqlx::query_scalar!(
                        "SELECT version FROM coupon.stamp_transactions WHERE id = $1",
                        id,
                    )
                    .fetch_optional(&mut **tx)
                    .await?,
                    "coupon_instances" => sqlx::query_scalar!(
                        "SELECT version FROM coupon.coupon_instances WHERE id = $1",
                        id,
                    )
                    .fetch_optional(&mut **tx)
                    .await?,
                    _ => continue,
                };

                if current != Some(expected) {
                    return Err(ApiError::with_message(
                        ErrorCode::PreviewExpired,
                        "보정 대상이 변경되었습니다. 미리보기를 다시 실행해 주세요.",
                    )
                    .internal(format!(
                        "{table}.{id} moved from {expected} to {current:?}"
                    )));
                }
            }
        }

        Ok(())
    }

    async fn plan_transaction_void(
        &self,
        tx: &mut crate::db::Tx<'_>,
        request: &AdjustmentPreviewRequest,
        now: DateTime<Utc>,
    ) -> ApiResult<AdjustmentPlan> {
        let transaction_id = request.transaction_id.ok_or_else(|| {
            ApiError::with_message(
                ErrorCode::ValidationFailed,
                "취소할 거래 ID 가 필요합니다.",
            )
        })?;

        let transaction = sqlx::query!(
            r#"
            SELECT id, store_id, user_id, status::text AS "status!", version
            FROM coupon.stamp_transactions
            WHERE id = $1
            "#,
            transaction_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::TransactionNotFound))?;

        let mut blockers = Vec::new();
        if transaction.status != "CONFIRMED" {
            blockers.push(format!(
                "거래가 이미 {} 상태입니다.",
                transaction.status
            ));
        }

        let lots = sqlx::query!(
            r#"
            SELECT
                lot_id AS "lot_id!",
                balance AS "balance!",
                expires_at AS "expires_at!"
            FROM coupon.stamp_lot_balances
            WHERE store_id = $1 AND user_id = $2
            "#,
            transaction.store_id,
            transaction.user_id,
        )
        .fetch_all(&mut **tx)
        .await?;

        let balances: Vec<LotBalance> = lots
            .into_iter()
            .map(|row| LotBalance {
                lot_id: row.lot_id,
                balance: row.balance,
                expires_at: row.expires_at,
            })
            .collect();
        let stamps_before = available_stamps(&balances, now);

        let consumed = sqlx::query!(
            r#"
            SELECT lot_id, quantity_delta, reward_coupon_id
            FROM coupon.stamp_ledger
            WHERE source_stamp_transaction_id = $1 AND event_type = 'CONSUME_FOR_REWARD'
            ORDER BY occurred_at, id
            "#,
            transaction_id,
        )
        .fetch_all(&mut **tx)
        .await?;

        let earned = sqlx::query!(
            r#"
            SELECT id, original_quantity
            FROM coupon.stamp_lots
            WHERE source_transaction_id = $1
            "#,
            transaction_id,
        )
        .fetch_optional(&mut **tx)
        .await?;

        let mut entries = Vec::new();
        let mut restored = 0i64;
        for row in &consumed {
            restored += i64::from(-row.quantity_delta);
            entries.push(ProposedLedgerEntry {
                lot_id: row.lot_id,
                event_type: "VOID".to_owned(),
                quantity_delta: i64::from(-row.quantity_delta),
                reason_code: "ADMIN_ADJUSTMENT_RESTORE".to_owned(),
            });
        }

        let mut removed = 0i64;
        if let Some(lot) = &earned {
            removed = i64::from(lot.original_quantity);
            entries.push(ProposedLedgerEntry {
                lot_id: lot.id,
                event_type: "ADMIN_ADJUSTMENT".to_owned(),
                quantity_delta: -removed,
                reason_code: "ADMIN_TRANSACTION_VOID".to_owned(),
            });
        } else {
            blockers.push("이 거래에 연결된 도장 묶음이 없습니다.".to_owned());
        }

        let mut affected_coupon_ids: Vec<Uuid> =
            consumed.iter().filter_map(|row| row.reward_coupon_id).collect();
        affected_coupon_ids.sort_unstable();
        affected_coupon_ids.dedup();

        let coupon_versions = sqlx::query!(
            r#"
            SELECT id, status::text AS "status!", version
            FROM coupon.coupon_instances
            WHERE id = ANY($1)
            "#,
            &affected_coupon_ids,
        )
        .fetch_all(&mut **tx)
        .await?;

        for coupon in &coupon_versions {
            if coupon.status == "USED" {
                blockers.push(format!(
                    "리워드 {} 이(가) 이미 사용되어 도장을 자동 복원할 수 없습니다.",
                    coupon.id
                ));
            }
        }

        // Keyed by id so execution can check each row it depends on individually, rather
        // than comparing one opaque digest and having to re-preview for any change at all.
        let mut coupon_version_map = serde_json::Map::new();
        for coupon in &coupon_versions {
            coupon_version_map.insert(coupon.id.to_string(), serde_json::json!(coupon.version));
        }
        let mut transaction_version_map = serde_json::Map::new();
        transaction_version_map.insert(
            transaction.id.to_string(),
            serde_json::json!(transaction.version),
        );

        Ok(AdjustmentPlan {
            stamps_before,
            // Restoring what the rewards consumed, then removing what the accrual granted.
            stamps_after: stamps_before + restored - removed,
            affected_coupon_ids,
            ledger_entries_to_append: entries,
            observed_versions: serde_json::json!({
                "stamp_transactions": transaction_version_map,
                "coupon_instances": coupon_version_map,
            }),
            blockers,
        })
    }

    async fn plan_stamp_grant(
        &self,
        request: &AdjustmentPreviewRequest,
    ) -> ApiResult<AdjustmentPlan> {
        // A grant creates a fresh lot rather than editing an old one, so there is nothing
        // to read a version from — it cannot go stale.
        let quantity = request.quantity.ok_or_else(|| {
            ApiError::with_message(ErrorCode::ValidationFailed, "보정 수량이 필요합니다.")
        })?;
        if request.store_id.is_none() || request.user_id.is_none() {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "보정 대상 상점과 회원이 필요합니다.",
            ));
        }

        Ok(AdjustmentPlan {
            stamps_before: 0,
            stamps_after: quantity,
            affected_coupon_ids: Vec::new(),
            ledger_entries_to_append: vec![ProposedLedgerEntry {
                // A new lot is created at execution time, so there is no id to name yet.
                lot_id: Uuid::nil(),
                event_type: "ADMIN_ADJUSTMENT".to_owned(),
                quantity_delta: quantity,
                reason_code: "ADMIN_STAMP_GRANT".to_owned(),
            }],
            observed_versions: serde_json::json!({}),
            blockers: Vec::new(),
        })
    }
}

/// The `target_snapshot` the preview froze, read back at execution time.
#[derive(Debug, Clone, Deserialize)]
struct AdjustmentTarget {
    #[serde(default)]
    transaction_id: Option<Uuid>,
    #[serde(default)]
    store_id: Option<Uuid>,
    #[serde(default)]
    user_id: Option<Uuid>,
    #[serde(default)]
    quantity: Option<i64>,
}

struct AdjustmentPlan {
    stamps_before: i64,
    stamps_after: i64,
    affected_coupon_ids: Vec<Uuid>,
    ledger_entries_to_append: Vec<ProposedLedgerEntry>,
    observed_versions: serde_json::Value,
    blockers: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjustment_types_use_the_stored_spelling() {
        assert_eq!(
            AdjustmentType::StampTransactionVoid.as_db(),
            "STAMP_TRANSACTION_VOID"
        );
        assert_eq!(AdjustmentType::StampGrant.as_db(), "STAMP_GRANT");
    }

    #[test]
    fn adjustment_types_round_trip_through_the_wire_format() {
        let json = serde_json::to_value(AdjustmentType::StampTransactionVoid).expect("serialises");
        assert_eq!(json, "STAMP_TRANSACTION_VOID");
        assert_eq!(
            serde_json::from_value::<AdjustmentType>(json).expect("deserialises"),
            AdjustmentType::StampTransactionVoid
        );
    }

    #[test]
    fn a_preview_with_blockers_is_not_executable() {
        // The response shape is what the admin app branches on, so pin it.
        let preview = AdjustmentPreview {
            adjustment_id: Uuid::nil(),
            case_id: Uuid::nil(),
            adjustment_type: AdjustmentType::StampTransactionVoid,
            status: "DRAFT".to_owned(),
            snapshot_taken_at: Utc::now(),
            preview_expires_at: Utc::now(),
            stamps_before: 10,
            stamps_after: 9,
            affected_coupon_ids: vec![Uuid::nil()],
            ledger_entries_to_append: Vec::new(),
            observed_versions: serde_json::json!({}),
            blockers: vec!["리워드가 이미 사용되었습니다.".to_owned()],
            executable: false,
        };

        let json = serde_json::to_value(&preview).expect("serialises");
        assert_eq!(json["executable"], false);
        assert!(json["blockers"].as_array().is_some_and(|list| !list.is_empty()));
        assert!(
            json.get("observed_versions").is_some(),
            "§13.4 requires the versions the execution must re-check"
        );
    }
}
