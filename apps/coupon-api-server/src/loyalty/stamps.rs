//! Stamp accrual, goal completion and reversal (§13.1, §13.4, STAMP-002…STAMP-007).
//!
//! The ledger is the truth. A customer's balance is never a column that gets incremented;
//! it is `SUM(quantity_delta)` over `stamp_ledger`, per lot, and every state change is a
//! new row rather than an edit to an old one (product principle 2).
//!
//! ## Lock order
//!
//! §13.1 fixes it: `store → policy → user/store_customer → coupon/stamp lot → nonce`.
//! Every path in this module takes locks in exactly that order, which is why an accrual
//! and a reversal running at the same moment queue up instead of deadlocking.
//!
//! The nonce is the odd one out: §13.1 also says to validate it *first*. Both are
//! satisfied by validating it with an unlocked read before the transaction opens, and
//! taking its row lock last — so a nonce consumed in between is caught at the point where
//! it matters, when the accrual tries to claim it.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{ActorType, AuditEntry, AuditService};
use crate::catalog::{CatalogService, OrderItemInput, OrderLine};
use crate::crypto::LookupHash;
use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::loyalty::policy::{LoyaltyPolicy, PolicyService};
use crate::qr::{AUDIENCE_STAMP, Presented, QrService};
use crate::stores::{OwnedStore, StoreService};

/// `coupon.stamp_transaction_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StampTransactionStatus {
    Confirmed,
    Voided,
    RequiresAdminReview,
}

impl StampTransactionStatus {
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "CONFIRMED" => StampTransactionStatus::Confirmed,
            "VOIDED" => StampTransactionStatus::Voided,
            _ => StampTransactionStatus::RequiresAdminReview,
        }
    }
}

/// What the owner is allowed to know about the customer in front of them (§3.1,
/// WALLET-005).
///
/// Every field here is either store-local or masked. There is deliberately no email, no
/// phone number, no Kakao identifier, and nothing about any other store.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PseudonymousCustomer {
    /// Store-scoped pseudonymous id. `None` until the customer's first accrual here.
    pub store_customer_id: Option<Uuid>,
    /// Stable store-local nickname, derived rather than taken from the profile.
    pub alias: String,
    /// e.g. `김**`.
    pub masked_name: String,
    pub is_new_customer: bool,
    pub first_seen_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub visit_count: i64,
}

/// The stamp board as both apps render it (§8.1: `N/10`, 가장 이른 도장 만료).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StampBoard {
    pub available: i64,
    pub target: i64,
    /// How many more stamps until the next reward.
    pub remaining_to_goal: i64,
    /// The soonest any currently-held stamp expires.
    pub earliest_expiry: Option<DateTime<Utc>>,
    /// Stamps that expire within seven days (§6.2 홈 우선 노출).
    pub expiring_within_seven_days: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PolicySummary {
    pub policy_id: Uuid,
    pub version_no: i32,
    pub name: String,
    pub target_stamp_count: i64,
    pub stamps_per_order: i64,
    pub minimum_order_amount: i64,
    pub daily_earning_limit: Option<i64>,
    pub restricts_items: bool,
    pub reward_title: String,
    pub reward_validity_days: i16,
}

/// `POST /owner/scan/resolve` (§11.4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ScanResolution {
    pub customer: PseudonymousCustomer,
    /// When the presented QR stops being usable, so the owner knows how long they have.
    pub qr_expires_at: DateTime<Utc>,
    pub policy: PolicySummary,
    pub stamp_board: StampBoard,
    /// True when the owner is scanning their own consumer profile (SEC-004). Not blocked,
    /// but recorded and shown.
    pub self_transaction: bool,
}

/// One reason an accrual cannot be approved as submitted.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PreviewIssue {
    pub code: String,
    pub message: String,
}

/// `POST /owner/stamp-transactions/preview` (§11.4).
///
/// Returns 200 even when the accrual would be refused, with the reasons listed: the scan
/// screen's REVIEW step has to *show* why (§6.3), and an error response cannot carry the
/// customer and board context it needs to do that. `confirm` re-checks everything, so a
/// stale or forged preview buys nothing (§11.4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StampPreview {
    pub preview_id: Uuid,
    pub customer: PseudonymousCustomer,
    pub policy: PolicySummary,
    pub business_day: NaiveDate,
    pub gross_amount: i64,
    /// The part of the order that counts, after item restrictions (§8.3).
    pub eligible_amount: i64,
    pub expected_stamps: i64,
    pub stamp_board_before: StampBoard,
    pub stamp_board_after: StampBoard,
    pub rewards_to_issue: i64,
    /// When the stamps this order would grant expire.
    pub stamps_expire_at: DateTime<Utc>,
    pub daily_used: i64,
    pub daily_limit: Option<i64>,
    /// Start of the next business day, for the "come back tomorrow" message (STAMP-005).
    pub next_business_day_start: DateTime<Utc>,
    pub self_transaction: bool,
    /// Non-blocking: shown to the owner before they approve (STAMP-003, SEC-004).
    pub warnings: Vec<PreviewIssue>,
    /// Blocking. `approvable` is false whenever this is non-empty.
    pub blockers: Vec<PreviewIssue>,
    pub approvable: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssuedReward {
    pub coupon_id: Uuid,
    pub title: String,
    pub description: String,
    pub usable_from: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// The result of `POST /owner/stamp-transactions` and of a reversal.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StampTransaction {
    pub transaction_id: Uuid,
    pub store_id: Uuid,
    pub status: StampTransactionStatus,
    pub business_day: NaiveDate,
    pub quantity: i64,
    pub external_order_ref: Option<String>,
    pub confirmed_at: DateTime<Utc>,
    pub voided_at: Option<DateTime<Utc>>,
    pub customer: PseudonymousCustomer,
    pub stamp_board: StampBoard,
    pub issued_rewards: Vec<IssuedReward>,
    /// Rewards taken back by a reversal (STAMP-007).
    pub revoked_reward_ids: Vec<Uuid>,
    pub stamps_expire_at: Option<DateTime<Utc>>,
    /// Until when the owner may still reverse this themselves (§8.6).
    pub voidable_until: Option<DateTime<Utc>>,
}

/// The order as the owner typed it (§11.4 적립 승인 요청 예시).
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct OrderInput {
    /// Optional POS reference. STAMP-003 uses a *new* one as the owner's confirmation
    /// that a near-duplicate really is a separate order.
    #[validate(length(max = 200))]
    pub external_order_ref: Option<String>,
    #[validate(range(min = 0, max = 100_000_000, message = "주문 금액을 확인해 주세요."))]
    pub gross_amount: i64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    #[validate(nested)]
    pub items: Vec<OrderItemInput>,
}

fn default_currency() -> String {
    "KRW".to_owned()
}

/// How the owner identified the customer. Exactly one must be present.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ScanRequest {
    /// The signed payload read from the camera.
    pub qr_token: Option<String>,
    /// The eight digits from the customer's screen (STORE-005).
    #[validate(length(min = 8, max = 16))]
    pub fallback_code: Option<String>,
}

impl ScanRequest {
    pub fn presented(&self) -> ApiResult<Presented<'_>> {
        match (self.qr_token.as_deref(), self.fallback_code.as_deref()) {
            (Some(token), None) => Ok(Presented::Token(token)),
            (None, Some(code)) => Ok(Presented::FallbackCode(code)),
            _ => Err(ApiError::with_message(
                ErrorCode::InvalidRequest,
                "QR 토큰 또는 8자리 보조 코드 중 하나만 보내 주세요.",
            )),
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct StampPreviewRequest {
    #[serde(flatten)]
    #[validate(nested)]
    pub scan: ScanRequest,
    #[validate(nested)]
    pub order: OrderInput,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ConfirmStampRequest {
    #[serde(flatten)]
    #[validate(nested)]
    pub scan: ScanRequest,
    /// Echoed from the preview. Recorded for support, never trusted as a substitute for
    /// re-validation (§11.4).
    pub preview_id: Option<Uuid>,
    #[validate(nested)]
    pub order: OrderInput,
    /// Approve despite a duplicate warning, having entered a distinct order reference
    /// (STAMP-003).
    #[serde(default)]
    pub acknowledge_duplicate: bool,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema, Validate)]
pub struct VoidStampRequest {
    #[validate(length(min = 1, max = 500, message = "취소 사유를 입력해 주세요."))]
    pub reason: Option<String>,
}

/// A lot with its derived balance, as the consumption planner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LotBalance {
    pub lot_id: Uuid,
    pub balance: i64,
    pub expires_at: DateTime<Utc>,
}

/// One reward's worth of stamps, and which lots pay for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardDraw {
    pub draws: Vec<(Uuid, i64)>,
}

