//! 작업 등록·실행·체크포인트·DLQ (§10.2 `jobs`, §14).
//!
//! Owns `job_registry`, `job_attempts` and `outbox_events`.
//!
//! §14.1 asks for BullMQ's operational model in Rust with one extra guarantee: **the same
//! logical job never runs twice anywhere in the cluster**. That is delivered by three
//! independent layers, and it matters that they are independent — §23.1 calls it 삼중
//! 방어 for a reason:
//!
//! 1. **`job_registry`** — a partial unique index over `unique_key` for the active
//!    statuses, so a second registration of the same logical job is refused by the
//!    database rather than deduplicated by a lookup that could race (§12.6-10).
//! 2. **A PostgreSQL advisory lock** on a stable 64-bit hash of that key, taken on a
//!    *dedicated connection*. Two workers that both received the message contend here, and
//!    a worker that dies has its lock released by the server closing its connection
//!    (§14.5-9) — no lease, no TTL, nothing to expire wrongly.
//! 3. **Domain uniqueness** — `(campaign_id, user_id, ordinal)` and friends. §14.2 is
//!    explicit that a Redis lock alone must not be what keeps the ledger honest, so even a
//!    job that somehow ran twice cannot issue the same coupon twice.
//!
//! Redis is transport only. Every state question is answered from PostgreSQL, which is
//! why JOB-005 (Redis is down) degrades to "jobs start late" rather than "jobs are lost":
//! the outbox still holds the intent, and the worker also polls the registry directly.

pub mod retry;
pub mod transport;
pub mod worker;

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::pool::PoolConnection;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};

pub use retry::{RetryClass, RetryDecision, backoff_for_attempt, classify_api_error};

/// The `{job_type}` component of a unique key (§14.3), and the operational policy each
/// kind of work carries (§14.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobType {
    /// 캠페인 대상 산정 — freeze who was eligible at publish time.
    BuildCampaignAudience,
    /// 쿠폰 대량 발급.
    IssueCampaign,
    /// 캠페인 회수.
    RevokeCampaign,
    /// 만료 처리.
    ExpireCoupons,
    /// 관리자 원장 보정 실행 (ADMIN-003: 대량 보정은 동기 API 가 아니라 큐 작업이다).
    ExecuteAdjustment,
}

impl JobType {
    pub fn as_db(self) -> &'static str {
        match self {
            JobType::BuildCampaignAudience => "build_campaign_audience",
            JobType::IssueCampaign => "issue_campaign",
            JobType::RevokeCampaign => "revoke_campaign",
            JobType::ExpireCoupons => "expire_coupons",
            JobType::ExecuteAdjustment => "execute_adjustment",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        match raw {
            "build_campaign_audience" => Some(JobType::BuildCampaignAudience),
            "issue_campaign" => Some(JobType::IssueCampaign),
            "revoke_campaign" => Some(JobType::RevokeCampaign),
            "expire_coupons" => Some(JobType::ExpireCoupons),
            "execute_adjustment" => Some(JobType::ExecuteAdjustment),
            _ => None,
        }
    }

    /// Rows per checkpointed batch (§14.6). Small enough that a pause is honoured
    /// promptly and a crash loses little work; large enough that the per-batch overhead
    /// does not dominate.
    pub fn batch_size(self) -> i64 {
        match self {
            JobType::BuildCampaignAudience => 1_000,
            JobType::IssueCampaign => 500,
            JobType::RevokeCampaign => 500,
            JobType::ExpireCoupons => 1_000,
            JobType::ExecuteAdjustment => 1,
        }
    }

    /// §14.6's retry column.
    pub fn retry_budget(self) -> RetryBudget {
        match self {
            JobType::BuildCampaignAudience => RetryBudget::Limited(5),
            JobType::IssueCampaign => RetryBudget::Limited(10),
            JobType::RevokeCampaign => RetryBudget::Limited(10),
            // §14.6 gives expiry 무제한 지연 재시도. It can afford that because JOB-004
            // says the online read decides expiry itself, so a sweep that keeps retrying
            // is behind on tidying rather than letting anyone spend an expired coupon.
            JobType::ExpireCoupons => RetryBudget::UnlimitedDelayed { alert_after: 20 },
            JobType::ExecuteAdjustment => RetryBudget::Limited(5),
        }
    }

    /// How long a `RUNNING` job may go silent before another worker may assume the
    /// previous one died (§14.5-9). The advisory lock is what actually prevents the
    /// overlap; this only decides when it is worth *trying*.
    pub fn visibility_timeout_secs(self) -> i32 {
        match self {
            JobType::ExecuteAdjustment => 120,
            _ => 300,
        }
    }
}

/// How many times a job type is willing to fail before it stops trying (§14.6, §14.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryBudget {
    Limited(i32),
    /// Never dead-letters; `alert_after` is the attempt count that is worth paging about.
    UnlimitedDelayed { alert_after: i32 },
}

impl RetryBudget {
    /// The number stored in `job_registry.max_attempts`. For the unlimited budget this is
    /// the alerting threshold, not a stopping point — [`RetryBudget::exhausted`] is what
    /// decides whether to give up.
    pub fn recorded_max_attempts(self) -> i32 {
        match self {
            RetryBudget::Limited(limit) => limit,
            RetryBudget::UnlimitedDelayed { alert_after } => alert_after,
        }
    }

    pub fn exhausted(self, attempts: i32) -> bool {
        match self {
            RetryBudget::Limited(limit) => attempts >= limit,
            RetryBudget::UnlimitedDelayed { .. } => false,
        }
    }
}

/// `coupon.job_status` (§14.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    PendingOutbox,
    Queued,
    Running,
    RetryWait,
    PauseRequested,
    Paused,
    Succeeded,
    DeadLetter,
    Cancelled,
}

