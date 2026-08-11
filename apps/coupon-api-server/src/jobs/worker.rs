//! 작업 실행 (§14.5, §14.6, CAMPAIGN-003, ADMIN-005, JOB-002…004).
//!
//! One runner, one job at a time per unique key. The nine steps of §14.5 live in
//! [`JobRuntime::run_once`] and the handlers below only ever see a job they already hold
//! the advisory lock for.
//!
//! Every handler is written to be **resumable and re-runnable**, not merely correct on a
//! clean run:
//!
//! * work is done in batches, with a checkpoint written after each one, so a crash costs
//!   at most one batch (§14.5-7);
//! * the campaign's state is re-read between batches, so a pause or a cancellation is
//!   honoured on a boundary the handler chose (CAMPAIGN-006);
//! * every write lands behind a domain unique constraint, so a job that somehow ran twice
//!   produces the same result as one that ran once (§14.2).

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::campaigns::{Campaign, CampaignStatus, IssueMode, RevokePolicy, audience};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::jobs::transport::JobTransport;
use crate::jobs::{
    ClaimedJob, JobControl, JobFailure, JobKey, JobProgress, JobSpec, JobStatus, JobType,
    RetryClass, backoff_for_attempt, worker_identity,
};
use crate::state::AppState;

/// How many jobs one poll may pick up.
const POLL_BATCH: i64 = 16;
/// §14.5-6: a worker that loses the lock re-queues after a short jittered delay rather
/// than spinning on it.
const LOCK_CONTENTION_DELAY: Duration = Duration::from_secs(5);

/// Where a batched handler left off (§14.5-7).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The last id processed, in the handler's stable order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_id: Option<Uuid>,
    #[serde(default)]
    pub batches: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

impl Checkpoint {
    fn read(raw: &serde_json::Value) -> Self {
        serde_json::from_value(raw.clone()).unwrap_or_default()
    }