impl RewardDraw {
    pub fn total(&self) -> i64 {
        self.draws.iter().map(|(_, quantity)| quantity).sum()
    }
}

/// Stamps a customer can actually spend right now.
///
/// Expiry is decided here, against the caller's `now`, rather than by trusting a batch to
/// have run: §18.1 requires the online judgement to be immediate.
pub fn available_stamps(lots: &[LotBalance], now: DateTime<Utc>) -> i64 {
    lots.iter()
        .filter(|lot| lot.expires_at > now && lot.balance > 0)
        .map(|lot| lot.balance)
        .sum()
}

/// Plan goal completions, spending the stamps that expire first (§8.1).
///
/// Returns one entry per reward that can be paid for. Spending the soonest-to-expire
/// stamps is what makes a full board actually cash out instead of quietly ageing away.
pub fn plan_reward_consumption(
    lots: &[LotBalance],
    target: i64,
    now: DateTime<Utc>,
) -> Vec<RewardDraw> {
    if target <= 0 {
        return Vec::new();
    }

    let mut usable: Vec<LotBalance> = lots
        .iter()
        .filter(|lot| lot.expires_at > now && lot.balance > 0)
        .cloned()
        .collect();
    // Earliest expiry first; the lot id only breaks ties so the plan is deterministic.
    usable.sort_by(|left, right| {
        left.expires_at
            .cmp(&right.expires_at)
            .then_with(|| left.lot_id.cmp(&right.lot_id))
    });

    let mut rewards = Vec::new();
    let mut cursor = 0usize;

    loop {
        let remaining_total: i64 = usable[cursor..].iter().map(|lot| lot.balance).sum();
        if remaining_total < target {
            break;
        }

        let mut needed = target;
        let mut draws = Vec::new();

        while needed > 0 {
            let lot = &mut usable[cursor];
            let take = needed.min(lot.balance);
            if take > 0 {
                draws.push((lot.lot_id, take));
                lot.balance -= take;
                needed -= take;
            }
            if lot.balance == 0 {
                cursor += 1;
            }
        }

        rewards.push(RewardDraw { draws });
    }

    rewards
}

pub struct StampService {
    stores: Arc<StoreService>,
    policies: Arc<PolicyService>,
    catalog: Arc<CatalogService>,
    qr: Arc<QrService>,
    audit: Arc<AuditService>,
    lookup_hash: Arc<LookupHash>,
    void_window: chrono::Duration,
}