impl JobStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            JobStatus::PendingOutbox => "PENDING_OUTBOX",
            JobStatus::Queued => "QUEUED",
            JobStatus::Running => "RUNNING",
            JobStatus::RetryWait => "RETRY_WAIT",
            JobStatus::PauseRequested => "PAUSE_REQUESTED",
            JobStatus::Paused => "PAUSED",
            JobStatus::Succeeded => "SUCCEEDED",
            JobStatus::DeadLetter => "DEAD_LETTER",
            JobStatus::Cancelled => "CANCELLED",
        }
    }

    /// An unrecognised status reads as `DEAD_LETTER`: a state this build does not
    /// understand must not be treated as runnable.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "PENDING_OUTBOX" => JobStatus::PendingOutbox,
            "QUEUED" => JobStatus::Queued,
            "RUNNING" => JobStatus::Running,
            "RETRY_WAIT" => JobStatus::RetryWait,
            "PAUSE_REQUESTED" => JobStatus::PauseRequested,
            "PAUSED" => JobStatus::Paused,
            "SUCCEEDED" => JobStatus::Succeeded,
            "CANCELLED" => JobStatus::Cancelled,
            _ => JobStatus::DeadLetter,
        }
    }

    /// The statuses `uq_job_registry_active_key` covers — that is, the ones that mean
    /// "this logical job is still in flight" (§12.6-10).
    pub const ACTIVE: [JobStatus; 6] = [
        JobStatus::PendingOutbox,
        JobStatus::Queued,
        JobStatus::Running,
        JobStatus::RetryWait,
        JobStatus::PauseRequested,
        JobStatus::Paused,
    ];

    pub fn is_active(self) -> bool {
        Self::ACTIVE.contains(&self)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStatus::Succeeded | JobStatus::DeadLetter | JobStatus::Cancelled
        )
    }
}

/// `{job_type}:{tenant_or_store_id}:{resource_id}:{operation_version}` (§14.3).
///
/// The key is the identity of a *logical* job, not of one attempt. Two API instances that
/// both decide a campaign should be issued produce the same key and therefore the same
/// job (JOB-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobKey {
    pub job_type: JobType,
    pub tenant: String,
    pub resource: String,
    pub operation_version: String,
}

/// The tenant slot for work that belongs to no single store (§14.3's `expire_coupons`).
pub const GLOBAL_TENANT: &str = "global";

impl JobKey {
    pub fn new(
        job_type: JobType,
        tenant: impl Into<String>,
        resource: impl Into<String>,
        operation_version: impl Into<String>,
    ) -> Self {
        Self {
            job_type,
            tenant: tenant.into(),
            resource: resource.into(),
            operation_version: operation_version.into(),
        }
    }

    /// `build_campaign_audience:store-uuid:campaign-uuid:3`.
    pub fn build_audience(store_id: Uuid, campaign_id: Uuid, generation: i32) -> Self {
        Self::new(
            JobType::BuildCampaignAudience,
            store_id.to_string(),
            campaign_id.to_string(),
            generation.to_string(),
        )
    }

    /// `issue_campaign:store-uuid:campaign-uuid:3` — §14.3's own example.
    pub fn issue_campaign(store_id: Uuid, campaign_id: Uuid, generation: i32) -> Self {
        Self::new(
            JobType::IssueCampaign,
            store_id.to_string(),
            campaign_id.to_string(),
            generation.to_string(),
        )
    }

    /// `revoke_campaign:store-uuid:campaign-uuid:case-uuid` — the operation version is the
    /// case, so two revocations authorised by two different cases are two jobs (§14.3).
    pub fn revoke_campaign(store_id: Uuid, campaign_id: Uuid, operation: &str) -> Self {
        Self::new(
            JobType::RevokeCampaign,
            store_id.to_string(),
            campaign_id.to_string(),
            operation,
        )
    }

    /// `expire_coupons:global:2026-08-10T06:00Z:v1` — one job per hour shard, so a late
    /// worker catches up shard by shard instead of one unbounded sweep (§14.6 시간 shard).
    pub fn expire_coupons(shard: DateTime<Utc>) -> Self {
        Self::new(
            JobType::ExpireCoupons,
            GLOBAL_TENANT,
            shard.format("%Y-%m-%dT%H:00Z").to_string(),
            "v1",
        )
    }

    pub fn execute_adjustment(case_id: Uuid, adjustment_id: Uuid, generation: i32) -> Self {
        Self::new(
            JobType::ExecuteAdjustment,
            case_id.to_string(),
            adjustment_id.to_string(),
            generation.to_string(),
        )
    }

    /// A stable 64-bit hash for `pg_try_advisory_lock` (§14.5-5).
    pub fn advisory_lock_key(&self) -> i64 {
        advisory_lock_key_for(&self.to_string())
    }
}

/// The lock a `unique_key` maps to (§14.5-5).
///
/// Stable is the operative word: the same key must map to the same lock in every process
/// and every release, which rules out `DefaultHasher` (explicitly not stable across
/// builds). SHA-256's first eight bytes cost nothing here and never move — which also
/// means a worker can derive the lock from the *stored* key without reconstructing the
/// `JobKey` parts.
pub fn advisory_lock_key_for(unique_key: &str) -> i64 {
    let digest = Sha256::digest(unique_key.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

impl fmt::Display for JobKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}:{}",
            self.job_type.as_db(),
            self.tenant,
            self.resource,
            self.operation_version
        )
    }
}

/// What an enqueue request asks for.
#[derive(Debug, Clone)]
pub struct JobSpec {
    pub key: JobKey,
    pub payload: serde_json::Value,
    pub store_id: Option<Uuid>,
    pub resource_id: Option<Uuid>,
    /// When the work becomes eligible. `None` means immediately.
    pub scheduled_at: Option<DateTime<Utc>>,
    pub requested_by_user_id: Option<Uuid>,
}

impl JobSpec {
    pub fn new(key: JobKey, payload: serde_json::Value) -> Self {
        Self {
            key,
            payload,
            store_id: None,
            resource_id: None,
            scheduled_at: None,
            requested_by_user_id: None,
        }
    }

    pub fn store(mut self, store_id: Uuid) -> Self {
        self.store_id = Some(store_id);
        self
    }

    pub fn resource(mut self, resource_id: Uuid) -> Self {
        self.resource_id = Some(resource_id);
        self
    }

    pub fn requested_by(mut self, user_id: Uuid) -> Self {
        self.requested_by_user_id = Some(user_id);
        self
    }

    pub fn at(mut self, scheduled_at: DateTime<Utc>) -> Self {
        self.scheduled_at = Some(scheduled_at);
        self
    }
}

/// The outcome of registering a job (§14.5 steps 1–2, JOB-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct EnqueuedJob {
    pub job_id: Uuid,
    pub status: JobStatus,
    pub generation: i32,
    /// True when this call found an existing job rather than creating one. The caller
    /// must not treat that as an error: JOB-001 says the second registration returns the
    /// first job's id.
    pub deduplicated: bool,
}