    fn value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// The payload every campaign job carries. Advisory only — §14.5-4 says the handler
/// re-reads the campaign, and it does.
#[derive(Debug, Clone, Deserialize)]
struct CampaignPayload {
    campaign_id: Uuid,
    store_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
struct AdjustmentPayload {
    adjustment_id: Uuid,
}

/// Everything a running worker needs.
pub struct JobRuntime {
    state: AppState,
    transport: Arc<dyn JobTransport>,
    worker_id: String,
}

impl JobRuntime {
    pub fn new(state: AppState, transport: Arc<dyn JobTransport>) -> Self {
        Self {
            state,
            transport,
            worker_id: worker_identity(),
        }
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    /// Publish whatever the API committed but could not deliver (§14.2, JOB-005).
    pub async fn relay(&self) -> ApiResult<u64> {
        let now = self.now().await?;
        self.state
            .jobs
            .relay_outbox(&self.state.pool, self.transport.as_ref(), POLL_BATCH, now)
            .await
    }

    /// Take everything currently due. The registry poll is both the JOB-005 fallback and
    /// the recovery path for a message that was published and then lost.
    pub async fn poll(&self) -> ApiResult<u64> {
        let now = self.now().await?;
        self.state.jobs.reclaim_stalled(&self.state.pool, now).await?;

        let due = self
            .state
            .jobs
            .due_jobs(&self.state.pool, now, POLL_BATCH)
            .await?;

        let mut ran = 0;
        for job_id in due {
            if self.run_once(job_id).await? {
                ran += 1;
            }
        }

        Ok(ran)
    }

    /// The database's clock, never the process's.
    ///
    /// §5.2 says the server's database time decides every period, and a job's
    /// `scheduled_at` was written by `clock_timestamp()`. Comparing it against a local
    /// `Utc::now()` would make a worker whose clock runs a millisecond slow decide that
    /// work due *now* is not due yet — rarely, and only under load, which is the worst
    /// kind of bug to have.
    async fn now(&self) -> ApiResult<DateTime<Utc>> {
        crate::qr::database_now(&self.state.pool).await
    }

    /// §14.5, steps 4 through 9, for one job.
    ///
    /// Returns `false` when there was nothing to do — the job moved on, or another worker
    /// holds the lock. Neither is an error and neither spends an attempt.
    pub async fn run_once(&self, job_id: Uuid) -> ApiResult<bool> {
        let now = self.now().await?;

        let Some(job) = self
            .state
            .jobs
            .claim(&self.state.pool, job_id, &self.worker_id, now)
            .await?
        else {
            // §14.5-6. The job may simply be somebody else's right now, so it goes back on
            // the queue after a jittered pause without counting as a failure.
            self.requeue_later(job_id).await;
            return Ok(false);
        };

        let outcome = self.dispatch(&job).await;
        let finished_at = self.now().await.unwrap_or_else(|_| Utc::now());

        match outcome {
            Ok(completion) => match completion.control {
                JobControl::Continue => {
                    self.state
                        .jobs
                        .succeed(
                            &self.state.pool,
                            &job,
                            &completion.checkpoint,
                            completion.progress,
                            finished_at,
                        )
                        .await?;
                    tracing::info!(
                        %job_id,
                        unique_key = job.unique_key,
                        processed = completion.progress.processed,
                        succeeded = completion.progress.succeeded,
                        failed = completion.progress.failed,
                        "jobs.succeeded"
                    );
                }
                JobControl::Pause | JobControl::Cancel => {
                    // The handler stopped on a boundary it chose (CAMPAIGN-006). The
                    // checkpoint is what makes resuming continue rather than restart.
                    self.state
                        .jobs
                        .checkpoint(
                            &self.state.pool,
                            &job,
                            &completion.checkpoint,
                            completion.progress,
                            finished_at,
                        )
                        .await?;
                    self.state
                        .jobs
                        .confirm_paused(&self.state.pool, job.job_id)
                        .await?;
                    tracing::info!(%job_id, unique_key = job.unique_key, "jobs.paused");
                }
            },
            Err(failure) => {
                let status = self
                    .state
                    .jobs
                    .fail(
                        &self.state.pool,
                        &job,
                        &failure.failure,
                        &failure.checkpoint,
                        failure.progress,
                        finished_at,
                    )
                    .await?;

                if status == JobStatus::RetryWait {
                    // The registry already carries `next_attempt_at`; publishing the delay
                    // to the transport as well means the retry does not have to wait for
                    // the next poll tick.
                    let delay = backoff_for_attempt(job.attempt_no);
                    if let Err(error) = self.transport.publish_after(job.job_id, delay).await {
                        tracing::warn!(%error, %job_id, "could not schedule the retry; the poll will pick it up");
                    }
                }
            }
        }

        // Step 8: release the lock and return its connection.
        job.lock.release().await;
        Ok(true)
    }

    async fn requeue_later(&self, job_id: Uuid) {
        if let Err(error) = self
            .transport
            .publish_after(job_id, LOCK_CONTENTION_DELAY)
            .await
        {
            tracing::debug!(%error, %job_id, "could not re-queue after lock contention");
        }
    }

    async fn dispatch(&self, job: &ClaimedJob) -> Result<Completion, Failure> {
        match job.job_type {
            JobType::BuildCampaignAudience => self.build_audience(job).await,
            JobType::IssueCampaign => self.issue_campaign(job).await,
            JobType::RevokeCampaign => self.revoke_campaign(job).await,
            JobType::ExpireCoupons => self.expire(job).await,
            JobType::ExecuteAdjustment => self.execute_adjustment(job).await,
        }
    }

    // -----------------------------------------------------------------------
    // Handlers
    // -----------------------------------------------------------------------

    /// CAMPAIGN-003 step 2: freeze who was eligible at publish time.
    async fn build_audience(&self, job: &ClaimedJob) -> Result<Completion, Failure> {
        let mut checkpoint = Checkpoint::read(&job.checkpoint);
        let mut progress = JobProgress::default();

        let payload: CampaignPayload = parse_payload(&job.payload, &checkpoint, progress)?;

        loop {
            let now = Utc::now();
            let mut tx = begin(&self.state, &checkpoint, progress).await?;

            let campaign = self
                .state
                .campaigns
                .find(&mut *tx, payload.store_id, payload.campaign_id)
                .await
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

            if let Some(stop) = campaign_stop_reason(&campaign) {
                return Err(Failure::permanent(stop, &checkpoint, progress));
            }

            let page = audience::page(
                &mut tx,
                payload.store_id,
                &campaign,
                checkpoint.after_id,
                JobType::BuildCampaignAudience.batch_size(),
                now,
            )
            .await
            .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

            if page.is_empty() {
                // Done. Recording *when* the snapshot was taken is what makes
                // `audience::is_eligible` switch from live evaluation to the frozen list.
                sqlx::query!(
                    r#"
                    UPDATE coupon.campaigns
                    SET audience_snapshot_at = $2,
                        audience_size = (
                            SELECT COUNT(*) FROM coupon.campaign_audience_members
                            WHERE campaign_id = $1
                        )
                    WHERE id = $1
                    "#,
                    payload.campaign_id,
                    now,
                )
                .execute(&mut *tx)
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

                // CAMPAIGN-003 step 1 continues here: the issuing job is registered by the
                // job that finished deciding who it is for, inside the same commit.
                let spec = JobSpec::new(
                    JobKey::issue_campaign(
                        payload.store_id,
                        payload.campaign_id,
                        campaign.issue_generation,
                    ),
                    serde_json::json!({
                        "campaign_id": payload.campaign_id,
                        "store_id": payload.store_id,
                    }),
                )
                .store(payload.store_id)
                .resource(payload.campaign_id);

                self.state
                    .jobs
                    .enqueue(&mut tx, &spec)
                    .await
                    .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

                tx.commit()
                    .await
                    .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

                return Ok(Completion {
                    checkpoint: checkpoint.value(),
                    progress,
                    control: JobControl::Continue,
                });
            }

            let last = page.last().copied();
            for user_id in &page {
                sqlx::query!(
                    r#"
                    INSERT INTO coupon.campaign_audience_members
                        (campaign_id, user_id, snapshot_reason, status)
                    VALUES ($1, $2, $3, 'PENDING')
                    ON CONFLICT (campaign_id, user_id) DO NOTHING
                    "#,
                    payload.campaign_id,
                    user_id,
                    serde_json::json!({
                        "audience_type": campaign.audience_type.as_db(),
                        "snapshot_at": now,
                    }),
                )
                .execute(&mut *tx)
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;
            }

            tx.commit()
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

            progress.processed += page.len() as i64;
            progress.succeeded += page.len() as i64;
            checkpoint.after_id = last;
            checkpoint.batches += 1;

            if let Some(control) = self.between_batches(job, &checkpoint, progress).await? {
                return Ok(control);
            }
        }
    }

    /// CAMPAIGN-003 steps 3–6: page the audience and issue.
    async fn issue_campaign(&self, job: &ClaimedJob) -> Result<Completion, Failure> {
        let mut checkpoint = Checkpoint::read(&job.checkpoint);
        let mut progress = JobProgress::default();

        let payload: CampaignPayload = parse_payload(&job.payload, &checkpoint, progress)?;

        loop {
            let now = Utc::now();
            let mut tx = begin(&self.state, &checkpoint, progress).await?;

            // §13.1's lock order, the same one the claim path takes, so a bulk issuance
            // and a manual accrual in the same store queue rather than deadlock.
            self.state
                .stores
                .lock_store(&mut tx, payload.store_id)
                .await
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;
            let store = self
                .state
                .stores
                .find_public(&mut *tx, payload.store_id)
                .await
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

            let campaign = self
                .state
                .campaigns
                .lock(&mut tx, payload.store_id, payload.campaign_id)
                .await
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

            // CAMPAIGN-006: 진행 중 대량 발급 워커는 배치 사이에서 상태를 확인한다. Read
            // *before* the batch, so a campaign paused a millisecond ago issues nothing.
            if let Some(stop) = campaign_stop_reason(&campaign) {
                return Err(Failure::permanent(stop, &checkpoint, progress));
            }

            // §15 동시성 결정표, 캠페인 중지와 발급 batch: 중지 확인 뒤 신규 배치 없음.
            // The heartbeat also reports a pause, but it can only do so *after* a batch;
            // reading the campaign here is what makes the pause bind to the next batch
            // rather than to the one after it.
            if campaign.status == CampaignStatus::Paused {
                return Ok(Completion {
                    checkpoint: checkpoint.value(),
                    progress,
                    control: JobControl::Pause,
                });
            }

            let members = sqlx::query!(
                r#"
                SELECT user_id
                FROM coupon.campaign_audience_members
                WHERE campaign_id = $1 AND status = 'PENDING'
                  AND ($2::uuid IS NULL OR user_id > $2)
                ORDER BY user_id
                LIMIT $3
                "#,
                payload.campaign_id,
                checkpoint.after_id,
                JobType::IssueCampaign.batch_size(),
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

            if members.is_empty() {
                tx.commit()
                    .await
                    .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;
                return Ok(Completion {
                    checkpoint: checkpoint.value(),
                    progress,
                    control: JobControl::Continue,
                });
            }

            let business_day = store
                .calendar()
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?
                .business_day(now);
            let counter = self
                .state
                .campaigns
                .lock_counter(&mut tx, payload.campaign_id, business_day)
                .await
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

            let cap = campaign.total_quantity.effective_cap();
            let mut issued_this_batch = 0i64;
            let mut last_user_id = None;

            for member in &members {
                last_user_id = Some(member.user_id);

                // §12.6-4/5, re-checked per member because the counters move as we go.
                let committed = campaign.issued_count + campaign.reserved_count + issued_this_batch;
                if committed >= cap {
                    break;
                }
                if let Some(daily) = campaign.per_business_day_quantity
                    && counter.issued_count + counter.reserved_count + issued_this_batch >= daily
                {
                    break;
                }

                let existing = self
                    .state
                    .campaigns
                    .existing_issuance(&mut tx, payload.campaign_id, member.user_id)
                    .await
                    .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

                if existing.count >= i64::from(campaign.per_user_quantity) {
                    // Already has everything the campaign allows them. Not a failure —
                    // a resumed run sees this for every member the previous run reached.
                    self.mark_member(&mut tx, payload.campaign_id, member.user_id, "SKIPPED", None, Some("PER_USER_LIMIT"), job.job_id)
                        .await
                        .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;
                    progress.processed += 1;
                    continue;
                }

                match self
                    .state
                    .campaigns
                    .issue_instance(
                        &mut tx,
                        &store,
                        &campaign,
                        member.user_id,
                        existing.next_ordinal,
                        now,
                        None,
                        Some(job.job_id),
                    )
                    .await
                {
                    Ok(issued) => {
                        self.mark_member(
                            &mut tx,
                            payload.campaign_id,
                            member.user_id,
                            "ISSUED",
                            Some(issued.coupon_id),
                            None,
                            job.job_id,
                        )
                        .await
                        .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;
                        issued_this_batch += 1;
                        progress.processed += 1;
                        progress.succeeded += 1;
                    }
                    // §13.2-6 again: a unique violation means this coupon already exists,
                    // which is precisely what a re-run of an interrupted job looks like.
                    Err(error) if error.code == ErrorCode::Conflict => {
                        self.mark_member(
                            &mut tx,
                            payload.campaign_id,
                            member.user_id,
                            "ISSUED",
                            None,
                            Some("ALREADY_ISSUED"),
                            job.job_id,
                        )
                        .await
                        .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;
                        progress.processed += 1;
                        progress.succeeded += 1;
                    }
                    Err(error) => {
                        // One member's failure does not sink the campaign; it is recorded
                        // and the run continues (JOB-003 부분 성공).
                        self.mark_member(
                            &mut tx,
                            payload.campaign_id,
                            member.user_id,
                            "FAILED",
                            None,
                            Some(error.code.as_str()),
                            job.job_id,
                        )
                        .await
                        .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;
                        progress.processed += 1;
                        progress.failed += 1;
                    }
                }
            }

            if issued_this_batch > 0 {
                self.state
                    .campaigns
                    .bump_counters(&mut tx, payload.campaign_id, business_day, issued_this_batch)
                    .await
                    .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;
            }

            tx.commit()
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

            checkpoint.after_id = last_user_id;
            checkpoint.batches += 1;

            if let Some(control) = self.between_batches(job, &checkpoint, progress).await? {
                return Ok(control);
            }
        }
    }

    /// CAMPAIGN-007 / ADMIN-005: take back unused coupons.
    ///
    /// `USED` coupons are never touched — ADMIN-005 keeps them in the statistics and puts
    /// them in the case instead.
    async fn revoke_campaign(&self, job: &ClaimedJob) -> Result<Completion, Failure> {
        let mut checkpoint = Checkpoint::read(&job.checkpoint);
        let mut progress = JobProgress::default();

        let payload: CampaignPayload = parse_payload(&job.payload, &checkpoint, progress)?;
        let reason = job
            .payload
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("캠페인 회수")
            .to_owned();

        loop {
            let now = Utc::now();
            let mut tx = begin(&self.state, &checkpoint, progress).await?;

            let campaign = self
                .state
                .campaigns
                .lock(&mut tx, payload.store_id, payload.campaign_id)
                .await
                .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

            // §14.6's 영구 실패 예 for a revocation is 승인 철회: the campaign is no
            // longer asking for its coupons back.
            if campaign.revoke_policy != RevokePolicy::RevokeUnused {
                return Err(Failure::permanent(
                    "REVOCATION_WITHDRAWN",
                    &checkpoint,
                    progress,
                ));
            }

            let coupons = sqlx::query!(
                r#"
                SELECT id, status::text AS "status!"
                FROM coupon.coupon_instances
                WHERE campaign_id = $1
                  AND status IN ('AVAILABLE', 'RESERVED', 'PENDING')
                  AND ($2::uuid IS NULL OR id > $2)
                ORDER BY id
                LIMIT $3
                FOR UPDATE SKIP LOCKED
                "#,
                payload.campaign_id,
                checkpoint.after_id,
                JobType::RevokeCampaign.batch_size(),
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

            if coupons.is_empty() {
                tx.commit()
                    .await
                    .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;
                return Ok(Completion {
                    checkpoint: checkpoint.value(),
                    progress,
                    control: JobControl::Continue,
                });
            }

            let mut revoked = 0i64;
            let mut last_id = None;

            for coupon in &coupons {
                last_id = Some(coupon.id);

                // CAMPAIGN-007: RESERVED 쿠폰은 신규 최종 승인을 막고 예약을 회수 상태로
                // 전환한다.
                sqlx::query!(
                    r#"
                    UPDATE coupon.redemption_reservations
                    SET status = 'REVOKED', completed_at = $2,
                        cancelled_reason = 'CAMPAIGN_REVOKED'
                    WHERE coupon_id = $1 AND status = 'ACTIVE'
                    "#,
                    coupon.id,
                    now,
                )
                .execute(&mut *tx)
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

                let changed = sqlx::query!(
                    r#"
                    UPDATE coupon.coupon_instances
                    SET status = 'REVOKED', revoked_at = $2, revocation_reason = $3
                    WHERE id = $1 AND status IN ('AVAILABLE', 'RESERVED', 'PENDING')
                    "#,
                    coupon.id,
                    now,
                    reason,
                )
                .execute(&mut *tx)
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

                progress.processed += 1;

                if changed.rows_affected() == 1 {
                    sqlx::query!(
                        r#"
                        INSERT INTO coupon.coupon_status_events
                            (coupon_id, from_status, to_status, actor_type, reason_code,
                             metadata, occurred_at)
                        VALUES ($1, $2::text::coupon.coupon_status, 'REVOKED', 'SYSTEM',
                                'CAMPAIGN_REVOKED', $3, $4)
                        "#,
                        coupon.id,
                        coupon.status,
                        serde_json::json!({ "campaign_id": payload.campaign_id, "job_id": job.job_id }),
                        now,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

                    revoked += 1;
                    progress.succeeded += 1;
                } else {
                    progress.failed += 1;
                }
            }

            if revoked > 0 {
                // §8.4: whether a revoked coupon frees its slot is the campaign's own
                // fixed decision, taken when it was created and never editable since.
                if campaign.restore_quantity_on_revoke {
                    sqlx::query!(
                        r#"
                        UPDATE coupon.campaigns
                        SET global_revoked_count = global_revoked_count + $2,
                            global_issued_count = GREATEST(global_issued_count - $2, 0)
                        WHERE id = $1
                        "#,
                        payload.campaign_id,
                        revoked,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;
                } else {
                    sqlx::query!(
                        r#"
                        UPDATE coupon.campaigns
                        SET global_revoked_count = global_revoked_count + $2
                        WHERE id = $1
                        "#,
                        payload.campaign_id,
                        revoked,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;
                }
            }

            tx.commit()
                .await
                .map_err(|error| Failure::from_sqlx(error, &checkpoint, progress))?;

            checkpoint.after_id = last_id;
            checkpoint.batches += 1;

            if let Some(control) = self.between_batches(job, &checkpoint, progress).await? {
                return Ok(control);
            }
        }
    }

    /// §18.1 housekeeping: write the terminal states online reads already assume.
    async fn expire(&self, job: &ClaimedJob) -> Result<Completion, Failure> {
        let checkpoint = Checkpoint::read(&job.checkpoint);
        let mut progress = JobProgress::default();
        let now = Utc::now();
        let batch = JobType::ExpireCoupons.batch_size();

        let stamps = self
            .state
            .loyalty_stamps
            .expire_due_lots(&self.state.pool, now, batch)
            .await
            .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

        let coupons = self
            .state
            .wallet
            .expire_due_coupons(&self.state.pool, now, batch)
            .await
            .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

        // REDEEM-002: a hold whose two minutes ran out returns the coupon.
        let reservations = self
            .state
            .redemptions
            .expire_due_reservations(&self.state.pool, now, batch)
            .await
            .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

        progress.processed = (stamps + coupons + reservations) as i64;
        progress.succeeded = progress.processed;

        Ok(Completion {
            checkpoint: checkpoint.value(),
            progress,
            control: JobControl::Continue,
        })
    }

    /// ADMIN-003: 대량 보정은 동기 API 가 아니라 검토 가능한 큐 작업으로 실행한다.
    async fn execute_adjustment(&self, job: &ClaimedJob) -> Result<Completion, Failure> {
        let checkpoint = Checkpoint::read(&job.checkpoint);
        let mut progress = JobProgress::default();

        let payload: AdjustmentPayload = serde_json::from_value(job.payload.clone())
            .map_err(|error| {
                Failure::permanent(
                    &format!("MALFORMED_PAYLOAD: {error}"),
                    &checkpoint,
                    progress,
                )
            })?;

        let applied = self
            .state
            .admin
            .execute_adjustment(&self.state.pool, payload.adjustment_id, job.job_id)
            .await
            .map_err(|error| Failure::from_api(error, &checkpoint, progress))?;

        progress.processed = applied;
        progress.succeeded = applied;

        Ok(Completion {
            checkpoint: checkpoint.value(),
            progress,
            control: JobControl::Continue,
        })
    }

    /// Heartbeat, checkpoint and ask whether to keep going (§14.5-7).
    async fn between_batches(
        &self,
        job: &ClaimedJob,
        checkpoint: &Checkpoint,
        progress: JobProgress,
    ) -> Result<Option<Completion>, Failure> {
        let control = self
            .state
            .jobs
            .checkpoint(
                &self.state.pool,
                job,
                &checkpoint.value(),
                progress,
                Utc::now(),
            )
            .await
            .map_err(|error| Failure::from_api(error, checkpoint, progress))?;

        Ok(match control {
            JobControl::Continue => None,
            other => Some(Completion {
                checkpoint: checkpoint.value(),
                progress,
                control: other,
            }),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn mark_member(
        &self,
        tx: &mut crate::db::Tx<'_>,
        campaign_id: Uuid,
        user_id: Uuid,
        status: &str,
        coupon_id: Option<Uuid>,
        error_code: Option<&str>,
        job_id: Uuid,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.campaign_audience_members
            SET status = $3, processed_at = clock_timestamp(), coupon_id = $4,
                error_code = $5, issued_job_id = $6
            WHERE campaign_id = $1 AND user_id = $2
            "#,
            campaign_id,
            user_id,
            status,
            coupon_id,
            error_code,
            job_id,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

/// A handler that finished, or stopped where it was asked to.
struct Completion {
    checkpoint: serde_json::Value,
    progress: JobProgress,
    control: JobControl,
}

/// A handler that failed, carrying the progress it did make so the next attempt resumes
/// rather than restarts (§14.7 부분 성공).
struct Failure {
    failure: JobFailure,
    checkpoint: serde_json::Value,
    progress: JobProgress,
}

impl Failure {
    fn permanent(code: &str, checkpoint: &Checkpoint, progress: JobProgress) -> Self {
        Self {
            failure: JobFailure::permanent(code, code),
            checkpoint: checkpoint.value(),
            progress,
        }
    }

    fn from_api(error: ApiError, checkpoint: &Checkpoint, progress: JobProgress) -> Self {
        Self {
            failure: JobFailure::from_api(&error),
            checkpoint: checkpoint.value(),
            progress,
        }
    }

    fn from_sqlx(error: sqlx::Error, checkpoint: &Checkpoint, progress: JobProgress) -> Self {
        let class = crate::jobs::retry::classify_sqlx_error(&error);
        Self {
            failure: JobFailure {
                code: "DATABASE_ERROR".to_owned(),
                message: error.to_string(),
                class,
            },
            checkpoint: checkpoint.value(),
            progress,
        }
    }
}

/// Whether a campaign's current state means the run must stop for good.
///
/// §14.6 names 캠페인 취소 as a permanent failure for bulk issuance — retrying it would
/// only reproduce the same refusal, and CAMPAIGN-006's pause is handled separately
/// because that one is meant to be resumable.
fn campaign_stop_reason(campaign: &Campaign) -> Option<&'static str> {
    match campaign.status {
        CampaignStatus::Cancelled => Some("CAMPAIGN_CANCELLED"),
        CampaignStatus::Ended => Some("CAMPAIGN_ENDED"),
        CampaignStatus::Draft => Some("CAMPAIGN_NOT_PUBLISHED"),
        CampaignStatus::Paused | CampaignStatus::Scheduled | CampaignStatus::Issuing => {
            if campaign.issue_mode != IssueMode::Direct {
                Some("CAMPAIGN_NOT_DIRECT")
            } else {
                None
            }
        }
    }
}

fn parse_payload<T: serde::de::DeserializeOwned>(
    payload: &serde_json::Value,
    checkpoint: &Checkpoint,
    progress: JobProgress,
) -> Result<T, Failure> {
    serde_json::from_value(payload.clone()).map_err(|error| {
        // §14.7: a malformed payload is not something a retry can fix.
        Failure {
            failure: JobFailure {
                code: "MALFORMED_PAYLOAD".to_owned(),
                message: error.to_string(),
                class: RetryClass::Permanent,
            },
            checkpoint: checkpoint.value(),
            progress,
        }
    })
}

async fn begin(
    state: &AppState,
    checkpoint: &Checkpoint,
    progress: JobProgress,
) -> Result<crate::db::Tx<'static>, Failure> {
    state
        .pool
        .begin()
        .await
        .map_err(|error| Failure::from_sqlx(error, checkpoint, progress))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaigns::tests_support::campaign;

    #[test]
    fn a_checkpoint_round_trips_and_an_unreadable_one_starts_over_safely() {
        let checkpoint = Checkpoint {
            after_id: Some(Uuid::from_u128(9)),
            batches: 3,
            phase: Some("issue".to_owned()),
        };

        let round_tripped = Checkpoint::read(&checkpoint.value());
        assert_eq!(round_tripped.after_id, checkpoint.after_id);
        assert_eq!(round_tripped.batches, 3);

        // A checkpoint this build cannot read means starting from the beginning — which
        // is safe precisely because every write is behind a domain unique constraint
        // (§14.2), so re-processing produces no duplicates.
        let unreadable = Checkpoint::read(&serde_json::json!({ "after_id": "not-a-uuid" }));
        assert_eq!(unreadable.after_id, None);
        assert_eq!(unreadable.batches, 0);
    }

    #[test]
    fn an_empty_checkpoint_serialises_without_a_cursor() {
        let json = Checkpoint::default().value();
        assert!(json.get("after_id").is_none());
        assert_eq!(json["batches"], 0);
    }

    #[test]
    fn a_cancelled_campaign_stops_the_issuing_job_for_good() {
        // §14.6: 쿠폰 대량 발급의 영구 실패 예 = 캠페인 취소.
        let mut campaign = campaign();
        campaign.issue_mode = IssueMode::Direct;

        campaign.status = CampaignStatus::Cancelled;
        assert_eq!(campaign_stop_reason(&campaign), Some("CAMPAIGN_CANCELLED"));

        campaign.status = CampaignStatus::Ended;
        assert_eq!(campaign_stop_reason(&campaign), Some("CAMPAIGN_ENDED"));

        campaign.status = CampaignStatus::Draft;
        assert_eq!(campaign_stop_reason(&campaign), Some("CAMPAIGN_NOT_PUBLISHED"));
    }

    #[test]
    fn a_paused_campaign_does_not_stop_the_job_for_good() {
        // CAMPAIGN-006: pausing is resumable, so it must not be a permanent failure —
        // the pause is honoured through the heartbeat's `JobControl::Pause` instead.
        let mut campaign = campaign();
        campaign.issue_mode = IssueMode::Direct;
        campaign.status = CampaignStatus::Paused;

        assert_eq!(campaign_stop_reason(&campaign), None);
    }

    #[test]
    fn a_first_come_campaign_has_no_issuing_job_to_run() {
        let mut campaign = campaign();
        campaign.issue_mode = IssueMode::FirstCome;
        campaign.status = CampaignStatus::Issuing;

        assert_eq!(campaign_stop_reason(&campaign), Some("CAMPAIGN_NOT_DIRECT"));
    }
}