impl StampService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stores: Arc<StoreService>,
        policies: Arc<PolicyService>,
        catalog: Arc<CatalogService>,
        qr: Arc<QrService>,
        audit: Arc<AuditService>,
        lookup_hash: Arc<LookupHash>,
        void_window: chrono::Duration,
    ) -> Self {
        Self {
            stores,
            policies,
            catalog,
            qr,
            audit,
            lookup_hash,
            void_window,
        }
    }

    /// `POST /owner/scan/resolve`: verify the QR and describe who is standing there.
    ///
    /// Does not consume the nonce — the owner may still cancel, and a scan that never
    /// becomes a transaction must leave the customer's QR usable.
    pub async fn resolve_scan(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        request: &ScanRequest,
    ) -> ApiResult<ScanResolution> {
        store.ensure_operating()?;

        let now = crate::qr::database_now(pool).await?;
        let nonce = self
            .qr
            .resolve(pool, request.presented()?, AUDIENCE_STAMP, now)
            .await?;

        let mut tx = pool.begin().await?;
        let policy = self.policies.active_for_accrual(&mut tx, store.id, now).await?;
        let customer = self.describe_customer(&mut tx, store.id, nonce.user_id).await?;
        let board = self
            .load_board(&mut tx, store.id, nonce.user_id, &policy, now, false)
            .await?;
        let owner_user_id = self.owner_of(&mut tx, store.id).await?;
        tx.commit().await?;

        Ok(ScanResolution {
            customer,
            qr_expires_at: nonce.expires_at,
            policy: summarise(&policy),
            stamp_board: board,
            self_transaction: owner_user_id == nonce.user_id,
        })
    }

    /// `POST /owner/stamp-transactions/preview`.
    pub async fn preview(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        request: &StampPreviewRequest,
    ) -> ApiResult<StampPreview> {
        store.ensure_operating()?;

        let now = crate::qr::database_now(pool).await?;
        let nonce = self
            .qr
            .resolve(pool, request.scan.presented()?, AUDIENCE_STAMP, now)
            .await?;

        let mut tx = pool.begin().await?;
        let policy = self.policies.active_for_accrual(&mut tx, store.id, now).await?;
        let lines = self
            .catalog
            .resolve_order_lines(&mut *tx, store.id, &request.order.items)
            .await?;
        let assessment = self
            .assess(&mut tx, store, &policy, nonce.user_id, &request.order, &lines, now)
            .await?;
        let customer = self.describe_customer(&mut tx, store.id, nonce.user_id).await?;
        let lots = self.load_lot_balances(&mut tx, store.id, nonce.user_id, false).await?;
        tx.commit().await?;

        let target = i64::from(policy.rules.target_stamp_count);
        let before = build_board(&lots, target, now);

        // Model the accrual without writing it: one hypothetical lot, then the same
        // planner `confirm` will run.
        let mut projected = lots.clone();
        projected.push(LotBalance {
            lot_id: Uuid::nil(),
            balance: assessment.quantity,
            expires_at: assessment.stamps_expire_at,
        });
        let rewards = plan_reward_consumption(&projected, target, now);
        let after_balance = available_stamps(&projected, now) - rewards.len() as i64 * target;

        Ok(StampPreview {
            preview_id: Uuid::new_v4(),
            customer,
            policy: summarise(&policy),
            business_day: assessment.business_day,
            gross_amount: request.order.gross_amount,
            eligible_amount: assessment.eligible_amount,
            expected_stamps: assessment.quantity,
            stamp_board_before: before,
            stamp_board_after: StampBoard {
                available: after_balance,
                target,
                remaining_to_goal: (target - after_balance % target.max(1)).min(target),
                earliest_expiry: earliest_expiry(&projected, now),
                expiring_within_seven_days: expiring_soon(&projected, now),
            },
            rewards_to_issue: rewards.len() as i64,
            stamps_expire_at: assessment.stamps_expire_at,
            daily_used: assessment.daily_used,
            daily_limit: assessment.daily_limit,
            next_business_day_start: assessment.next_business_day_start,
            self_transaction: assessment.self_transaction,
            warnings: assessment.warnings,
            blockers: assessment.blockers.clone(),
            approvable: assessment.blockers.is_empty(),
        })
    }

    /// `POST /owner/stamp-transactions`: the §13.1 transaction, in its documented order.
    pub async fn confirm(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        idempotency_key: Uuid,
        request: &ConfirmStampRequest,
    ) -> ApiResult<StampTransaction> {
        store.ensure_operating()?;

        // Step 2, part one: the nonce is *validated* before the transaction opens. Its
        // row lock is taken at the end, where the fixed lock order puts it.
        let unlocked_now = crate::qr::database_now(pool).await?;
        let nonce = self
            .qr
            .resolve(
                pool,
                request.scan.presented()?,
                AUDIENCE_STAMP,
                unlocked_now,
            )
            .await?;

        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        // Step 3, in the §13.1 lock order.
        self.stores.lock_store(&mut tx, store.id).await?;
        let policy = self.policies.active_for_accrual(&mut tx, store.id, now).await?;
        let customer_id = self
            .lock_store_customer(&mut tx, store.id, nonce.user_id)
            .await?;

        // The nonce is validated again now that the store lock serialises us against any
        // competing scan, so a QR that has just been spent is reported as spent rather
        // than tripping over some later rule first (§15).
        self.qr.ensure_unconsumed(&mut tx, nonce.nonce_id, now).await?;

        // Step 4.
        let lines = self
            .catalog
            .resolve_order_lines(&mut *tx, store.id, &request.order.items)
            .await?;
        let assessment = self
            .assess(
                &mut tx,
                store,
                &policy,
                nonce.user_id,
                &request.order,
                &lines,
                now,
            )
            .await?;

        if let Some(blocker) = assessment.blockers.first() {
            return Err(blocker_to_error(blocker));
        }
        // STAMP-003: a near-duplicate only goes through when the owner has said it is a
        // separate order, and has a distinct reference to back that up.
        if assessment.duplicate_of.is_some()
            && !(request.acknowledge_duplicate && assessment.has_distinct_order_ref)
        {
            return Err(ApiError::new(ErrorCode::DuplicateTransactionSuspected).internal(format!(
                "duplicate of {:?}",
                assessment.duplicate_of
            )));
        }

        // Step 5.
        let transaction_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.stamp_transactions
                (store_id, user_id, policy_id, business_day, quantity, external_order_ref,
                 order_snapshot, status, approved_by_user_id, confirmed_at, idempotency_key)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'CONFIRMED', $8, $9, $10)
            RETURNING id
            "#,
            store.id,
            nonce.user_id,
            policy.id,
            assessment.business_day,
            i16::try_from(assessment.quantity).unwrap_or(i16::MAX),
            request.order.external_order_ref.as_deref(),
            order_snapshot(request, &lines, &assessment),
            owner_user_id,
            now,
            idempotency_key,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(map_transaction_conflict)?;

        let lot_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.stamp_lots
                (store_id, user_id, policy_id, source_transaction_id, original_quantity,
                 earned_at, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id
            "#,
            store.id,
            nonce.user_id,
            policy.id,
            transaction_id,
            i16::try_from(assessment.quantity).unwrap_or(i16::MAX),
            now,
            assessment.stamps_expire_at,
        )
        .fetch_one(&mut *tx)
        .await?;

        self.append_ledger(
            &mut tx,
            lot_id,
            "EARN",
            assessment.quantity,
            Some(transaction_id),
            None,
            owner_user_id,
            "STAMP_EARNED",
            serde_json::json!({ "business_day": assessment.business_day }),
        )
        .await?;

        // Step 6: lots, locked in expiry order so a concurrent reversal cannot reorder
        // them underneath the plan.
        let lots = self
            .load_lot_balances(&mut tx, store.id, nonce.user_id, true)
            .await?;
        let target = i64::from(policy.rules.target_stamp_count);
        let plan = plan_reward_consumption(&lots, target, now);

        // Step 7: each reward coupon and the ledger rows that paid for it, in the same
        // transaction. STAMP-004 forbids a state where stamps vanished but no coupon
        // exists, and a single transaction is what guarantees that.
        let mut issued_rewards = Vec::new();
        for draw in &plan {
            let reward = self
                .issue_reward_coupon(&mut tx, store, &policy, nonce.user_id, transaction_id, now)
                .await?;

            for (draw_lot_id, quantity) in &draw.draws {
                self.append_ledger(
                    &mut tx,
                    *draw_lot_id,
                    "CONSUME_FOR_REWARD",
                    -quantity,
                    Some(transaction_id),
                    Some(reward.coupon_id),
                    owner_user_id,
                    "LOYALTY_GOAL_REACHED",
                    serde_json::json!({ "target": target }),
                )
                .await?;
            }

            issued_rewards.push(reward);
        }

        // Step 8: the nonce, last in the lock order, and consumed conditionally so a
        // concurrent scan of the same QR loses here rather than double-crediting.
        self.qr.lock(&mut tx, nonce.nonce_id, now).await?;
        self.qr
            .consume(&mut tx, nonce.nonce_id, "STAMP", transaction_id, now)
            .await?;

        self.touch_customer(&mut tx, customer_id, now, assessment.self_transaction)
            .await?;

        self.publish_outbox(
            &mut tx,
            "stamp_transaction",
            transaction_id,
            1,
            "STAMP_EARNED",
            serde_json::json!({
                "store_id": store.id,
                "user_id": nonce.user_id,
                "quantity": assessment.quantity,
                "expires_at": assessment.stamps_expire_at,
            }),
        )
        .await?;
        for reward in &issued_rewards {
            self.publish_outbox(
                &mut tx,
                "coupon_instance",
                reward.coupon_id,
                1,
                "REWARD_ISSUED",
                serde_json::json!({
                    "store_id": store.id,
                    "user_id": nonce.user_id,
                    "expires_at": reward.expires_at,
                    "source_transaction_id": transaction_id,
                }),
            )
            .await?;
        }

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::StoreOwner,
                    "stamp_transaction.confirmed",
                    "stamp_transaction",
                )
                .actor(owner_user_id)
                .resource(transaction_id)
                .store(store.id)
                .metadata(serde_json::json!({
                    "quantity": assessment.quantity,
                    "rewards_issued": issued_rewards.len(),
                    "business_day": assessment.business_day,
                    "self_transaction": assessment.self_transaction,
                })),
            )
            .await?;

        let board = self
            .load_board(&mut tx, store.id, nonce.user_id, &policy, now, true)
            .await?;
        let customer = self.describe_customer(&mut tx, store.id, nonce.user_id).await?;

        // Step 9. The idempotent response itself is stored by the middleware once this
        // returns, so the commit is the last thing that happens here.
        tx.commit().await?;

        tracing::info!(
            %transaction_id,
            store_id = %store.id,
            quantity = assessment.quantity,
            rewards = issued_rewards.len(),
            "loyalty.stamp_confirmed"
        );

        Ok(StampTransaction {
            transaction_id,
            store_id: store.id,
            status: StampTransactionStatus::Confirmed,
            business_day: assessment.business_day,
            quantity: assessment.quantity,
            external_order_ref: request.order.external_order_ref.clone(),
            confirmed_at: now,
            voided_at: None,
            customer,
            stamp_board: board,
            issued_rewards,
            revoked_reward_ids: Vec::new(),
            stamps_expire_at: Some(assessment.stamps_expire_at),
            voidable_until: Some(now + self.void_window),
        })
    }

    /// `POST /owner/stamp-transactions/:id/void` (STAMP-007, §13.4).
    ///
    /// Never deletes anything: the original accrual stays, and the reversal is expressed
    /// as opposing ledger rows. Anything that cannot be reversed safely stops and asks for
    /// an administrator rather than guessing.
    pub async fn void(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        transaction_id: Uuid,
        request: &VoidStampRequest,
    ) -> ApiResult<StampTransaction> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        // Same lock order as accrual, so the two serialise instead of deadlocking.
        self.stores.lock_store(&mut tx, store.id).await?;

        let original = sqlx::query!(
            r#"
            SELECT id, user_id, policy_id, business_day, quantity, external_order_ref,
                   status::text AS "status!", confirmed_at
            FROM coupon.stamp_transactions
            WHERE id = $1 AND store_id = $2
            FOR UPDATE
            "#,
            transaction_id,
            store.id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::TransactionNotFound))?;

        if StampTransactionStatus::from_db(&original.status) != StampTransactionStatus::Confirmed {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "이미 취소되었거나 관리자 확인이 필요한 거래입니다.",
            ));
        }
        // §8.6: 적립 취소 24시간.
        if now > original.confirmed_at + self.void_window {
            return Err(ApiError::new(ErrorCode::VoidWindowExpired));
        }

        let customer_id = self
            .lock_store_customer(&mut tx, store.id, original.user_id)
            .await?;

        // Every stamp this transaction spent on a reward, so the reversal can put each
        // one back on the lot it came from.
        let consumed = sqlx::query!(
            r#"
            SELECT lot_id, quantity_delta, reward_coupon_id
            FROM coupon.stamp_ledger
            WHERE source_stamp_transaction_id = $1 AND event_type = 'CONSUME_FOR_REWARD'
            ORDER BY occurred_at, id
            "#,
            transaction_id,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut reward_ids: Vec<Uuid> = consumed.iter().filter_map(|row| row.reward_coupon_id).collect();
        reward_ids.sort_unstable();
        reward_ids.dedup();

        // Take back the rewards first: if any of them cannot be taken back, nothing else
        // should have happened (STAMP-007).
        for reward_id in &reward_ids {
            self.revoke_reward_for_void(&mut tx, *reward_id, owner_user_id, transaction_id, now)
                .await?;
        }

        for row in &consumed {
            self.append_ledger(
                &mut tx,
                row.lot_id,
                "VOID",
                i64::from(-row.quantity_delta),
                Some(transaction_id),
                None,
                owner_user_id,
                "STAMP_TRANSACTION_VOIDED_RESTORE",
                serde_json::json!({ "restored_for": transaction_id }),
            )
            .await?;
        }

        // Now undo the accrual itself. The lot must still hold everything it earned —
        // if a later transaction already spent some of it, reversing would drive the lot
        // negative, and §12.6-3 says that must not happen. That is an administrator's
        // call, not a self-service one.
        let lot = sqlx::query!(
            r#"
            SELECT
                b.lot_id AS "lot_id!",
                b.balance AS "balance!",
                b.original_quantity AS "original_quantity!",
                b.expires_at AS "expires_at!"
            FROM coupon.stamp_lot_balances b
            WHERE b.source_transaction_id = $1
            "#,
            transaction_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            ApiError::new(ErrorCode::RequiresAdminReview)
                .internal("the accrual has no lot to reverse")
        })?;

        let earned = i64::from(lot.original_quantity);
        if lot.balance < earned {
            self.flag_for_admin_review(&mut tx, transaction_id).await?;
            tx.commit().await?;
            return Err(ApiError::with_message(
                ErrorCode::RequiresAdminReview,
                "이 적립의 도장이 이미 사용되어 점주가 취소할 수 없습니다.",
            ));
        }

        self.append_ledger(
            &mut tx,
            lot.lot_id,
            "VOID",
            -earned,
            Some(transaction_id),
            None,
            owner_user_id,
            "STAMP_TRANSACTION_VOIDED",
            serde_json::json!({ "reason": request.reason }),
        )
        .await?;

        sqlx::query!(
            r#"
            UPDATE coupon.stamp_transactions
            SET status = 'VOIDED', voided_at = $2, voided_by_user_id = $3, void_reason = $4
            WHERE id = $1 AND status = 'CONFIRMED'
            "#,
            transaction_id,
            now,
            owner_user_id,
            request.reason.as_deref(),
        )
        .execute(&mut *tx)
        .await?;

        self.publish_outbox(
            &mut tx,
            "stamp_transaction",
            transaction_id,
            2,
            "TRANSACTION_VOIDED",
            serde_json::json!({
                "store_id": store.id,
                "user_id": original.user_id,
                "restored_stamps": earned,
                "revoked_rewards": reward_ids,
            }),
        )
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::StoreOwner,
                    "stamp_transaction.voided",
                    "stamp_transaction",
                )
                .actor(owner_user_id)
                .resource(transaction_id)
                .store(store.id)
                .reason(request.reason.clone().unwrap_or_else(|| "점주 취소".to_owned()))
                .transition(
                    &serde_json::json!({ "status": "CONFIRMED" }),
                    &serde_json::json!({ "status": "VOIDED" }),
                )
                .metadata(serde_json::json!({ "revoked_rewards": reward_ids })),
            )
            .await?;

        let policy = self
            .policies
            .load_one(&mut tx, store.id, original.policy_id)
            .await?;
        let board = self
            .load_board(&mut tx, store.id, original.user_id, &policy, now, true)
            .await?;
        let customer = self
            .describe_customer(&mut tx, store.id, original.user_id)
            .await?;
        self.touch_customer(&mut tx, customer_id, now, false).await?;

        tx.commit().await?;

        tracing::info!(%transaction_id, store_id = %store.id, "loyalty.stamp_voided");

        Ok(StampTransaction {
            transaction_id,
            store_id: store.id,
            status: StampTransactionStatus::Voided,
            business_day: original.business_day,
            quantity: i64::from(original.quantity),
            external_order_ref: original.external_order_ref,
            confirmed_at: original.confirmed_at,
            voided_at: Some(now),
            customer,
            stamp_board: board,
            issued_rewards: Vec::new(),
            revoked_reward_ids: reward_ids,
            stamps_expire_at: Some(lot.expires_at),
            voidable_until: None,
        })
    }

    /// Reverse an accrual on an administrator's authority (ADMIN-003, §13.4).
    ///
    /// The owner's own reversal ([`StampService::void`]) refuses anything it cannot undo
    /// safely and hands it to an administrator. This is the other side of that door — it
    /// runs past the owner's 24-hour window and past a spent reward — but it is still not
    /// an edit: every change is a new ledger event, and a lot is never driven below zero
    /// (§12.6-3, enforced by `trg_stamp_ledger_balance` regardless of what is asked here).
    ///
    /// Returns the number of ledger events appended.
    pub async fn admin_void_transaction(
        &self,
        tx: &mut Tx<'_>,
        transaction_id: Uuid,
        admin_user_id: Uuid,
        adjustment_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<i64> {
        let original = sqlx::query!(
            r#"
            SELECT id, store_id, user_id, status::text AS "status!"
            FROM coupon.stamp_transactions
            WHERE id = $1
            FOR UPDATE
            "#,
            transaction_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::TransactionNotFound))?;

        if StampTransactionStatus::from_db(&original.status) == StampTransactionStatus::Voided {
            // Already reversed. Re-running the job must not reverse it twice.
            return Ok(0);
        }

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

        let mut appended = 0i64;

        for row in &consumed {
            self.append_ledger_as(
                tx,
                "SYSTEM_ADMIN",
                row.lot_id,
                "ADMIN_ADJUSTMENT",
                i64::from(-row.quantity_delta),
                Some(transaction_id),
                None,
                Some(admin_user_id),
                "ADMIN_ADJUSTMENT_RESTORE",
                serde_json::json!({ "adjustment_id": adjustment_id }),
            )
            .await?;
            appended += 1;
        }

        // Rewards this accrual produced. An unused one is taken back; a spent one is left
        // alone and recorded — the customer already had the benefit, and clawing it back
        // would be a second, separate decision.
        let mut reward_ids: Vec<Uuid> = consumed
            .iter()
            .filter_map(|row| row.reward_coupon_id)
            .collect();
        reward_ids.sort_unstable();
        reward_ids.dedup();

        for reward_id in &reward_ids {
            let changed = sqlx::query!(
                r#"
                UPDATE coupon.coupon_instances
                SET status = 'REVOKED', revoked_at = $2, revocation_reason = $3
                WHERE id = $1 AND status IN ('AVAILABLE', 'RESERVED')
                "#,
                reward_id,
                now,
                "관리자 보정으로 회수됨",
            )
            .execute(&mut **tx)
            .await?;

            if changed.rows_affected() == 1 {
                sqlx::query!(
                    r#"
                    INSERT INTO coupon.coupon_status_events
                        (coupon_id, from_status, to_status, actor_type, actor_user_id,
                         reason_code, transaction_id, metadata, occurred_at)
                    SELECT $1,
                           (SELECT to_status FROM coupon.coupon_status_events
                            WHERE coupon_id = $1 ORDER BY occurred_at DESC, id DESC LIMIT 1),
                           'REVOKED', 'SYSTEM_ADMIN', $2, 'ADMIN_ADJUSTMENT', $3, $4, $5
                    "#,
                    reward_id,
                    admin_user_id,
                    transaction_id,
                    serde_json::json!({ "adjustment_id": adjustment_id }),
                    now,
                )
                .execute(&mut **tx)
                .await?;
            }
        }

        // Now the accrual itself.
        let lot = sqlx::query!(
            r#"
            SELECT b.lot_id AS "lot_id!", b.balance AS "balance!",
                   b.original_quantity AS "original_quantity!"
            FROM coupon.stamp_lot_balances b
            WHERE b.source_transaction_id = $1
            "#,
            transaction_id,
        )
        .fetch_optional(&mut **tx)
        .await?;

        if let Some(lot) = lot {
            // Take back only what is still there. Removing more would drive the lot
            // negative, which §12.6-3 forbids and the trigger would refuse anyway — and
            // an administrator's correction should not fail on arithmetic we can do here.
            let removable = i64::from(lot.original_quantity).min(lot.balance);
            if removable > 0 {
                self.append_ledger_as(
                    tx,
                    "SYSTEM_ADMIN",
                    lot.lot_id,
                    "ADMIN_ADJUSTMENT",
                    -removable,
                    Some(transaction_id),
                    None,
                    Some(admin_user_id),
                    "ADMIN_TRANSACTION_VOID",
                    serde_json::json!({
                        "adjustment_id": adjustment_id,
                        "requested": lot.original_quantity,
                        "removed": removable,
                    }),
                )
                .await?;
                appended += 1;
            }
        }

        sqlx::query!(
            r#"
            UPDATE coupon.stamp_transactions
            SET status = 'VOIDED', voided_at = $2, voided_by_user_id = $3, void_reason = $4
            WHERE id = $1 AND status <> 'VOIDED'
            "#,
            transaction_id,
            now,
            admin_user_id,
            "관리자 보정",
        )
        .execute(&mut **tx)
        .await?;

        Ok(appended)
    }

    /// Put stamps on a customer's board without an accrual behind them (ADMIN-004 도장
    /// 보정).
    ///
    /// A grant creates a *new* lot rather than editing an existing one, which is what
    /// keeps the ledger reconstructible: the correction is visible as its own event with
    /// its own expiry rather than hidden inside somebody else's earlier accrual.
    #[allow(clippy::too_many_arguments)]
    pub async fn admin_grant_stamps(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        user_id: Uuid,
        quantity: i64,
        admin_user_id: Uuid,
        adjustment_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<i64> {
        if quantity <= 0 {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "보정 수량은 1개 이상이어야 합니다.",
            ));
        }

        // Re-running the job must not grant twice. The ledger event carries the
        // adjustment id, so "has this correction already been applied" is a question the
        // ledger itself answers.
        let already = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.stamp_ledger
            WHERE reason_code = 'ADMIN_STAMP_GRANT'
              AND metadata ->> 'adjustment_id' = $1
            LIMIT 1
            "#,
            adjustment_id.to_string(),
        )
        .fetch_optional(&mut **tx)
        .await?;

        if already.is_some() {
            return Ok(0);
        }

        let policy = self.policies.active_for_accrual(tx, store_id, now).await?;

        let lot_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.stamp_lots
                (store_id, user_id, policy_id, original_quantity, earned_at, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
            "#,
            store_id,
            user_id,
            policy.id,
            i16::try_from(quantity).unwrap_or(i16::MAX),
            now,
            now + policy.rules.stamp_validity(),
        )
        .fetch_one(&mut **tx)
        .await?;

        self.append_ledger_as(
            tx,
            "SYSTEM_ADMIN",
            lot_id,
            "ADMIN_ADJUSTMENT",
            quantity,
            None,
            None,
            Some(admin_user_id),
            "ADMIN_STAMP_GRANT",
            serde_json::json!({ "adjustment_id": adjustment_id }),
        )
        .await?;

        Ok(1)
    }

    /// Write `EXPIRE` rows for lots whose moment has passed (STAMP-006, §18.1).
    ///
    /// Housekeeping only. Every read already treats an expired lot as spent, so a late
    /// run cannot let anyone spend a stamp they should not have.
    pub async fn expire_due_lots(
        &self,
        pool: &PgPool,
        now: DateTime<Utc>,
        batch: i64,
    ) -> ApiResult<u64> {
        let due = sqlx::query!(
            r#"
            SELECT lot_id AS "lot_id!", balance AS "balance!"
            FROM coupon.stamp_lot_balances
            WHERE expires_at <= $1 AND balance > 0
            ORDER BY expires_at
            LIMIT $2
            "#,
            now,
            batch,
        )
        .fetch_all(pool)
        .await?;

        let mut expired = 0u64;
        for lot in due {
            let mut tx = pool.begin().await?;

            // Re-read under the lock: the balance may have moved since the scan above.
            let balance = sqlx::query_scalar!(
                r#"
                SELECT b.balance AS "balance!"
                FROM coupon.stamp_lot_balances b
                JOIN coupon.stamp_lots l ON l.id = b.lot_id
                WHERE b.lot_id = $1 AND b.expires_at <= $2
                FOR UPDATE OF l
                "#,
                lot.lot_id,
                now,
            )
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or(0);

            if balance > 0 {
                self.append_ledger(
                    &mut tx,
                    lot.lot_id,
                    "EXPIRE",
                    -balance,
                    None,
                    None,
                    None,
                    "STAMP_EXPIRED",
                    serde_json::json!({ "expired_at": now }),
                )
                .await?;
                expired += 1;
            }

            tx.commit().await?;
        }

        Ok(expired)
    }

    /// The customer's board, for the wallet and for both apps' scan screens.
    pub async fn load_board(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        user_id: Uuid,
        policy: &LoyaltyPolicy,
        now: DateTime<Utc>,
        locked: bool,
    ) -> ApiResult<StampBoard> {
        let lots = self.load_lot_balances(tx, store_id, user_id, locked).await?;
        Ok(build_board(&lots, i64::from(policy.rules.target_stamp_count), now))
    }

    async fn load_lot_balances(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        user_id: Uuid,
        locked: bool,
    ) -> ApiResult<Vec<LotBalance>> {
        // `FOR UPDATE OF l` locks the lot rows, not the ledger the view aggregates — the
        // ledger is append-only and has nothing to lock.
        if locked {
            Ok(sqlx::query_as!(
                LotBalance,
                r#"
                SELECT
                    b.lot_id AS "lot_id!",
                    b.balance AS "balance!",
                    b.expires_at AS "expires_at!"
                FROM coupon.stamp_lot_balances b
                JOIN coupon.stamp_lots l ON l.id = b.lot_id
                WHERE b.store_id = $1 AND b.user_id = $2
                ORDER BY b.expires_at, b.lot_id
                FOR UPDATE OF l
                "#,
                store_id,
                user_id,
            )
            .fetch_all(&mut **tx)
            .await?)
        } else {
            Ok(sqlx::query_as!(
                LotBalance,
                r#"
                SELECT
                    b.lot_id AS "lot_id!",
                    b.balance AS "balance!",
                    b.expires_at AS "expires_at!"
                FROM coupon.stamp_lot_balances b
                WHERE b.store_id = $1 AND b.user_id = $2
                ORDER BY b.expires_at, b.lot_id
                "#,
                store_id,
                user_id,
            )
            .fetch_all(&mut **tx)
            .await?)
        }
    }

    /// Everything `preview` and `confirm` both need to decide.
    #[allow(clippy::too_many_arguments)]
    async fn assess(
        &self,
        tx: &mut Tx<'_>,
        store: &OwnedStore,
        policy: &LoyaltyPolicy,
        user_id: Uuid,
        order: &OrderInput,
        lines: &[OrderLine],
        now: DateTime<Utc>,
    ) -> ApiResult<Assessment> {
        let calendar = store.calendar()?;
        let business_day = calendar.business_day(now);
        let quantity = i64::from(policy.rules.stamps_per_order);

        let mut blockers = Vec::new();
        let mut warnings = Vec::new();

        if order.currency != "KRW" {
            blockers.push(PreviewIssue {
                code: "UNSUPPORTED_CURRENCY".to_owned(),
                message: "원화 주문만 처리할 수 있습니다.".to_owned(),
            });
        }

        let eligibility = policy.rules.restriction().evaluate(lines, order.gross_amount);
        if let Some(rejection) = eligibility.rejection() {
            blockers.push(PreviewIssue {
                code: rejection.code.as_str().to_owned(),
                message: rejection.message,
            });
        }

        // The minimum is measured against what actually qualifies, so a restricted policy
        // cannot be satisfied by padding the bill with excluded items.
        if eligibility.eligible_amount < policy.rules.minimum_order_amount {
            blockers.push(PreviewIssue {
                code: ErrorCode::MinimumOrderNotMet.as_str().to_owned(),
                message: format!(
                    "최소 주문 금액까지 {}원 부족합니다.",
                    policy.rules.minimum_order_amount - eligibility.eligible_amount
                ),
            });
        }

        let daily_used = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM coupon.stamp_transactions
            WHERE store_id = $1 AND user_id = $2 AND business_day = $3 AND status = 'CONFIRMED'
            "#,
            store.id,
            user_id,
            business_day,
        )
        .fetch_one(&mut **tx)
        .await?;

        let daily_limit = policy.rules.daily_earning_limit.map(i64::from);
        let next_business_day_start = calendar.next_business_day_start(now);

        if let Some(limit) = daily_limit
            && daily_used >= limit
        {
            blockers.push(PreviewIssue {
                code: ErrorCode::DailyLimitExceeded.as_str().to_owned(),
                message: format!(
                    "오늘은 이미 {daily_used}회 적립했습니다. 다음 영업일은 {} 부터입니다.",
                    next_business_day_start.to_rfc3339()
                ),
            });
        }

        // STAMP-003: same customer, same amount, inside the policy's window.
        let duplicate = sqlx::query!(
            r#"
            SELECT id, external_order_ref, confirmed_at
            FROM coupon.stamp_transactions
            WHERE store_id = $1 AND user_id = $2 AND status = 'CONFIRMED'
              AND confirmed_at > $3
              AND (order_snapshot -> 'gross_amount')::bigint = $4
            ORDER BY confirmed_at DESC
            LIMIT 1
            "#,
            store.id,
            user_id,
            now - policy.rules.duplicate_window(),
            order.gross_amount,
        )
        .fetch_optional(&mut **tx)
        .await?;

        let has_distinct_order_ref = match (&duplicate, order.external_order_ref.as_deref()) {
            (Some(previous), Some(reference)) => {
                previous.external_order_ref.as_deref() != Some(reference)
            }
            _ => false,
        };

        if let Some(previous) = &duplicate {
            warnings.push(PreviewIssue {
                code: ErrorCode::DuplicateTransactionSuspected.as_str().to_owned(),
                message: format!(
                    "{} 에 같은 금액의 적립이 있었습니다. 별도 주문이면 주문번호를 입력해 주세요.",
                    previous.confirmed_at.to_rfc3339()
                ),
            });
        }

        let owner_user_id = self.owner_of(tx, store.id).await?;
        let self_transaction = owner_user_id == user_id;
        if self_transaction {
            // SEC-004: allowed, but never silent.
            warnings.push(PreviewIssue {
                code: "SELF_TRANSACTION".to_owned(),
                message: "점주 본인 계정에 대한 적립입니다. 위험 집계에 포함됩니다.".to_owned(),
            });
        }

        Ok(Assessment {
            business_day,
            quantity,
            eligible_amount: eligibility.eligible_amount,
            stamps_expire_at: now + policy.rules.stamp_validity(),
            daily_used,
            daily_limit,
            next_business_day_start,
            self_transaction,
            duplicate_of: duplicate.map(|row| row.id),
            has_distinct_order_ref,
            warnings,
            blockers,
        })
    }

    async fn owner_of(&self, tx: &mut Tx<'_>, store_id: Uuid) -> ApiResult<Uuid> {
        Ok(sqlx::query_scalar!(
            "SELECT owner_user_id FROM coupon.stores WHERE id = $1",
            store_id,
        )
        .fetch_one(&mut **tx)
        .await?)
    }

    /// Create the store-local customer record if this is a first visit, then lock it.
    ///
    /// This is the §13.1 `user/store_customer` lock: holding it serialises everything one
    /// customer does in one store, which is what makes the daily-limit count read above
    /// safe to act on.
    async fn lock_store_customer(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        user_id: Uuid,
    ) -> ApiResult<Uuid> {
        sqlx::query!(
            r#"
            INSERT INTO coupon.store_customers (store_id, user_id, alias)
            VALUES ($1, $2, $3)
            ON CONFLICT (store_id, user_id) DO NOTHING
            "#,
            store_id,
            user_id,
            self.derive_alias(store_id, user_id),
        )
        .execute(&mut **tx)
        .await?;

        Ok(sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.store_customers
            WHERE store_id = $1 AND user_id = $2
            FOR UPDATE
            "#,
            store_id,
            user_id,
        )
        .fetch_one(&mut **tx)
        .await?)
    }

    async fn touch_customer(
        &self,
        tx: &mut Tx<'_>,
        customer_id: Uuid,
        now: DateTime<Utc>,
        self_transaction: bool,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.store_customers
            SET last_seen_at = $2,
                interaction_count = interaction_count + 1,
                risk_flags = CASE
                    WHEN $3 AND NOT (risk_flags @> '["SELF_TRANSACTION"]'::jsonb)
                        THEN risk_flags || '["SELF_TRANSACTION"]'::jsonb
                    ELSE risk_flags
                END
            WHERE id = $1
            "#,
            customer_id,
            now,
            self_transaction,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// The pseudonymous view of a customer (WALLET-005).
    async fn describe_customer(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        user_id: Uuid,
    ) -> ApiResult<PseudonymousCustomer> {
        let row = sqlx::query!(
            r#"
            SELECT
                c.id AS "store_customer_id?",
                c.alias AS "alias?",
                c.first_seen_at AS "first_seen_at?",
                c.last_seen_at AS "last_seen_at?",
                c.interaction_count AS "interaction_count?",
                u.display_name
            FROM coupon.users u
            LEFT JOIN coupon.store_customers c
                   ON c.user_id = u.id AND c.store_id = $1
            WHERE u.id = $2
            "#,
            store_id,
            user_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::UserNotFound))?;

        Ok(PseudonymousCustomer {
            store_customer_id: row.store_customer_id,
            alias: row
                .alias
                .unwrap_or_else(|| self.derive_alias(store_id, user_id)),
            masked_name: mask_display_name(&row.display_name),
            is_new_customer: row.store_customer_id.is_none(),
            first_seen_at: row.first_seen_at,
            last_seen_at: row.last_seen_at,
            visit_count: row.interaction_count.unwrap_or(0),
        })
    }

    /// A store-local nickname that is stable but says nothing about the person.
    ///
    /// Derived from a keyed hash of the pair, so the same customer keeps one nickname in
    /// one store and gets an unrelated one in the next (§3.1).
    fn derive_alias(&self, store_id: Uuid, user_id: Uuid) -> String {
        let digest = self
            .lookup_hash
            .hash("store-customer-alias", &format!("{store_id}:{user_id}"));
        // Unambiguous alphabet: no 0/O or 1/I, because owners read these aloud.
        const ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
        let suffix: String = digest
            .iter()
            .take(4)
            .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
            .collect();
        format!("손님-{suffix}")
    }

    /// Create one reward coupon for a completed board.
    async fn issue_reward_coupon(
        &self,
        tx: &mut Tx<'_>,
        store: &OwnedStore,
        policy: &LoyaltyPolicy,
        user_id: Uuid,
        transaction_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<IssuedReward> {
        let reward_definition_id = sqlx::query_scalar!(
            "SELECT id FROM coupon.loyalty_reward_definitions WHERE policy_id = $1",
            policy.id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            ApiError::with_message(
                ErrorCode::NoActivePolicy,
                "정책에 리워드가 설정되어 있지 않습니다.",
            )
        })?;

        // `coupon_instances.title`/`description` are NOT NULL and the customer reads them,
        // so fall back rather than write a blank card. Publishing already demands both
        // (STAMP-001); this only covers a policy published before that rule existed.
        let title = non_empty(&policy.reward.title)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{} 리워드", policy.name));
        let description = non_empty(&policy.reward.description)
            .unwrap_or("도장 적립 보상으로 발급된 쿠폰입니다.")
            .to_owned();
        let expires_at = now + chrono::Duration::days(i64::from(policy.reward.validity_days));

        // The snapshot is what the coupon means for the rest of its life. A later policy
        // version cannot reach back and change it (§8.5, product principle 4).
        let condition_snapshot = serde_json::json!({
            "schema_version": 1,
            "source": "LOYALTY_REWARD",
            "store": { "id": store.id, "name": store.name },
            "policy": {
                "id": policy.id,
                "version_no": policy.version_no,
                "target_stamp_count": policy.rules.target_stamp_count,
            },
            "reward": policy.reward,
            "issued_for_transaction_id": transaction_id,
            "issued_at": now,
        });

        let coupon_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.coupon_instances
                (store_id, user_id, source_type, reward_definition_id, status, title,
                 description, benefit_type, usable_from, expires_at, condition_snapshot,
                 issued_at)
            VALUES ($1, $2, 'LOYALTY_REWARD', $3, 'AVAILABLE', $4, $5,
                    $6::text::coupon.benefit_type, $7, $8, $9, $7)
            RETURNING id
            "#,
            store.id,
            user_id,
            reward_definition_id,
            title,
            description,
            policy.reward.benefit_type.as_db(),
            now,
            expires_at,
            condition_snapshot,
        )
        .fetch_one(&mut **tx)
        .await?;

        // §12.6-8: the event records the transition that just happened, and a trigger
        // checks it against the instance's live status.
        sqlx::query!(
            r#"
            INSERT INTO coupon.coupon_status_events
                (coupon_id, from_status, to_status, actor_type, reason_code, transaction_id,
                 metadata, occurred_at)
            VALUES ($1, NULL, 'AVAILABLE', 'SYSTEM', 'LOYALTY_GOAL_REACHED', $2, $3, $4)
            "#,
            coupon_id,
            transaction_id,
            serde_json::json!({ "policy_id": policy.id }),
            now,
        )
        .execute(&mut **tx)
        .await?;

        Ok(IssuedReward {
            coupon_id,
            title,
            description,
            usable_from: now,
            expires_at,
        })
    }

    /// Take a reward back as part of a reversal, following STAMP-007's table.
    async fn revoke_reward_for_void(
        &self,
        tx: &mut Tx<'_>,
        coupon_id: Uuid,
        owner_user_id: Uuid,
        transaction_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        let coupon = sqlx::query!(
            r#"
            SELECT status::text AS "status!"
            FROM coupon.coupon_instances
            WHERE id = $1
            FOR UPDATE
            "#,
            coupon_id,
        )
        .fetch_one(&mut **tx)
        .await?;

        match coupon.status.as_str() {
            "AVAILABLE" => {}
            // The customer already spent it; unwinding that is a support case, not a
            // button the owner gets to press.
            "USED" => {
                return Err(ApiError::with_message(
                    ErrorCode::RequiresAdminReview,
                    "이미 사용된 리워드가 있어 점주가 취소할 수 없습니다.",
                ));
            }
            "RESERVED" => {
                return Err(ApiError::with_message(
                    ErrorCode::RequiresAdminReview,
                    "사용 예약 중인 리워드가 있어 점주가 취소할 수 없습니다.",
                ));
            }
            // STAMP-007: expired or already revoked rewards do not get their stamps put
            // back without an administrator looking at it.
            other => {
                return Err(ApiError::with_message(
                    ErrorCode::RequiresAdminReview,
                    "리워드 상태가 변경되어 점주가 취소할 수 없습니다.",
                )
                .internal(format!("reward coupon {coupon_id} is {other}")));
            }
        }

        sqlx::query!(
            r#"
            UPDATE coupon.coupon_instances
            SET status = 'REVOKED', revoked_at = $2, revocation_reason = $3
            WHERE id = $1 AND status = 'AVAILABLE'
            "#,
            coupon_id,
            now,
            "적립 취소로 회수됨",
        )
        .execute(&mut **tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.coupon_status_events
                (coupon_id, from_status, to_status, actor_type, actor_user_id, reason_code,
                 transaction_id, metadata, occurred_at)
            VALUES ($1, 'AVAILABLE', 'REVOKED', 'STORE_OWNER', $2, 'STAMP_TRANSACTION_VOIDED',
                    $3, '{}'::jsonb, $4)
            "#,
            coupon_id,
            owner_user_id,
            transaction_id,
            now,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    async fn flag_for_admin_review(&self, tx: &mut Tx<'_>, transaction_id: Uuid) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.stamp_transactions
            SET status = 'REQUIRES_ADMIN_REVIEW', voided_at = clock_timestamp()
            WHERE id = $1 AND status = 'CONFIRMED'
            "#,
            transaction_id,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn append_ledger(
        &self,
        tx: &mut Tx<'_>,
        lot_id: Uuid,
        event_type: &str,
        quantity_delta: i64,
        source_transaction_id: Option<Uuid>,
        reward_coupon_id: Option<Uuid>,
        actor_user_id: impl Into<Option<Uuid>>,
        reason_code: &str,
        metadata: serde_json::Value,
    ) -> ApiResult<()> {
        let actor_user_id = actor_user_id.into();
        let actor_type = if actor_user_id.is_some() {
            "STORE_OWNER"
        } else {
            "SYSTEM"
        };

        self.append_ledger_as(
            tx,
            actor_type,
            lot_id,
            event_type,
            quantity_delta,
            source_transaction_id,
            reward_coupon_id,
            actor_user_id,
            reason_code,
            metadata,
        )
        .await
    }

    /// The same, for an actor the owner-or-system guess would get wrong — an
    /// administrator applying a correction (ADMIN-003) is neither.
    #[allow(clippy::too_many_arguments)]
    async fn append_ledger_as(
        &self,
        tx: &mut Tx<'_>,
        actor_type: &str,
        lot_id: Uuid,
        event_type: &str,
        quantity_delta: i64,
        source_transaction_id: Option<Uuid>,
        reward_coupon_id: Option<Uuid>,
        actor_user_id: Option<Uuid>,
        reason_code: &str,
        metadata: serde_json::Value,
    ) -> ApiResult<()> {
        let delta = i16::try_from(quantity_delta).map_err(|_| {
            ApiError::new(ErrorCode::UnprocessableRequest)
                .internal(format!("ledger delta {quantity_delta} does not fit"))
        })?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.stamp_ledger
                (lot_id, event_type, quantity_delta, source_stamp_transaction_id,
                 reward_coupon_id, actor_type, actor_user_id, reason_code, metadata)
            VALUES ($1, $2::text::coupon.stamp_ledger_event_type, $3, $4, $5,
                    $6::text::coupon.actor_type, $7, $8, $9)
            "#,
            lot_id,
            event_type,
            delta,
            source_transaction_id,
            reward_coupon_id,
            actor_type,
            actor_user_id,
            reason_code,
            metadata,
        )
        .execute(&mut **tx)
        .await
        .map_err(|error| match &error {
            // §12.6-3, enforced by `trg_stamp_ledger_balance`. Reaching it means the plan
            // and the ledger disagreed, which must never quietly become a 500.
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23514") => {
                ApiError::with_message(
                    ErrorCode::RequiresAdminReview,
                    "도장 잔량이 맞지 않아 처리할 수 없습니다.",
                )
                .internal(db.to_string())
            }
            _ => ApiError::from(error),
        })?;

        Ok(())
    }

    /// Record a domain event for the worker to pick up (§14.2).
    ///
    /// Written in the same transaction as the change, so a message can be late but never
    /// describes something that did not happen.
    async fn publish_outbox(
        &self,
        tx: &mut Tx<'_>,
        aggregate_type: &str,
        aggregate_id: Uuid,
        aggregate_version: i64,
        event_type: &str,
        payload: serde_json::Value,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO coupon.outbox_events
                (aggregate_type, aggregate_id, aggregate_version, event_type, correlation_id, payload)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (aggregate_type, aggregate_id, aggregate_version, event_type) DO NOTHING
            "#,
            aggregate_type,
            aggregate_id,
            aggregate_version,
            event_type,
            Uuid::new_v4(),
            payload,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

/// Everything the checks produced, shared by preview and confirm so the two cannot drift.
#[derive(Debug, Clone)]
struct Assessment {
    business_day: NaiveDate,
    quantity: i64,
    eligible_amount: i64,
    stamps_expire_at: DateTime<Utc>,
    daily_used: i64,
    daily_limit: Option<i64>,
    next_business_day_start: DateTime<Utc>,
    self_transaction: bool,
    duplicate_of: Option<Uuid>,
    has_distinct_order_ref: bool,
    warnings: Vec<PreviewIssue>,
    blockers: Vec<PreviewIssue>,
}

pub fn build_board(lots: &[LotBalance], target: i64, now: DateTime<Utc>) -> StampBoard {
    let available = available_stamps(lots, now);
    let target = target.max(1);

    StampBoard {
        available,
        target,
        // Progress towards the *next* reward, so a board holding 12 of 10 shows 8 to go
        // rather than a negative number.
        remaining_to_goal: (target - available % target) % target,
        earliest_expiry: earliest_expiry(lots, now),
        expiring_within_seven_days: expiring_soon(lots, now),
    }
}

fn earliest_expiry(lots: &[LotBalance], now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    lots.iter()
        .filter(|lot| lot.expires_at > now && lot.balance > 0)
        .map(|lot| lot.expires_at)
        .min()
}

fn expiring_soon(lots: &[LotBalance], now: DateTime<Utc>) -> i64 {
    let horizon = now + chrono::Duration::days(7);
    lots.iter()
        .filter(|lot| lot.expires_at > now && lot.expires_at <= horizon && lot.balance > 0)
        .map(|lot| lot.balance)
        .sum()
}

fn summarise(policy: &LoyaltyPolicy) -> PolicySummary {
    PolicySummary {
        policy_id: policy.id,
        version_no: policy.version_no,
        name: policy.name.clone(),
        target_stamp_count: i64::from(policy.rules.target_stamp_count),
        stamps_per_order: i64::from(policy.rules.stamps_per_order),
        minimum_order_amount: policy.rules.minimum_order_amount,
        daily_earning_limit: policy.rules.daily_earning_limit.map(i64::from),
        restricts_items: policy.rules.restriction().restricts_items(),
        reward_title: policy.reward.title.clone(),
        reward_validity_days: policy.reward.validity_days,
    }
}

/// Show the owner enough to tell two customers apart, and no more (WALLET-005).
pub fn mask_display_name(display_name: &str) -> String {
    let mut characters = display_name.trim().chars();
    match characters.next() {
        None => "고객".to_owned(),
        Some(first) => {
            let remaining = characters.count();
            if remaining == 0 {
                format!("{first}*")
            } else {
                format!("{first}{}", "*".repeat(remaining))
            }
        }
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// The frozen record of the order the owner claimed (§17.1: 거래는 주문 스냅샷을 보관한다).
fn order_snapshot(
    request: &ConfirmStampRequest,
    lines: &[OrderLine],
    assessment: &Assessment,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "gross_amount": request.order.gross_amount,
        "eligible_amount": assessment.eligible_amount,
        "currency": request.order.currency,
        "external_order_ref": request.order.external_order_ref,
        "preview_id": request.preview_id,
        "self_transaction": assessment.self_transaction,
        "acknowledged_duplicate_of": assessment.duplicate_of,
        "items": lines
            .iter()
            .map(|line| serde_json::json!({
                "catalog_item_id": line.catalog_item_id,
                "name_snapshot": line.name_snapshot,
                "quantity": line.quantity,
                "unit_price": line.unit_price,
            }))
            .collect::<Vec<_>>(),
    })
}

/// Turn the first blocking reason into the response `confirm` fails with.
fn blocker_to_error(blocker: &PreviewIssue) -> ApiError {
    let code = match blocker.code.as_str() {
        "MINIMUM_ORDER_NOT_MET" => ErrorCode::MinimumOrderNotMet,
        "ITEM_NOT_ELIGIBLE" => ErrorCode::ItemNotEligible,
        "DAILY_LIMIT_EXCEEDED" => ErrorCode::DailyLimitExceeded,
        _ => ErrorCode::UnprocessableRequest,
    };
    ApiError::with_message(code, blocker.message.clone())
}

fn map_transaction_conflict(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db)
            if db.code().as_deref() == Some("23505")
                && db.constraint() == Some("uq_stamp_transaction_idempotency") =>
        {
            // The middleware normally catches a replay first; this is the database's own
            // guarantee that one key produces one accrual even if it did not.
            ApiError::new(ErrorCode::IdempotencyRequestInProgress)
                .internal("stamp transaction idempotency key is already in use")
        }
        _ => ApiError::from(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(days: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + days * 86_400, 0).expect("valid timestamp")
    }

    fn lot(id: u128, balance: i64, expires_in_days: i64) -> LotBalance {
        LotBalance {
            lot_id: Uuid::from_u128(id),
            balance,
            expires_at: at(expires_in_days),
        }
    }

    fn now() -> DateTime<Utc> {
        at(0)
    }

    #[test]
    fn expired_lots_do_not_count_towards_the_balance() {
        // STAMP-006: the online judgement is immediate, not "whatever the batch left".
        let lots = vec![lot(1, 5, -1), lot(2, 3, 10)];
        assert_eq!(available_stamps(&lots, now()), 3);
    }

    #[test]
    fn a_lot_expiring_exactly_now_is_already_gone() {
        assert_eq!(available_stamps(&[lot(1, 5, 0)], now()), 0, "[start, end)");
    }

    #[test]
    fn nothing_is_planned_below_the_target() {
        let plan = plan_reward_consumption(&[lot(1, 9, 10)], 10, now());
        assert!(plan.is_empty());
    }

    #[test]
    fn the_soonest_to_expire_stamps_are_spent_first() {
        // §8.1: 목표 달성 시 먼저 만료되는 가용 도장부터 소비한다.
        let lots = vec![lot(1, 6, 30), lot(2, 6, 5)];
        let plan = plan_reward_consumption(&lots, 10, now());

        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].draws,
            vec![(Uuid::from_u128(2), 6), (Uuid::from_u128(1), 4)],
            "the lot expiring in five days must be drained before the one expiring in thirty"
        );
        assert_eq!(plan[0].total(), 10);
    }

    #[test]
    fn one_accrual_can_complete_several_boards() {
        // STAMP-004: 한 번에 여러 개를 적립해 목표를 두 번 넘으면 리워드를 여러 장 발급한다.
        let plan = plan_reward_consumption(&[lot(1, 25, 10)], 10, now());

        assert_eq!(plan.len(), 2, "25 stamps against a target of 10 pays twice");
        assert_eq!(plan.iter().map(RewardDraw::total).sum::<i64>(), 20);
    }

    #[test]
    fn leftover_stamps_are_carried_forward_not_consumed() {
        let lots = vec![lot(1, 23, 10)];
        let plan = plan_reward_consumption(&lots, 10, now());
        let spent: i64 = plan.iter().map(RewardDraw::total).sum();

        assert_eq!(spent, 20);
        assert_eq!(
            available_stamps(&lots, now()) - spent,
            3,
            "the remainder stays on the board"
        );
    }

    #[test]
    fn a_reward_may_be_paid_for_from_several_lots() {
        let plan = plan_reward_consumption(&[lot(1, 4, 1), lot(2, 4, 2), lot(3, 4, 3)], 10, now());

        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].draws,
            vec![
                (Uuid::from_u128(1), 4),
                (Uuid::from_u128(2), 4),
                (Uuid::from_u128(3), 2),
            ]
        );
    }

    #[test]
    fn no_lot_is_ever_planned_beyond_its_balance() {
        // §12.6-3, checked at the level that decides it.
        let lots = vec![lot(1, 3, 1), lot(2, 7, 2), lot(3, 11, 3)];
        let plan = plan_reward_consumption(&lots, 10, now());

        let mut spent_per_lot = std::collections::HashMap::new();
        for draw in &plan {
            for (lot_id, quantity) in &draw.draws {
                *spent_per_lot.entry(*lot_id).or_insert(0i64) += quantity;
            }
        }

        for source in &lots {
            assert!(
                spent_per_lot.get(&source.lot_id).copied().unwrap_or(0) <= source.balance,
                "lot {} was over-drawn",
                source.lot_id
            );
        }
    }

    #[test]
    fn expired_lots_are_never_drawn_from() {
        let plan = plan_reward_consumption(&[lot(1, 20, -1), lot(2, 10, 5)], 10, now());

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].draws, vec![(Uuid::from_u128(2), 10)]);
    }

    #[test]
    fn the_plan_is_deterministic_for_lots_sharing_an_expiry() {
        let lots = vec![lot(2, 5, 10), lot(1, 5, 10)];
        let first = plan_reward_consumption(&lots, 10, now());
        let second = plan_reward_consumption(&lots, 10, now());

        assert_eq!(first, second);
        assert_eq!(first[0].draws[0].0, Uuid::from_u128(1), "ties break by id");
    }

    #[test]
    fn an_absurd_target_plans_nothing_rather_than_looping() {
        assert!(plan_reward_consumption(&[lot(1, 10, 5)], 0, now()).is_empty());
        assert!(plan_reward_consumption(&[lot(1, 10, 5)], -1, now()).is_empty());
    }

    #[test]
    fn the_board_reports_progress_towards_the_next_reward() {
        let board = build_board(&[lot(1, 7, 30)], 10, now());
        assert_eq!(board.available, 7);
        assert_eq!(board.remaining_to_goal, 3);

        // A board that is exactly full needs nothing more.
        let full = build_board(&[lot(1, 10, 30)], 10, now());
        assert_eq!(full.remaining_to_goal, 0);

        // And one carrying a remainder counts from the remainder, not from zero.
        let carried = build_board(&[lot(1, 12, 30)], 10, now());
        assert_eq!(carried.remaining_to_goal, 8);
    }

    #[test]
    fn the_board_reports_the_soonest_expiry_and_what_is_about_to_lapse() {
        let board = build_board(&[lot(1, 3, 3), lot(2, 4, 30), lot(3, 5, -1)], 10, now());

        assert_eq!(board.available, 7, "the expired lot is not counted");
        assert_eq!(board.earliest_expiry, Some(at(3)));
        assert_eq!(
            board.expiring_within_seven_days, 3,
            "only the lot inside the seven-day horizon"
        );
    }

    #[test]
    fn an_empty_board_is_reported_as_empty_rather_than_complete() {
        let board = build_board(&[], 10, now());
        assert_eq!(board.available, 0);
        assert_eq!(board.remaining_to_goal, 0);
        assert_eq!(board.earliest_expiry, None);
    }

    #[test]
    fn display_names_are_masked_to_the_first_character() {
        assert_eq!(mask_display_name("김동환"), "김**");
        assert_eq!(mask_display_name("Park"), "P***");
        assert_eq!(mask_display_name("김"), "김*");
        assert_eq!(mask_display_name("  "), "고객");
        assert_eq!(mask_display_name(""), "고객");
    }

    #[test]
    fn a_scan_must_name_exactly_one_way_of_identifying_the_customer() {
        let token_only = ScanRequest {
            qr_token: Some("a.b.c".to_owned()),
            fallback_code: None,
        };
        assert!(matches!(
            token_only.presented().expect("token"),
            Presented::Token("a.b.c")
        ));

        let code_only = ScanRequest {
            qr_token: None,
            fallback_code: Some("01234567".to_owned()),
        };
        assert!(matches!(
            code_only.presented().expect("code"),
            Presented::FallbackCode("01234567")
        ));

        for ambiguous in [
            ScanRequest { qr_token: None, fallback_code: None },
            ScanRequest {
                qr_token: Some("a.b.c".to_owned()),
                fallback_code: Some("01234567".to_owned()),
            },
        ] {
            assert_eq!(
                ambiguous.presented().expect_err("must reject").code,
                ErrorCode::InvalidRequest
            );
        }
    }

    #[test]
    fn a_blocker_keeps_its_specific_status_code() {
        for (code, expected) in [
            ("MINIMUM_ORDER_NOT_MET", ErrorCode::MinimumOrderNotMet),
            ("ITEM_NOT_ELIGIBLE", ErrorCode::ItemNotEligible),
            ("DAILY_LIMIT_EXCEEDED", ErrorCode::DailyLimitExceeded),
            ("SOMETHING_ELSE", ErrorCode::UnprocessableRequest),
        ] {
            let error = blocker_to_error(&PreviewIssue {
                code: code.to_owned(),
                message: "…".to_owned(),
            });
            assert_eq!(error.code, expected);
            assert_eq!(error.status().as_u16(), 422);
        }
    }
}