/// A job as the admin queue view lists it (§11.5 `GET /admin/jobs`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobSummary {
    pub id: Uuid,
    pub unique_key: String,
    pub job_type: String,
    pub status: JobStatus,
    pub generation: i32,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub processed_count: i64,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub store_id: Option<Uuid>,
    pub resource_id: Option<Uuid>,
    pub scheduled_at: DateTime<Utc>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One try, with why it ended (§14.7).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobAttempt {
    pub id: Uuid,
    pub attempt_no: i32,
    pub generation: i32,
    pub worker_id: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub processed_count: i64,
    pub succeeded_count: i64,
    pub failed_count: i64,
    pub checkpoint: serde_json::Value,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub retryable: Option<bool>,
    pub next_attempt_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct JobDetail {
    #[serde(flatten)]
    pub job: JobSummary,
    pub payload: serde_json::Value,
    pub checkpoint: serde_json::Value,
    pub attempts: Vec<JobAttempt>,
    pub retry_of_job_id: Option<Uuid>,
    pub retry_reason: Option<String>,
}

/// Progress a handler reports at a checkpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobProgress {
    pub processed: i64,
    pub succeeded: i64,
    pub failed: i64,
}

impl JobProgress {
    pub fn add(&mut self, other: JobProgress) {
        self.processed += other.processed;
        self.succeeded += other.succeeded;
        self.failed += other.failed;
    }
}

/// What a heartbeat tells the handler to do next.
///
/// This is how CAMPAIGN-006's "진행 중 대량 발급 워커는 배치 사이에서 상태를 확인하고
/// 안전하게 멈춘다" is actually implemented: the handler asks between batches, and stops
/// on a boundary it chose rather than being killed mid-write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobControl {
    Continue,
    Pause,
    Cancel,
}

/// A held `pg_advisory_lock`, and the connection that holds it.
///
/// The connection is the point. §14.5-9 wants a worker crash to release the lock without
/// anyone cleaning up, and a session-scoped advisory lock does exactly that: PostgreSQL
/// drops it when the backend goes away. Nothing here has a TTL that could expire while
/// the work is still running.
pub struct AdvisoryLock {
    key: i64,
    connection: Option<PoolConnection<sqlx::Postgres>>,
}

impl fmt::Debug for AdvisoryLock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdvisoryLock")
            .field("key", &self.key)
            .field("held", &self.connection.is_some())
            .finish()
    }
}

impl AdvisoryLock {
    /// Try to take the lock. `Ok(None)` means another worker holds it, which §14.5-6 says
    /// is not a failure — the caller re-queues without spending an attempt.
    pub async fn try_acquire(pool: &PgPool, key: i64) -> ApiResult<Option<Self>> {
        let mut connection = pool.acquire().await?;

        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *connection)
            .await?;

        if acquired {
            Ok(Some(Self {
                key,
                connection: Some(connection),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn key(&self) -> i64 {
        self.key
    }

    /// Release explicitly, returning the connection to the pool *unlocked*.
    ///
    /// Dropping without this also releases the lock — but only once the connection is
    /// recycled or closed, and a pooled connection handed to the next caller while still
    /// holding a lock would be a slow, baffling bug. So the happy path unlocks by hand.
    pub async fn release(mut self) {
        if let Some(mut connection) = self.connection.take() {
            if let Err(error) = sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(self.key)
                .execute(&mut *connection)
                .await
            {
                // The connection is dropped below either way, which releases the lock.
                tracing::warn!(%error, key = self.key, "could not release the advisory lock cleanly");
                connection.close_on_drop();
            }
        }
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        // Reaching here means `release` was not called — a panic, or an early return on
        // an error path. The lock must not travel with a pooled connection, so the
        // connection is closed instead of being reused.
        if let Some(mut connection) = self.connection.take() {
            connection.close_on_drop();
        }
    }
}

/// A job this worker has exclusive right to run.
#[derive(Debug)]
pub struct ClaimedJob {
    pub job_id: Uuid,
    pub job_type: JobType,
    pub unique_key: String,
    pub generation: i32,
    pub attempt_no: i32,
    pub attempt_id: Uuid,
    pub payload: serde_json::Value,
    pub checkpoint: serde_json::Value,
    pub store_id: Option<Uuid>,
    pub resource_id: Option<Uuid>,
    /// Held for as long as this value lives.
    pub lock: AdvisoryLock,
}

/// Why an attempt ended badly.
#[derive(Debug, Clone)]
pub struct JobFailure {
    pub code: String,
    pub message: String,
    pub class: RetryClass,
}

impl JobFailure {
    pub fn permanent(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            class: RetryClass::Permanent,
        }
    }

    pub fn transient(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            class: RetryClass::Transient,
        }
    }

    pub fn from_api(error: &ApiError) -> Self {
        Self {
            code: error.code.as_str().to_owned(),
            message: error
                .internal
                .clone()
                .unwrap_or_else(|| error.message.clone()),
            class: classify_api_error(error),
        }
    }
}

/// Filters for `GET /admin/jobs`.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct JobQuery {
    pub job_type: Option<String>,
    pub status: Option<JobStatus>,
    pub store_id: Option<Uuid>,
    pub resource_id: Option<Uuid>,
}

pub struct JobService;

impl Default for JobService {
    fn default() -> Self {
        Self::new()
    }
}

impl JobService {
    pub fn new() -> Self {
        Self
    }

    /// §14.5 steps 1–3: register the job, or hand back the one that already exists.
    ///
    /// Called inside the caller's transaction on purpose. §14.2 wants the outbox row and
    /// the domain change to commit together, so that a crash between "the campaign is
    /// published" and "the issuing job exists" is impossible.
    pub async fn enqueue(&self, tx: &mut Tx<'_>, spec: &JobSpec) -> ApiResult<EnqueuedJob> {
        self.enqueue_generation(tx, spec, 1, None, None).await
    }

    /// The same, at an explicit generation. §14.7 requires a *new* generation for a
    /// dead-letter reprocess, so the original attempt history stays attached to the
    /// original job rather than being appended to.
    pub async fn enqueue_generation(
        &self,
        tx: &mut Tx<'_>,
        spec: &JobSpec,
        generation: i32,
        retry_of_job_id: Option<Uuid>,
        retry_reason: Option<&str>,
    ) -> ApiResult<EnqueuedJob> {
        let unique_key = spec.key.to_string();

        // Step 2, ahead of the insert. An active job wins outright; a *succeeded* job at
        // the same generation also wins, because JOB-001 says re-registering a version
        // that already ran returns its result instead of running it again.
        if let Some(existing) = self.find_reusable(&mut **tx, &unique_key, generation).await? {
            return Ok(existing);
        }

        let budget = spec.key.job_type.retry_budget();
        let scheduled_at = spec.scheduled_at.unwrap_or_else(Utc::now);

        let inserted = sqlx::query!(
            r#"
            INSERT INTO coupon.job_registry
                (unique_key, job_type, generation, status, payload, max_attempts,
                 scheduled_at, lock_key, store_id, resource_id, visibility_timeout_secs,
                 requested_by_user_id, retry_of_job_id, retry_reason)
            VALUES ($1, $2, $3, 'PENDING_OUTBOX', $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING id
            "#,
            unique_key,
            spec.key.job_type.as_db(),
            generation,
            spec.payload,
            budget.recorded_max_attempts(),
            scheduled_at,
            spec.key.advisory_lock_key(),
            spec.store_id,
            spec.resource_id,
            spec.key.job_type.visibility_timeout_secs(),
            spec.requested_by_user_id,
            retry_of_job_id,
            retry_reason,
        )
        .fetch_optional(&mut **tx)
        .await;

        let job_id = match inserted {
            Ok(Some(row)) => row.id,
            Ok(None) => {
                return Err(ApiError::new(ErrorCode::ServiceUnavailable)
                    .internal("job insert returned no row"));
            }
            // Either the active-key index or `(unique_key, generation)` fired, which both
            // mean the same thing: somebody else got there first between our lookup and
            // our insert. That is the *expected* outcome under contention (JOB-001), so
            // it resolves to the winning job rather than to an error.
            Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
                return self
                    .find_reusable(&mut **tx, &unique_key, generation)
                    .await?
                    .ok_or_else(|| {
                        ApiError::new(ErrorCode::Conflict)
                            .internal(format!("job {unique_key} conflicted but could not be found"))
                    });
            }
            Err(error) => return Err(ApiError::from(error)),
        };

        // Step 3's precondition: the relay publishes from here, so a Redis outage delays
        // the job rather than losing it (JOB-005).
        sqlx::query!(
            r#"
            INSERT INTO coupon.outbox_events
                (aggregate_type, aggregate_id, aggregate_version, event_type, correlation_id, payload)
            VALUES ('job', $1, $2, 'JOB_ENQUEUED', $3, $4)
            ON CONFLICT (aggregate_type, aggregate_id, aggregate_version, event_type) DO NOTHING
            "#,
            job_id,
            i64::from(generation),
            Uuid::new_v4(),
            serde_json::json!({
                "job_id": job_id,
                "job_type": spec.key.job_type.as_db(),
                "unique_key": unique_key,
                "scheduled_at": scheduled_at,
            }),
        )
        .execute(&mut **tx)
        .await?;

        Ok(EnqueuedJob {
            job_id,
            status: JobStatus::PendingOutbox,
            generation,
            deduplicated: false,
        })
    }

    /// An active job with this key, or a succeeded one at this generation (JOB-001).
    async fn find_reusable<'e, E>(
        &self,
        executor: E,
        unique_key: &str,
        generation: i32,
    ) -> ApiResult<Option<EnqueuedJob>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, status::text AS "status!", generation
            FROM coupon.job_registry
            WHERE unique_key = $1
              AND (
                    status IN ('PENDING_OUTBOX', 'QUEUED', 'RUNNING', 'RETRY_WAIT',
                               'PAUSE_REQUESTED', 'PAUSED')
                 OR (status = 'SUCCEEDED' AND generation = $2)
              )
            ORDER BY generation DESC
            LIMIT 1
            "#,
            unique_key,
            generation,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|row| EnqueuedJob {
            job_id: row.id,
            status: JobStatus::from_db(&row.status),
            generation: row.generation,
            deduplicated: true,
        }))
    }

    /// §14.5-3: publish `job_id` — and only `job_id` — to the transport.
    ///
    /// The message deliberately carries no state. Whatever a worker receives, it re-reads
    /// the registry (step 4), so a duplicated or stale message cannot make it act on an
    /// out-of-date view of the job.
    pub async fn relay_outbox(
        &self,
        pool: &PgPool,
        transport: &dyn transport::JobTransport,
        batch: i64,
        now: DateTime<Utc>,
    ) -> ApiResult<u64> {
        let pending = sqlx::query!(
            r#"
            SELECT id, aggregate_id
            FROM coupon.outbox_events
            WHERE event_type IN ('JOB_ENQUEUED', 'JOB_RESUMED')
              AND status IN ('PENDING', 'FAILED')
              AND available_at <= $1
            ORDER BY created_at
            LIMIT $2
            FOR UPDATE SKIP LOCKED
            "#,
            now,
            batch,
        )
        .fetch_all(pool)
        .await?;

        let mut published = 0u64;
        for event in pending {
            match transport.publish(event.aggregate_id).await {
                Ok(()) => {
                    let mut tx = pool.begin().await?;
                    sqlx::query!(
                        r#"
                        UPDATE coupon.outbox_events
                        SET status = 'PUBLISHED', published_at = $2, attempt_count = attempt_count + 1
                        WHERE id = $1
                        "#,
                        event.id,
                        now,
                    )
                    .execute(&mut *tx)
                    .await?;

                    sqlx::query!(
                        r#"
                        UPDATE coupon.job_registry
                        SET status = 'QUEUED'
                        WHERE id = $1 AND status = 'PENDING_OUTBOX'
                        "#,
                        event.aggregate_id,
                    )
                    .execute(&mut *tx)
                    .await?;

                    tx.commit().await?;
                    published += 1;
                }
                Err(error) => {
                    // JOB-005: the row stays, backs off, and is retried. Nothing is lost
                    // and no user was told the work had started.
                    sqlx::query!(
                        r#"
                        UPDATE coupon.outbox_events
                        SET status = 'FAILED',
                            attempt_count = attempt_count + 1,
                            available_at = $2,
                            last_error = $3
                        WHERE id = $1
                        "#,
                        event.id,
                        now + chrono::Duration::seconds(15),
                        error.to_string(),
                    )
                    .execute(pool)
                    .await?;
                    tracing::warn!(%error, job_id = %event.aggregate_id, "job enqueue relay failed");
                }
            }
        }

        Ok(published)
    }

    /// §14.5 steps 4–7: verify, lock, and take the job.
    ///
    /// Returns `Ok(None)` for every reason a worker should quietly move on — the job is
    /// no longer runnable, or another worker holds the lock. Neither is an error and
    /// neither spends an attempt (§14.5-6).
    pub async fn claim(
        &self,
        pool: &PgPool,
        job_id: Uuid,
        worker_id: &str,
        now: DateTime<Utc>,
    ) -> ApiResult<Option<ClaimedJob>> {
        // Step 4: the message said only "look at this job"; the registry says what it is.
        let job = sqlx::query!(
            r#"
            SELECT id, unique_key, job_type, generation, status::text AS "status!",
                   payload, checkpoint, attempt_count, lock_key, store_id, resource_id,
                   scheduled_at, next_attempt_at
            FROM coupon.job_registry
            WHERE id = $1
            "#,
            job_id,
        )
        .fetch_optional(pool)
        .await?;

        let Some(job) = job else {
            return Ok(None);
        };

        let status = JobStatus::from_db(&job.status);
        if !matches!(status, JobStatus::Queued | JobStatus::RetryWait) {
            return Ok(None);
        }

        let due_at = job.next_attempt_at.unwrap_or(job.scheduled_at);
        if due_at > now {
            return Ok(None);
        }

        let Some(job_type) = JobType::from_db(&job.job_type) else {
            // A job type this build does not implement is not ours to run — a rolling
            // deploy is the ordinary way to see one.
            tracing::warn!(job_type = job.job_type, %job_id, "unknown job type; leaving it queued");
            return Ok(None);
        };

        // Step 5. `lock_key` is stored for operators to read, but the lock actually taken
        // is derived from the key here, so a hand-edited column cannot point two
        // different jobs at one lock.
        let lock_key = advisory_lock_key_for(&job.unique_key);
        let Some(lock) = AdvisoryLock::try_acquire(pool, lock_key).await? else {
            // Step 6: not a failure. Record it as a contended attempt for visibility and
            // let the caller re-queue with jitter.
            tracing::debug!(%job_id, "another worker holds the advisory lock");
            return Ok(None);
        };

        // Re-read under the lock: between the read above and the lock, another worker may
        // have finished the whole job.
        let mut tx = pool.begin().await?;
        let current = sqlx::query!(
            r#"
            SELECT status::text AS "status!", attempt_count, generation, checkpoint
            FROM coupon.job_registry
            WHERE id = $1
            FOR UPDATE
            "#,
            job_id,
        )
        .fetch_one(&mut *tx)
        .await?;

        if !matches!(
            JobStatus::from_db(&current.status),
            JobStatus::Queued | JobStatus::RetryWait
        ) {
            drop(tx);
            lock.release().await;
            return Ok(None);
        }

        let attempt_no = current.attempt_count + 1;

        sqlx::query!(
            r#"
            UPDATE coupon.job_registry
            SET status = 'RUNNING',
                attempt_count = $2,
                started_at = COALESCE(started_at, $3),
                heartbeat_at = $3,
                locked_by = $4,
                next_attempt_at = NULL
            WHERE id = $1
            "#,
            job_id,
            attempt_no,
            now,
            worker_id,
        )
        .execute(&mut *tx)
        .await?;

        let attempt_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.job_attempts
                (job_id, attempt_no, generation, worker_id, status, started_at, checkpoint)
            VALUES ($1, $2, $3, $4, 'RUNNING', $5, $6)
            RETURNING id
            "#,
            job_id,
            attempt_no,
            current.generation,
            worker_id,
            now,
            current.checkpoint,
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Some(ClaimedJob {
            job_id,
            job_type,
            unique_key: job.unique_key,
            generation: current.generation,
            attempt_no,
            attempt_id,
            payload: job.payload,
            checkpoint: job.checkpoint,
            store_id: job.store_id,
            resource_id: job.resource_id,
            lock,
        }))
    }

    /// §14.5-7: heartbeat, checkpoint and counters, and the answer to "should I keep going".
    pub async fn checkpoint(
        &self,
        pool: &PgPool,
        job: &ClaimedJob,
        checkpoint: &serde_json::Value,
        progress: JobProgress,
        now: DateTime<Utc>,
    ) -> ApiResult<JobControl> {
        let status = sqlx::query_scalar!(
            r#"
            UPDATE coupon.job_registry
            SET heartbeat_at = $2,
                checkpoint = $3,
                processed_count = $4,
                succeeded_count = $5,
                failed_count = $6
            WHERE id = $1
            RETURNING status::text AS "status!"
            "#,
            job.job_id,
            now,
            checkpoint,
            progress.processed,
            progress.succeeded,
            progress.failed,
        )
        .fetch_one(pool)
        .await?;

        sqlx::query!(
            r#"
            UPDATE coupon.job_attempts
            SET checkpoint = $2, processed_count = $3, succeeded_count = $4, failed_count = $5
            WHERE id = $1
            "#,
            job.attempt_id,
            checkpoint,
            progress.processed,
            progress.succeeded,
            progress.failed,
        )
        .execute(pool)
        .await?;

        Ok(match JobStatus::from_db(&status) {
            JobStatus::PauseRequested => JobControl::Pause,
            JobStatus::Cancelled => JobControl::Cancel,
            _ => JobControl::Continue,
        })
    }

    /// §14.5-8, the happy half.
    pub async fn succeed(
        &self,
        pool: &PgPool,
        job: &ClaimedJob,
        checkpoint: &serde_json::Value,
        progress: JobProgress,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        let mut tx = pool.begin().await?;

        sqlx::query!(
            r#"
            UPDATE coupon.job_registry
            SET status = 'SUCCEEDED',
                finished_at = $2,
                heartbeat_at = $2,
                checkpoint = $3,
                processed_count = $4,
                succeeded_count = $5,
                failed_count = $6,
                locked_by = NULL,
                next_attempt_at = NULL
            WHERE id = $1
            "#,
            job.job_id,
            now,
            checkpoint,
            progress.processed,
            progress.succeeded,
            progress.failed,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE coupon.job_attempts
            SET status = 'SUCCEEDED', finished_at = $2, checkpoint = $3,
                processed_count = $4, succeeded_count = $5, failed_count = $6
            WHERE id = $1
            "#,
            job.attempt_id,
            now,
            checkpoint,
            progress.processed,
            progress.succeeded,
            progress.failed,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// §14.5-8, the unhappy half, and §14.7's retry policy.
    ///
    /// Returns the status the job landed in, so the caller can log a dead-letter as the
    /// alertable event §14.7 says it is.
    pub async fn fail(
        &self,
        pool: &PgPool,
        job: &ClaimedJob,
        failure: &JobFailure,
        checkpoint: &serde_json::Value,
        progress: JobProgress,
        now: DateTime<Utc>,
    ) -> ApiResult<JobStatus> {
        let budget = job.job_type.retry_budget();
        let decision = failure.class.decide(job.attempt_no, budget);

        let (status, next_attempt_at) = match decision {
            RetryDecision::Retry { after } => (
                JobStatus::RetryWait,
                Some(now + chrono::Duration::from_std(after).unwrap_or_default()),
            ),
            RetryDecision::DeadLetter => (JobStatus::DeadLetter, None),
        };

        let mut tx = pool.begin().await?;

        sqlx::query!(
            r#"
            UPDATE coupon.job_registry
            SET status = $2::text::coupon.job_status,
                next_attempt_at = $3,
                finished_at = CASE WHEN $2 = 'DEAD_LETTER' THEN $4 ELSE NULL::timestamptz END,
                dead_lettered_at = CASE WHEN $2 = 'DEAD_LETTER' THEN $4 ELSE dead_lettered_at END,
                heartbeat_at = $4,
                checkpoint = $5,
                processed_count = $6,
                succeeded_count = $7,
                failed_count = $8,
                last_error_code = $9,
                last_error_message = $10,
                locked_by = NULL
            WHERE id = $1
            "#,
            job.job_id,
            status.as_db(),
            next_attempt_at,
            now,
            checkpoint,
            progress.processed,
            progress.succeeded,
            progress.failed,
            failure.code,
            failure.message,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE coupon.job_attempts
            SET status = 'FAILED', finished_at = $2, checkpoint = $3,
                processed_count = $4, succeeded_count = $5, failed_count = $6,
                error_code = $7, error_message = $8, retryable = $9, next_attempt_at = $10
            WHERE id = $1
            "#,
            job.attempt_id,
            now,
            checkpoint,
            progress.processed,
            progress.succeeded,
            progress.failed,
            failure.code,
            failure.message,
            status == JobStatus::RetryWait,
            next_attempt_at,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        if status == JobStatus::DeadLetter {
            // §14.7: dead-letter 진입은 첫 건부터 알린다.
            tracing::error!(
                job_id = %job.job_id,
                unique_key = job.unique_key,
                error.code = failure.code,
                error.message = failure.message,
                attempts = job.attempt_no,
                "jobs.dead_letter"
            );
        }

        Ok(status)
    }

    /// Park a running or queued job (§14.4 운영 중지).
    pub async fn request_pause(&self, pool: &PgPool, job_id: Uuid) -> ApiResult<JobStatus> {
        let status = sqlx::query_scalar!(
            r#"
            UPDATE coupon.job_registry
            SET status = CASE
                    WHEN status = 'RUNNING' THEN 'PAUSE_REQUESTED'::coupon.job_status
                    ELSE 'PAUSED'::coupon.job_status
                END,
                pause_requested_at = clock_timestamp(),
                paused_at = CASE WHEN status <> 'RUNNING' THEN clock_timestamp() ELSE NULL END
            WHERE id = $1
              AND status IN ('PENDING_OUTBOX', 'QUEUED', 'RUNNING', 'RETRY_WAIT')
            RETURNING status::text AS "status!"
            "#,
            job_id,
        )
        .fetch_optional(pool)
        .await?;

        status.map(|raw| JobStatus::from_db(&raw)).ok_or_else(|| {
            ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "일시 중지할 수 있는 작업이 아닙니다.",
            )
        })
    }

    /// Mark a paused job as parked, once the handler has actually stopped.
    pub async fn confirm_paused(&self, pool: &PgPool, job_id: Uuid) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.job_registry
            SET status = 'PAUSED', paused_at = clock_timestamp(), locked_by = NULL
            WHERE id = $1 AND status = 'PAUSE_REQUESTED'
            "#,
            job_id,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// CAMPAIGN-006: 재개하면 같은 작업 키와 체크포인트로 미처리 대상부터 계속한다.
    pub async fn resume(&self, tx: &mut Tx<'_>, job_id: Uuid) -> ApiResult<()> {
        let changed = sqlx::query!(
            r#"
            UPDATE coupon.job_registry
            SET status = 'PENDING_OUTBOX', pause_requested_at = NULL, paused_at = NULL,
                next_attempt_at = NULL
            WHERE id = $1 AND status IN ('PAUSED', 'PAUSE_REQUESTED')
            RETURNING generation
            "#,
            job_id,
        )
        .fetch_optional(&mut **tx)
        .await?;

        let Some(row) = changed else {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "재개할 수 있는 작업이 아닙니다.",
            ));
        };

        // The checkpoint is deliberately untouched, so the handler picks up where it
        // stopped rather than starting the campaign over.
        sqlx::query!(
            r#"
            INSERT INTO coupon.outbox_events
                (aggregate_type, aggregate_id, aggregate_version, event_type, correlation_id, payload)
            VALUES ('job', $1, $2, 'JOB_RESUMED', $3, $4)
            ON CONFLICT (aggregate_type, aggregate_id, aggregate_version, event_type) DO NOTHING
            "#,
            job_id,
            i64::from(row.generation),
            Uuid::new_v4(),
            serde_json::json!({ "job_id": job_id }),
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// `POST /admin/jobs/:id/retry` (§11.5, §14.7).
    ///
    /// A dead-letter is not simply re-queued: §14.7 requires an administrator's reason and
    /// a **new generation**, so the failed run keeps its own history and the reprocess is
    /// a distinct, attributable act.
    pub async fn reprocess(
        &self,
        pool: &PgPool,
        job_id: Uuid,
        admin_user_id: Uuid,
        reason: &str,
    ) -> ApiResult<EnqueuedJob> {
        if reason.trim().is_empty() {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "재처리 사유를 입력해야 합니다.",
            ));
        }

        let mut tx = pool.begin().await?;

        let original = sqlx::query!(
            r#"
            SELECT id, unique_key, job_type, generation, status::text AS "status!",
                   payload, store_id, resource_id
            FROM coupon.job_registry
            WHERE id = $1
            FOR UPDATE
            "#,
            job_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::with_message(ErrorCode::NotFound, "작업을 찾을 수 없습니다."))?;

        let status = JobStatus::from_db(&original.status);
        if !matches!(status, JobStatus::DeadLetter | JobStatus::Cancelled) {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "실패 큐에 있는 작업만 재처리할 수 있습니다.",
            ));
        }

        let Some(job_type) = JobType::from_db(&original.job_type) else {
            return Err(ApiError::new(ErrorCode::UnprocessableRequest)
                .internal(format!("unknown job type {}", original.job_type)));
        };

        // The unique key is preserved verbatim — the *logical* job is the same one, and
        // JOB-001 requires the target-level uniqueness constraints to keep applying.
        let parts: Vec<&str> = original.unique_key.splitn(4, ':').collect();
        let key = if parts.len() == 4 {
            JobKey::new(job_type, parts[1], parts[2], parts[3])
        } else {
            return Err(ApiError::new(ErrorCode::UnprocessableRequest)
                .internal(format!("malformed job key {}", original.unique_key)));
        };

        let mut spec = JobSpec::new(key, original.payload).requested_by(admin_user_id);
        spec.store_id = original.store_id;
        spec.resource_id = original.resource_id;

        let enqueued = self
            .enqueue_generation(
                &mut tx,
                &spec,
                original.generation + 1,
                Some(original.id),
                Some(reason),
            )
            .await?;

        tx.commit().await?;

        tracing::info!(
            job_id = %enqueued.job_id,
            retry_of = %original.id,
            generation = enqueued.generation,
            "jobs.reprocess_requested"
        );

        Ok(enqueued)
    }

    /// Jobs the worker may run right now. The DB-polling path, used both as the JOB-005
    /// fallback and as the recovery route for a message that was never delivered.
    pub async fn due_jobs(
        &self,
        pool: &PgPool,
        now: DateTime<Utc>,
        batch: i64,
    ) -> ApiResult<Vec<Uuid>> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT id
            FROM coupon.job_registry
            WHERE status IN ('QUEUED', 'RETRY_WAIT')
              AND COALESCE(next_attempt_at, scheduled_at) <= $1
            ORDER BY COALESCE(next_attempt_at, scheduled_at)
            LIMIT $2
            "#,
            now,
            batch,
        )
        .fetch_all(pool)
        .await?)
    }

    /// Put a job whose worker went silent back on the queue (§14.5-9).
    ///
    /// Safe to run from every worker: the advisory lock is what actually prevents two
    /// runners, so the worst case of a premature reclaim is one `claim` that fails to
    /// take the lock and moves on.
    pub async fn reclaim_stalled(&self, pool: &PgPool, now: DateTime<Utc>) -> ApiResult<u64> {
        let result = sqlx::query!(
            r#"
            UPDATE coupon.job_registry
            SET status = 'QUEUED', locked_by = NULL, next_attempt_at = $1
            WHERE status = 'RUNNING'
              AND heartbeat_at IS NOT NULL
              AND heartbeat_at < $1::timestamptz - make_interval(secs => visibility_timeout_secs)
            "#,
            now,
        )
        .execute(pool)
        .await?;

        // The attempt row is left as `RUNNING` deliberately: it records that a worker
        // started and never came back, which is exactly what an operator needs to see.
        sqlx::query!(
            r#"
            UPDATE coupon.job_attempts a
            SET status = 'ABANDONED', finished_at = $1
            WHERE a.status = 'RUNNING'
              AND a.started_at < $1::timestamptz - interval '1 hour'
            "#,
            now,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// `GET /admin/jobs` (§11.5).
    pub async fn list(
        &self,
        pool: &PgPool,
        query: &JobQuery,
        limit: i64,
    ) -> ApiResult<Vec<JobSummary>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, unique_key, job_type, status::text AS "status!", generation,
                   attempt_count, max_attempts, processed_count, succeeded_count,
                   failed_count, store_id, resource_id, scheduled_at, next_attempt_at,
                   started_at, heartbeat_at, finished_at, last_error_code,
                   last_error_message, created_at
            FROM coupon.job_registry
            WHERE ($1::text IS NULL OR job_type = $1)
              AND ($2::text IS NULL OR status::text = $2)
              AND ($3::uuid IS NULL OR store_id = $3)
              AND ($4::uuid IS NULL OR resource_id = $4)
            ORDER BY created_at DESC
            LIMIT $5
            "#,
            query.job_type.as_deref(),
            query.status.map(JobStatus::as_db),
            query.store_id,
            query.resource_id,
            limit,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| JobSummary {
                id: row.id,
                unique_key: row.unique_key,
                job_type: row.job_type,
                status: JobStatus::from_db(&row.status),
                generation: row.generation,
                attempt_count: row.attempt_count,
                max_attempts: row.max_attempts,
                processed_count: row.processed_count,
                succeeded_count: row.succeeded_count,
                failed_count: row.failed_count,
                store_id: row.store_id,
                resource_id: row.resource_id,
                scheduled_at: row.scheduled_at,
                next_attempt_at: row.next_attempt_at,
                started_at: row.started_at,
                heartbeat_at: row.heartbeat_at,
                finished_at: row.finished_at,
                last_error_code: row.last_error_code,
                last_error_message: row.last_error_message,
                created_at: row.created_at,
            })
            .collect())
    }

    /// One job with every attempt it made (§11.5 작업·시도·체크포인트).
    pub async fn detail(&self, pool: &PgPool, job_id: Uuid) -> ApiResult<JobDetail> {
        let row = sqlx::query!(
            r#"
            SELECT id, unique_key, job_type, status::text AS "status!", generation,
                   attempt_count, max_attempts, processed_count, succeeded_count,
                   failed_count, store_id, resource_id, scheduled_at, next_attempt_at,
                   started_at, heartbeat_at, finished_at, last_error_code,
                   last_error_message, created_at, payload, checkpoint, retry_of_job_id,
                   retry_reason
            FROM coupon.job_registry
            WHERE id = $1
            "#,
            job_id,
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::with_message(ErrorCode::NotFound, "작업을 찾을 수 없습니다."))?;

        let attempts = sqlx::query!(
            r#"
            SELECT id, attempt_no, generation, worker_id, status, started_at, finished_at,
                   processed_count, succeeded_count, failed_count, checkpoint, error_code,
                   error_message, retryable, next_attempt_at
            FROM coupon.job_attempts
            WHERE job_id = $1
            ORDER BY generation, attempt_no
            "#,
            job_id,
        )
        .fetch_all(pool)
        .await?;

        Ok(JobDetail {
            job: JobSummary {
                id: row.id,
                unique_key: row.unique_key,
                job_type: row.job_type,
                status: JobStatus::from_db(&row.status),
                generation: row.generation,
                attempt_count: row.attempt_count,
                max_attempts: row.max_attempts,
                processed_count: row.processed_count,
                succeeded_count: row.succeeded_count,
                failed_count: row.failed_count,
                store_id: row.store_id,
                resource_id: row.resource_id,
                scheduled_at: row.scheduled_at,
                next_attempt_at: row.next_attempt_at,
                started_at: row.started_at,
                heartbeat_at: row.heartbeat_at,
                finished_at: row.finished_at,
                last_error_code: row.last_error_code,
                last_error_message: row.last_error_message,
                created_at: row.created_at,
            },
            payload: row.payload,
            checkpoint: row.checkpoint,
            retry_of_job_id: row.retry_of_job_id,
            retry_reason: row.retry_reason,
            attempts: attempts
                .into_iter()
                .map(|attempt| JobAttempt {
                    id: attempt.id,
                    attempt_no: attempt.attempt_no,
                    generation: attempt.generation,
                    worker_id: attempt.worker_id,
                    status: attempt.status,
                    started_at: attempt.started_at,
                    finished_at: attempt.finished_at,
                    processed_count: attempt.processed_count,
                    succeeded_count: attempt.succeeded_count,
                    failed_count: attempt.failed_count,
                    checkpoint: attempt.checkpoint,
                    error_code: attempt.error_code,
                    error_message: attempt.error_message,
                    retryable: attempt.retryable,
                    next_attempt_at: attempt.next_attempt_at,
                })
                .collect(),
        })
    }
}

/// This process's identity in `job_registry.locked_by` and `job_attempts.worker_id`.
pub fn worker_identity() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned());
    format!("{host}/{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_key_renders_the_documented_shape() {
        // §14.3's own examples, character for character.
        let store = Uuid::from_u128(1);
        let campaign = Uuid::from_u128(2);

        assert_eq!(
            JobKey::issue_campaign(store, campaign, 3).to_string(),
            format!("issue_campaign:{store}:{campaign}:3")
        );

        let shard = "2026-08-10T06:30:00Z"
            .parse::<DateTime<Utc>>()
            .expect("timestamp");
        assert_eq!(
            JobKey::expire_coupons(shard).to_string(),
            "expire_coupons:global:2026-08-10T06:00Z:v1",
            "the shard is the hour, so everything inside one hour is one job"
        );

        let case = Uuid::from_u128(3);
        assert_eq!(
            JobKey::revoke_campaign(store, campaign, &case.to_string()).to_string(),
            format!("revoke_campaign:{store}:{campaign}:{case}")
        );
    }

    #[test]
    fn the_operation_version_makes_a_new_run_a_new_job() {
        let store = Uuid::from_u128(1);
        let campaign = Uuid::from_u128(2);

        assert_ne!(
            JobKey::issue_campaign(store, campaign, 1).to_string(),
            JobKey::issue_campaign(store, campaign, 2).to_string(),
        );
    }

    #[test]
    fn the_advisory_lock_key_is_stable_and_key_specific() {
        let store = Uuid::from_u128(1);
        let campaign = Uuid::from_u128(2);
        let key = JobKey::issue_campaign(store, campaign, 1);

        // Stable across calls — and, because it is a SHA-256 prefix rather than a
        // `DefaultHasher`, across processes and releases too.
        assert_eq!(key.advisory_lock_key(), key.advisory_lock_key());
        assert_eq!(
            key.advisory_lock_key(),
            advisory_lock_key_for(&key.to_string()),
            "deriving from the stored string must land on the same lock"
        );

        assert_ne!(
            key.advisory_lock_key(),
            JobKey::issue_campaign(store, campaign, 2).advisory_lock_key(),
        );
        assert_ne!(
            key.advisory_lock_key(),
            JobKey::build_audience(store, campaign, 1).advisory_lock_key(),
        );
    }

    #[test]
    fn every_job_type_round_trips_through_its_stored_spelling() {
        for job_type in [
            JobType::BuildCampaignAudience,
            JobType::IssueCampaign,
            JobType::RevokeCampaign,
            JobType::ExpireCoupons,
            JobType::ExecuteAdjustment,
        ] {
            assert_eq!(JobType::from_db(job_type.as_db()), Some(job_type));
        }
        assert_eq!(JobType::from_db("something_new"), None);
    }

    #[test]
    fn job_type_policy_matches_the_documented_table() {
        // §14.6, read straight across.
        assert_eq!(JobType::BuildCampaignAudience.batch_size(), 1_000);
        assert_eq!(
            JobType::BuildCampaignAudience.retry_budget(),
            RetryBudget::Limited(5)
        );
        assert_eq!(JobType::IssueCampaign.batch_size(), 500);
        assert_eq!(JobType::IssueCampaign.retry_budget(), RetryBudget::Limited(10));
        assert_eq!(JobType::RevokeCampaign.batch_size(), 500);
        assert_eq!(JobType::RevokeCampaign.retry_budget(), RetryBudget::Limited(10));
        assert_eq!(JobType::ExpireCoupons.batch_size(), 1_000);
    }

    #[test]
    fn expiry_never_gives_up_but_does_ask_for_attention() {
        // §14.6: 만료 처리는 무제한 지연 재시도. JOB-004 explains why that is safe.
        let budget = JobType::ExpireCoupons.retry_budget();
        assert!(!budget.exhausted(1_000));
        assert_eq!(budget.recorded_max_attempts(), 20, "alerting threshold");

        assert!(RetryBudget::Limited(5).exhausted(5));
        assert!(!RetryBudget::Limited(5).exhausted(4));
    }

    #[test]
    fn statuses_round_trip_and_unknown_values_fail_closed() {
        for status in [
            JobStatus::PendingOutbox,
            JobStatus::Queued,
            JobStatus::Running,
            JobStatus::RetryWait,
            JobStatus::PauseRequested,
            JobStatus::Paused,
            JobStatus::Succeeded,
            JobStatus::DeadLetter,
            JobStatus::Cancelled,
        ] {
            assert_eq!(JobStatus::from_db(status.as_db()), status);
        }

        assert_eq!(
            JobStatus::from_db("SOMETHING_NEW"),
            JobStatus::DeadLetter,
            "a status this build cannot reason about must not look runnable"
        );
    }

    #[test]
    fn the_active_statuses_are_exactly_the_ones_the_unique_index_covers() {
        // §12.6-10. If this list and `uq_job_registry_active_key` disagree, two live jobs
        // could share a key, so the two are asserted against each other here.
        let active: Vec<&str> = JobStatus::ACTIVE.iter().map(|s| s.as_db()).collect();
        assert_eq!(
            active,
            vec![
                "PENDING_OUTBOX",
                "QUEUED",
                "RUNNING",
                "RETRY_WAIT",
                "PAUSE_REQUESTED",
                "PAUSED"
            ]
        );

        for status in [JobStatus::Succeeded, JobStatus::DeadLetter, JobStatus::Cancelled] {
            assert!(!status.is_active());
            assert!(status.is_terminal());
        }
    }

    #[test]
    fn progress_accumulates_across_batches() {
        let mut total = JobProgress::default();
        total.add(JobProgress { processed: 500, succeeded: 498, failed: 2 });
        total.add(JobProgress { processed: 300, succeeded: 300, failed: 0 });

        assert_eq!(total.processed, 800);
        assert_eq!(total.succeeded, 798);
        assert_eq!(total.failed, 2);
    }
}
