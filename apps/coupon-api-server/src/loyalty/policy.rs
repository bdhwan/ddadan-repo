//! Stamp policy versions (§8.1, §12.3, STAMP-001, STAMP-008).
//!
//! A policy is never edited in place once it is live. The owner drafts a new version and
//! publishes it, and the new version applies to *future* accruals only — stamps already
//! in a customer's wallet keep the terms they were earned under (product principle 4,
//! STAMP-008).
//!
//! Two invariants are held by the database, not by this code: at most one `ACTIVE` and at
//! most one `SCHEDULED` version per store, both partial unique indexes. That is what makes
//! "publish" safe to run from two tabs at once.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::catalog::{CatalogService, ItemRestriction};
use crate::db::{Tx, changed_one_row};
use crate::error::{ApiError, ApiResult, ErrorCode, FieldError};

/// `coupon.policy_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PolicyStatus {
    Draft,
    Scheduled,
    Active,
    Paused,
    Ended,
}

impl PolicyStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            PolicyStatus::Draft => "DRAFT",
            PolicyStatus::Scheduled => "SCHEDULED",
            PolicyStatus::Active => "ACTIVE",
            PolicyStatus::Paused => "PAUSED",
            PolicyStatus::Ended => "ENDED",
        }
    }

    /// Unknown statuses are treated as ended: a version this build does not understand
    /// must not accrue anything.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "DRAFT" => PolicyStatus::Draft,
            "SCHEDULED" => PolicyStatus::Scheduled,
            "ACTIVE" => PolicyStatus::Active,
            "PAUSED" => PolicyStatus::Paused,
            _ => PolicyStatus::Ended,
        }
    }

    /// Only a draft may be edited (STAMP-008).
    pub fn is_editable(self) -> bool {
        self == PolicyStatus::Draft
    }
}

/// `coupon.benefit_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BenefitType {
    FixedAmount,
    Percentage,
    FreeItem,
}

impl BenefitType {
    pub fn as_db(self) -> &'static str {
        match self {
            BenefitType::FixedAmount => "FIXED_AMOUNT",
            BenefitType::Percentage => "PERCENTAGE",
            BenefitType::FreeItem => "FREE_ITEM",
        }
    }

    pub fn from_db(raw: &str) -> Self {
        match raw {
            "PERCENTAGE" => BenefitType::Percentage,
            "FREE_ITEM" => BenefitType::FreeItem,
            _ => BenefitType::FixedAmount,
        }
    }
}

/// The §8.1 defaults, in one place so the API, the UI hints and the tests agree.
pub mod defaults {
    pub const TARGET_STAMP_COUNT: i16 = 10;
    pub const STAMPS_PER_ORDER: i16 = 1;
    pub const MINIMUM_ORDER_AMOUNT: i64 = 0;
    pub const DAILY_EARNING_LIMIT: i16 = 1;
    pub const STAMP_VALIDITY_DAYS: i16 = 180;
    pub const REWARD_VALIDITY_DAYS: i16 = 30;
    pub const DUPLICATE_WARNING_MINUTES: i32 = 5;
}

/// Everything a draft or an active version says about earning a stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PolicyRules {
    /// 2–100, default 10.
    pub target_stamp_count: i16,
    /// 1–10, default 1.
    pub stamps_per_order: i16,
    /// Won, default 0.
    pub minimum_order_amount: i64,
    /// Accruals per business day, 1–20. `None` means unlimited (§8.1).
    pub daily_earning_limit: Option<i16>,
    /// 1–60 minutes, default 5 (STAMP-003).
    pub duplicate_warning_minutes: i32,
    /// 1–730 days, default 180.
    pub stamp_validity_days: i16,
    pub eligible_item_ids: Vec<Uuid>,
    pub eligible_category_ids: Vec<Uuid>,
    pub excluded_item_ids: Vec<Uuid>,
}

impl Default for PolicyRules {
    fn default() -> Self {
        Self {
            target_stamp_count: defaults::TARGET_STAMP_COUNT,
            stamps_per_order: defaults::STAMPS_PER_ORDER,
            minimum_order_amount: defaults::MINIMUM_ORDER_AMOUNT,
            daily_earning_limit: Some(defaults::DAILY_EARNING_LIMIT),
            duplicate_warning_minutes: defaults::DUPLICATE_WARNING_MINUTES,
            stamp_validity_days: defaults::STAMP_VALIDITY_DAYS,
            eligible_item_ids: Vec::new(),
            eligible_category_ids: Vec::new(),
            excluded_item_ids: Vec::new(),
        }
    }
}

impl PolicyRules {
    pub fn restriction(&self) -> ItemRestriction {
        ItemRestriction {
            eligible_item_ids: self.eligible_item_ids.clone(),
            eligible_category_ids: self.eligible_category_ids.clone(),
            excluded_item_ids: self.excluded_item_ids.clone(),
        }
    }

    pub fn duplicate_window(&self) -> Duration {
        Duration::minutes(i64::from(self.duplicate_warning_minutes))
    }

    pub fn stamp_validity(&self) -> Duration {
        Duration::days(i64::from(self.stamp_validity_days))
    }
}

/// The reward a completed board pays out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RewardDefinition {
    pub benefit_type: BenefitType,
    pub fixed_amount: Option<i64>,
    /// 1–100.
    pub percentage: Option<i16>,
    pub maximum_discount_amount: Option<i64>,
    pub free_item_ids: Vec<Uuid>,
    pub minimum_order_amount: i64,
    /// 1–365 days, default 30.
    pub validity_days: i16,
    /// Shown on the wallet card.
    pub title: String,
    /// The usage conditions in the customer's words.
    pub description: String,
    /// The notice STAMP-001 requires before a policy may go live.
    pub customer_notice: String,
}

impl Default for RewardDefinition {
    fn default() -> Self {
        Self {
            benefit_type: BenefitType::FixedAmount,
            fixed_amount: None,
            percentage: None,
            maximum_discount_amount: None,
            free_item_ids: Vec::new(),
            minimum_order_amount: 0,
            validity_days: defaults::REWARD_VALIDITY_DAYS,
            title: String::new(),
            description: String::new(),
            customer_notice: String::new(),
        }
    }
}

/// One version, as the owner app sees it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoyaltyPolicy {
    pub id: Uuid,
    pub store_id: Uuid,
    pub version_no: i32,
    pub status: PolicyStatus,
    pub name: String,
    #[serde(flatten)]
    pub rules: PolicyRules,
    pub reward: RewardDefinition,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub schema_version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoyaltyPoliciesResponse {
    pub policies: Vec<LoyaltyPolicy>,
    /// The version accruals are being judged against right now, if any.
    pub active_policy_id: Option<Uuid>,
    /// The version that takes over at its `starts_at` (§6.3 shows this as "다음 예약").
    pub scheduled_policy_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreatePolicyRequest {
    #[validate(length(min = 1, max = 160, message = "정책 이름은 1~160자여야 합니다."))]
    pub name: String,
    /// Omitted fields take the §8.1 defaults.
    pub rules: Option<PolicyRules>,
    pub reward: Option<RewardDefinition>,
    /// Optional absolute end of the version.
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdatePolicyRequest {
    #[validate(length(min = 1, max = 160, message = "정책 이름은 1~160자여야 합니다."))]
    pub name: Option<String>,
    pub rules: Option<PolicyRules>,
    pub reward: Option<RewardDefinition>,
    pub ends_at: Option<DateTime<Utc>>,
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct PublishPolicyRequest {
    /// When the version takes over. Absent or in the past means immediately.
    pub effective_at: Option<DateTime<Utc>>,
}

/// Validate a draft's numbers against the §8.1 ranges.
///
/// The database has the same CHECK constraints; this exists so the owner gets a field
/// error naming the input rather than a bare constraint violation.
pub fn validate_rules(rules: &PolicyRules) -> ApiResult<()> {
    let mut errors = Vec::new();

    if !(2..=100).contains(&rules.target_stamp_count) {
        errors.push(FieldError::new(
            "rules.target_stamp_count",
            "OUT_OF_RANGE",
            "목표 도장 수는 2~100개여야 합니다.",
        ));
    }
    if !(1..=10).contains(&rules.stamps_per_order) {
        errors.push(FieldError::new(
            "rules.stamps_per_order",
            "OUT_OF_RANGE",
            "주문당 적립 수는 1~10개여야 합니다.",
        ));
    }
    if !(0..=100_000_000).contains(&rules.minimum_order_amount) {
        errors.push(FieldError::new(
            "rules.minimum_order_amount",
            "OUT_OF_RANGE",
            "최소 주문액은 0~100,000,000원이어야 합니다.",
        ));
    }
    // `None` is the documented spelling of "무제한", so only a present value is ranged.
    if let Some(limit) = rules.daily_earning_limit
        && !(1..=20).contains(&limit)
    {
        errors.push(FieldError::new(
            "rules.daily_earning_limit",
            "OUT_OF_RANGE",
            "영업일당 적립 횟수는 1~20회이거나 무제한이어야 합니다.",
        ));
    }
    if !(1..=60).contains(&rules.duplicate_warning_minutes) {
        errors.push(FieldError::new(
            "rules.duplicate_warning_minutes",
            "OUT_OF_RANGE",
            "중복 경고 구간은 1~60분이어야 합니다.",
        ));
    }
    if !(1..=730).contains(&rules.stamp_validity_days) {
        errors.push(FieldError::new(
            "rules.stamp_validity_days",
            "OUT_OF_RANGE",
            "도장 유효기간은 1~730일이어야 합니다.",
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ApiError::with_fields(ErrorCode::ValidationFailed, errors))
    }
}

/// Validate the reward.
///
/// STAMP-001 draws the line for us: **초안 저장 시 논리 오류만 검사한다**. So a draft is
/// checked for contradictions — a fixed amount *and* a percentage, a rate outside 1–100 —
/// but not for being unfinished. `for_publish` adds the completeness rules that only
/// matter once a customer can earn the thing: every field the benefit needs, plus the
/// wording they will read.
pub fn validate_reward(reward: &RewardDefinition, for_publish: bool) -> ApiResult<()> {
    let mut errors = Vec::new();

    if !(1..=365).contains(&reward.validity_days) {
        errors.push(FieldError::new(
            "reward.validity_days",
            "OUT_OF_RANGE",
            "리워드 유효기간은 1~365일이어야 합니다.",
        ));
    }
    if reward.minimum_order_amount < 0 {
        errors.push(FieldError::new(
            "reward.minimum_order_amount",
            "OUT_OF_RANGE",
            "최소 주문액은 0원 이상이어야 합니다.",
        ));
    }

    // Mirrors `ck_reward_benefit_fields`: exactly the fields the benefit type needs, and
    // none of the others.
    match reward.benefit_type {
        BenefitType::FixedAmount => {
            // A present-but-nonsensical amount is a logic error even in a draft; an absent
            // one is merely unfinished, and only blocks publishing.
            if reward.fixed_amount.is_some_and(|amount| amount <= 0)
                || (for_publish && reward.fixed_amount.is_none())
            {
                errors.push(FieldError::new(
                    "reward.fixed_amount",
                    "REQUIRED",
                    "정액 할인액을 입력해 주세요.",
                ));
            }
            if reward.percentage.is_some() || !reward.free_item_ids.is_empty() {
                errors.push(FieldError::new(
                    "reward.benefit_type",
                    "CONFLICT",
                    "정액 할인에는 할인율이나 무료 품목을 함께 지정할 수 없습니다.",
                ));
            }
        }
        BenefitType::Percentage => {
            if reward
                .percentage
                .is_some_and(|value| !(1..=100).contains(&value))
                || (for_publish && reward.percentage.is_none())
            {
                errors.push(FieldError::new(
                    "reward.percentage",
                    "OUT_OF_RANGE",
                    "할인율은 1~100% 여야 합니다.",
                ));
            }
            // §8.2 makes the cap mandatory for a rate discount, so an uncapped one is a
            // logic error rather than an omission — but only once a rate is actually set.
            if reward.maximum_discount_amount.is_some_and(|amount| amount <= 0)
                || ((for_publish || reward.percentage.is_some())
                    && reward.maximum_discount_amount.is_none())
            {
                errors.push(FieldError::new(
                    "reward.maximum_discount_amount",
                    "REQUIRED",
                    "정률 할인에는 최대 할인액이 필요합니다.",
                ));
            }
            if reward.fixed_amount.is_some() || !reward.free_item_ids.is_empty() {
                errors.push(FieldError::new(
                    "reward.benefit_type",
                    "CONFLICT",
                    "정률 할인에는 정액 할인액이나 무료 품목을 함께 지정할 수 없습니다.",
                ));
            }
        }
        BenefitType::FreeItem => {
            if for_publish && reward.free_item_ids.is_empty() {
                errors.push(FieldError::new(
                    "reward.free_item_ids",
                    "REQUIRED",
                    "무료 제공할 품목을 하나 이상 선택해 주세요.",
                ));
            }
            if reward.fixed_amount.is_some() || reward.percentage.is_some() {
                errors.push(FieldError::new(
                    "reward.benefit_type",
                    "CONFLICT",
                    "무료 품목에는 할인액이나 할인율을 함께 지정할 수 없습니다.",
                ));
            }
        }
    }

    if for_publish {
        // STAMP-001: 활성화 시 할인 내용, 사용 조건, 고객 고지 문구가 모두 있어야 한다.
        for (value, field, message) in [
            (&reward.title, "reward.title", "리워드 이름을 입력해 주세요."),
            (
                &reward.description,
                "reward.description",
                "리워드 사용 조건을 입력해 주세요.",
            ),
            (
                &reward.customer_notice,
                "reward.customer_notice",
                "고객 고지 문구를 입력해 주세요.",
            ),
        ] {
            if value.trim().is_empty() {
                errors.push(FieldError::new(field, "REQUIRED", message));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ApiError::with_fields(ErrorCode::ValidationFailed, errors))
    }
}

/// The immutable record of what a version said, stored alongside the columns (§12.1).
pub fn rule_snapshot(name: &str, rules: &PolicyRules, reward: &RewardDefinition) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "name": name,
        "rules": rules,
        "reward": reward,
    })
}

pub struct PolicyService {
    catalog: std::sync::Arc<CatalogService>,
}

impl PolicyService {
    pub fn new(catalog: std::sync::Arc<CatalogService>) -> Self {
        Self { catalog }
    }

    /// Every version of a store's policy, newest first.
    pub async fn list(&self, pool: &PgPool, store_id: Uuid) -> ApiResult<LoyaltyPoliciesResponse> {
        // Promote a scheduled version whose moment has come, so the list never shows a
        // "scheduled" version that should already be live (§11.6).
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;
        self.settle_schedule(&mut tx, store_id, now).await?;
        let policies = self.load_all(&mut tx, store_id).await?;
        tx.commit().await?;

        let active_policy_id = policies
            .iter()
            .find(|policy| policy.status == PolicyStatus::Active)
            .map(|policy| policy.id);
        let scheduled_policy_id = policies
            .iter()
            .find(|policy| policy.status == PolicyStatus::Scheduled)
            .map(|policy| policy.id);

        Ok(LoyaltyPoliciesResponse {
            policies,
            active_policy_id,
            scheduled_policy_id,
        })
    }

    async fn load_all(&self, tx: &mut Tx<'_>, store_id: Uuid) -> ApiResult<Vec<LoyaltyPolicy>> {
        let rows = sqlx::query_as!(
            PolicyRow,
            r#"
            SELECT
                p.id,
                p.store_id,
                p.version_no,
                p.status::text AS "status!",
                p.name,
                p.rule_snapshot,
                p.starts_at,
                p.ends_at,
                p.published_at,
                p.schema_version,
                p.created_at,
                p.updated_at,
                p.version
            FROM coupon.loyalty_policies p
            WHERE p.store_id = $1
            ORDER BY p.version_no DESC
            "#,
            store_id,
        )
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows.into_iter().map(hydrate).collect())
    }

    /// Create the next draft version.
    pub async fn create(
        &self,
        pool: &PgPool,
        store_id: Uuid,
        created_by: Uuid,
        request: &CreatePolicyRequest,
    ) -> ApiResult<LoyaltyPolicy> {
        let rules = request.rules.clone().unwrap_or_default();
        let reward = request.reward.clone().unwrap_or_default();

        validate_rules(&rules)?;
        validate_reward(&reward, false)?;

        let mut tx = pool.begin().await?;
        self.ensure_catalog_selectable(&mut tx, store_id, &rules, &reward)
            .await?;

        // `uq_loyalty_policies_store_version` makes this safe: if two drafts race for the
        // same number, one loses the insert and retries with a fresh read.
        let next_version = sqlx::query_scalar!(
            "SELECT COALESCE(MAX(version_no), 0) + 1 FROM coupon.loyalty_policies WHERE store_id = $1",
            store_id,
        )
        .fetch_one(&mut *tx)
        .await?
        .unwrap_or(1);

        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.loyalty_policies
                (store_id, version_no, status, name, target_stamp_count, stamps_per_order,
                 minimum_order_amount, daily_earning_limit, duplicate_warning_seconds,
                 stamp_validity_days, ends_at, eligible_item_ids, eligible_category_ids,
                 excluded_item_ids, rule_snapshot, created_by_user_id)
            VALUES ($1, $2, 'DRAFT', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            RETURNING id
            "#,
            store_id,
            next_version,
            request.name.trim(),
            rules.target_stamp_count,
            rules.stamps_per_order,
            rules.minimum_order_amount,
            rules.daily_earning_limit,
            rules.duplicate_warning_minutes * 60,
            rules.stamp_validity_days,
            request.ends_at,
            &rules.eligible_item_ids,
            &rules.eligible_category_ids,
            &rules.excluded_item_ids,
            rule_snapshot(request.name.trim(), &rules, &reward),
            created_by,
        )
        .fetch_one(&mut *tx)
        .await?;

        // The normalised reward row carries the database's own completeness CHECK, so it
        // is only written once the reward actually is complete. Until then the draft's
        // reward lives in `rule_snapshot`, which is what an unfinished draft is for.
        if validate_reward(&reward, true).is_ok() {
            self.upsert_reward(&mut tx, id, &reward).await?;
        }
        let policy = self.load_one(&mut tx, store_id, id).await?;
        tx.commit().await?;

        Ok(policy)
    }

    /// Edit a draft. An active version is never edited — STAMP-008 sends the owner to a
    /// new version instead.
    pub async fn update(
        &self,
        pool: &PgPool,
        store_id: Uuid,
        policy_id: Uuid,
        request: &UpdatePolicyRequest,
        expected_version: Option<i64>,
    ) -> ApiResult<LoyaltyPolicy> {
        let mut tx = pool.begin().await?;
        let current = self.load_one(&mut tx, store_id, policy_id).await?;

        if !current.status.is_editable() {
            return Err(ApiError::with_message(
                ErrorCode::PolicyNotEditable,
                match current.status {
                    PolicyStatus::Active => {
                        "활성 정책은 수정할 수 없습니다. 새 버전을 만들어 게시해 주세요."
                    }
                    PolicyStatus::Scheduled => {
                        "예약된 정책은 수정할 수 없습니다. 예약을 취소하거나 새 버전을 만들어 주세요."
                    }
                    _ => "종료된 정책은 수정할 수 없습니다.",
                },
            ));
        }

        let rules = request.rules.clone().unwrap_or(current.rules.clone());
        let reward = request.reward.clone().unwrap_or(current.reward.clone());
        let name = request
            .name
            .as_deref()
            .map(str::trim)
            .unwrap_or(current.name.as_str())
            .to_owned();

        validate_rules(&rules)?;
        validate_reward(&reward, false)?;
        self.ensure_catalog_selectable(&mut tx, store_id, &rules, &reward)
            .await?;

        let result = sqlx::query!(
            r#"
            UPDATE coupon.loyalty_policies
            SET name = $3,
                target_stamp_count = $4,
                stamps_per_order = $5,
                minimum_order_amount = $6,
                daily_earning_limit = $7,
                duplicate_warning_seconds = $8,
                stamp_validity_days = $9,
                ends_at = COALESCE($10, ends_at),
                eligible_item_ids = $11,
                eligible_category_ids = $12,
                excluded_item_ids = $13,
                rule_snapshot = $14
            WHERE id = $1 AND store_id = $2 AND status = 'DRAFT'
              AND ($15::bigint IS NULL OR version = $15)
            "#,
            policy_id,
            store_id,
            name,
            rules.target_stamp_count,
            rules.stamps_per_order,
            rules.minimum_order_amount,
            rules.daily_earning_limit,
            rules.duplicate_warning_minutes * 60,
            rules.stamp_validity_days,
            request.ends_at,
            &rules.eligible_item_ids,
            &rules.eligible_category_ids,
            &rules.excluded_item_ids,
            rule_snapshot(&name, &rules, &reward),
            expected_version,
        )
        .execute(&mut *tx)
        .await?;

        if !changed_one_row(&result) {
            crate::http::concurrency::ensure_updated(result.rows_affected(), true)?;
        }

        if validate_reward(&reward, true).is_ok() {
            self.upsert_reward(&mut tx, policy_id, &reward).await?;
        }
        let policy = self.load_one(&mut tx, store_id, policy_id).await?;
        tx.commit().await?;

        Ok(policy)
    }

    /// Publish a draft, immediately or at a future instant (§11.4).
    pub async fn publish(
        &self,
        pool: &PgPool,
        store_id: Uuid,
        policy_id: Uuid,
        request: &PublishPolicyRequest,
    ) -> ApiResult<LoyaltyPolicy> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        let current = self.load_one(&mut tx, store_id, policy_id).await?;
        if current.status != PolicyStatus::Draft {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "초안 상태의 정책만 게시할 수 있습니다.",
            ));
        }

        validate_rules(&current.rules)?;
        // The stricter check: every field the benefit needs, and the wording the customer
        // will read, have to exist by now (STAMP-001).
        validate_reward(&current.reward, true)?;
        // Materialise the reward the coupons will reference. Going live is the moment it
        // stops being a sketch.
        self.upsert_reward(&mut tx, policy_id, &current.reward).await?;

        if let Some(ends_at) = current.ends_at
            && ends_at <= now
        {
            return Err(ApiError::with_message(
                ErrorCode::UnprocessableRequest,
                "이미 지난 종료 시각으로는 게시할 수 없습니다.",
            ));
        }

        let immediate = request
            .effective_at
            .is_none_or(|effective_at| effective_at <= now);

        if immediate {
            // End the outgoing version *before* activating the new one; the partial
            // unique index would reject the overlap otherwise, which is exactly the
            // protection §12.6-2 asks for.
            self.end_active(&mut tx, store_id, now).await?;

            sqlx::query!(
                r#"
                UPDATE coupon.loyalty_policies
                SET status = 'ACTIVE', starts_at = $3, published_at = $3
                WHERE id = $1 AND store_id = $2 AND status = 'DRAFT'
                "#,
                policy_id,
                store_id,
                now,
            )
            .execute(&mut *tx)
            .await
            .map_err(map_policy_conflict)?;
        } else {
            let effective_at = request.effective_at.expect("checked above");
            if let Some(ends_at) = current.ends_at
                && ends_at <= effective_at
            {
                return Err(ApiError::with_message(
                    ErrorCode::UnprocessableRequest,
                    "종료 시각은 시작 시각보다 뒤여야 합니다.",
                ));
            }

            sqlx::query!(
                r#"
                UPDATE coupon.loyalty_policies
                SET status = 'SCHEDULED', starts_at = $3, published_at = $4
                WHERE id = $1 AND store_id = $2 AND status = 'DRAFT'
                "#,
                policy_id,
                store_id,
                effective_at,
                now,
            )
            .execute(&mut *tx)
            .await
            .map_err(map_policy_conflict)?;
        }

        let policy = self.load_one(&mut tx, store_id, policy_id).await?;
        tx.commit().await?;

        Ok(policy)
    }

    /// Bring the store's versions in line with the clock.
    ///
    /// Called at the start of anything that depends on "which policy applies now", so a
    /// scheduled switch-over takes effect without a background job having to have run
    /// (§18.1: 온라인 판정은 즉시).
    pub async fn settle_schedule(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        // A version that has reached its own end date stops, whether or not a successor
        // is waiting (STAMP-008).
        sqlx::query!(
            r#"
            UPDATE coupon.loyalty_policies
            SET status = 'ENDED'
            WHERE store_id = $1 AND status = 'ACTIVE' AND ends_at IS NOT NULL AND ends_at <= $2
            "#,
            store_id,
            now,
        )
        .execute(&mut **tx)
        .await?;

        let due: Option<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.loyalty_policies
            WHERE store_id = $1 AND status = 'SCHEDULED' AND starts_at <= $2
            ORDER BY starts_at
            LIMIT 1
            FOR UPDATE
            "#,
            store_id,
            now,
        )
        .fetch_optional(&mut **tx)
        .await?;

        let Some(due) = due else {
            return Ok(());
        };

        self.end_active(tx, store_id, now).await?;

        sqlx::query!(
            r#"
            UPDATE coupon.loyalty_policies
            SET status = 'ACTIVE'
            WHERE id = $1 AND status = 'SCHEDULED'
            "#,
            due,
        )
        .execute(&mut **tx)
        .await?;

        tracing::info!(store_id = %store_id, policy_id = %due, "loyalty.policy_activated");
        Ok(())
    }

    async fn end_active(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.loyalty_policies
            SET status = 'ENDED', ends_at = COALESCE(ends_at, $2)
            WHERE store_id = $1 AND status = 'ACTIVE'
            "#,
            store_id,
            now,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// The version an accrual right now must be judged against (§13.1 step 3).
    ///
    /// Locks the row so the policy cannot be republished underneath a transaction that
    /// has already read its numbers.
    pub async fn active_for_accrual(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        now: DateTime<Utc>,
    ) -> ApiResult<LoyaltyPolicy> {
        self.settle_schedule(tx, store_id, now).await?;

        let id: Option<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.loyalty_policies
            WHERE store_id = $1 AND status = 'ACTIVE'
            FOR NO KEY UPDATE
            "#,
            store_id,
        )
        .fetch_optional(&mut **tx)
        .await?;

        let Some(id) = id else {
            return Err(ApiError::new(ErrorCode::NoActivePolicy));
        };

        let policy = self.load_one(tx, store_id, id).await?;

        // Belt and braces: `settle_schedule` should already have retired anything outside
        // its window, but an accrual is the last place to discover otherwise.
        if policy.starts_at.is_some_and(|starts_at| now < starts_at) {
            return Err(ApiError::with_message(
                ErrorCode::NoActivePolicy,
                "아직 시작되지 않은 정책입니다.",
            ));
        }
        if policy.ends_at.is_some_and(|ends_at| now >= ends_at) {
            return Err(ApiError::with_message(
                ErrorCode::NoActivePolicy,
                "종료된 정책입니다.",
            ));
        }

        Ok(policy)
    }

    pub async fn load_one(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        policy_id: Uuid,
    ) -> ApiResult<LoyaltyPolicy> {
        let row = sqlx::query_as!(
            PolicyRow,
            r#"
            SELECT
                p.id,
                p.store_id,
                p.version_no,
                p.status::text AS "status!",
                p.name,
                p.rule_snapshot,
                p.starts_at,
                p.ends_at,
                p.published_at,
                p.schema_version,
                p.created_at,
                p.updated_at,
                p.version
            FROM coupon.loyalty_policies p
            WHERE p.id = $1 AND p.store_id = $2
            "#,
            policy_id,
            store_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::LoyaltyPolicyNotFound))?;

        Ok(hydrate(row))
    }

    async fn upsert_reward(
        &self,
        tx: &mut Tx<'_>,
        policy_id: Uuid,
        reward: &RewardDefinition,
    ) -> ApiResult<()> {
        // `loyalty_reward_definitions` is referenced by every coupon a completed board
        // issues, so the row is replaced only while the policy is still a draft — which
        // is the only state that reaches here.
        sqlx::query!(
            r#"
            INSERT INTO coupon.loyalty_reward_definitions
                (policy_id, benefit_type, fixed_amount, percentage, maximum_discount_amount,
                 free_item_ids, minimum_order_amount, validity_days, condition_snapshot)
            VALUES ($1, $2::text::coupon.benefit_type, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (policy_id) DO UPDATE SET
                benefit_type = EXCLUDED.benefit_type,
                fixed_amount = EXCLUDED.fixed_amount,
                percentage = EXCLUDED.percentage,
                maximum_discount_amount = EXCLUDED.maximum_discount_amount,
                free_item_ids = EXCLUDED.free_item_ids,
                minimum_order_amount = EXCLUDED.minimum_order_amount,
                validity_days = EXCLUDED.validity_days,
                condition_snapshot = EXCLUDED.condition_snapshot
            "#,
            policy_id,
            reward.benefit_type.as_db(),
            reward.fixed_amount,
            reward.percentage,
            reward.maximum_discount_amount,
            &reward.free_item_ids,
            reward.minimum_order_amount,
            reward.validity_days,
            serde_json::to_value(reward).unwrap_or_else(|_| serde_json::json!({})),
        )
        .execute(&mut **tx)
        .await
        .map_err(|error| match &error {
            // The database CHECK is the same rule `validate_reward` enforces; reaching it
            // means the two drifted, which is a bug worth naming rather than a 500.
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23514") => {
                ApiError::with_message(ErrorCode::ValidationFailed, "리워드 설정을 확인해 주세요.")
                    .internal(db.to_string())
            }
            _ => ApiError::from(error),
        })?;

        Ok(())
    }

    async fn ensure_catalog_selectable(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        rules: &PolicyRules,
        reward: &RewardDefinition,
    ) -> ApiResult<()> {
        // §8.3: a *new* policy may only name active items. Existing snapshots keep
        // whatever they already reference.
        let mut items = rules.eligible_item_ids.clone();
        items.extend_from_slice(&rules.excluded_item_ids);
        items.extend_from_slice(&reward.free_item_ids);
        items.sort_unstable();
        items.dedup();

        self.catalog
            .ensure_selectable(&mut **tx, store_id, &items)
            .await?;
        self.catalog
            .ensure_categories_selectable(&mut **tx, store_id, &rules.eligible_category_ids)
            .await
    }
}

/// Rebuild the API shape from the row.
///
/// The columns are authoritative for the numbers the database enforces; the reward and
/// its wording come from the snapshot, which is the record of what the owner actually
/// published (§12.1).
fn hydrate(row: PolicyRow) -> LoyaltyPolicy {
    let snapshot = row.rule_snapshot;

    let rules: PolicyRules = snapshot
        .get("rules")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let reward: RewardDefinition = snapshot
        .get("reward")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();

    LoyaltyPolicy {
        id: row.id,
        store_id: row.store_id,
        version_no: row.version_no,
        status: PolicyStatus::from_db(&row.status),
        name: row.name,
        rules,
        reward,
        starts_at: row.starts_at,
        ends_at: row.ends_at,
        published_at: row.published_at,
        schema_version: row.schema_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        version: row.version,
    }
}

/// Shape of the policy SELECT, named so `hydrate` can be shared by both call sites.
struct PolicyRow {
    id: Uuid,
    store_id: Uuid,
    version_no: i32,
    status: String,
    name: String,
    rule_snapshot: serde_json::Value,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    published_at: Option<DateTime<Utc>>,
    schema_version: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

fn map_policy_conflict(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            match db.constraint() {
                // §12.6-2, and its scheduled counterpart.
                Some("uq_loyalty_policies_active_store") => ApiError::with_message(
                    ErrorCode::Conflict,
                    "이미 활성화된 정책이 있습니다. 새로고침 후 다시 시도해 주세요.",
                ),
                Some("uq_loyalty_policies_scheduled_store") => {
                    ApiError::new(ErrorCode::PolicyAlreadyScheduled)
                }
                _ => ApiError::new(ErrorCode::Conflict).internal(db.to_string()),
            }
        }
        _ => ApiError::from(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reward() -> RewardDefinition {
        RewardDefinition {
            benefit_type: BenefitType::FixedAmount,
            fixed_amount: Some(3_000),
            title: "아메리카노 3,000원 할인".to_owned(),
            description: "1만원 이상 주문 시 사용".to_owned(),
            customer_notice: "다른 할인과 중복 사용 불가".to_owned(),
            ..RewardDefinition::default()
        }
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let rules = PolicyRules::default();

        // §8.1 / §23.1.
        assert_eq!(rules.target_stamp_count, 10);
        assert_eq!(rules.stamps_per_order, 1);
        assert_eq!(rules.minimum_order_amount, 0);
        assert_eq!(rules.daily_earning_limit, Some(1));
        assert_eq!(rules.stamp_validity_days, 180);
        assert_eq!(rules.duplicate_warning_minutes, 5);
        assert_eq!(RewardDefinition::default().validity_days, 30);

        validate_rules(&rules).expect("the defaults must be valid");
    }

    #[test]
    fn every_range_boundary_is_accepted_and_the_step_outside_is_not() {
        let cases: [(&str, PolicyRules, bool); 12] = [
            ("target 2", PolicyRules { target_stamp_count: 2, ..Default::default() }, true),
            ("target 100", PolicyRules { target_stamp_count: 100, ..Default::default() }, true),
            ("target 1", PolicyRules { target_stamp_count: 1, ..Default::default() }, false),
            ("target 101", PolicyRules { target_stamp_count: 101, ..Default::default() }, false),
            ("per order 10", PolicyRules { stamps_per_order: 10, ..Default::default() }, true),
            ("per order 11", PolicyRules { stamps_per_order: 11, ..Default::default() }, false),
            ("daily 20", PolicyRules { daily_earning_limit: Some(20), ..Default::default() }, true),
            ("daily 21", PolicyRules { daily_earning_limit: Some(21), ..Default::default() }, false),
            ("validity 730", PolicyRules { stamp_validity_days: 730, ..Default::default() }, true),
            ("validity 731", PolicyRules { stamp_validity_days: 731, ..Default::default() }, false),
            ("window 60", PolicyRules { duplicate_warning_minutes: 60, ..Default::default() }, true),
            ("window 61", PolicyRules { duplicate_warning_minutes: 61, ..Default::default() }, false),
        ];

        for (label, rules, expected) in cases {
            assert_eq!(
                validate_rules(&rules).is_ok(),
                expected,
                "{label} should be {}",
                if expected { "accepted" } else { "rejected" }
            );
        }
    }

    #[test]
    fn an_unlimited_daily_allowance_is_expressed_as_absent() {
        let rules = PolicyRules {
            daily_earning_limit: None,
            ..Default::default()
        };
        validate_rules(&rules).expect("무제한 is a valid configuration");
    }

    #[test]
    fn a_draft_may_omit_customer_wording_but_a_published_version_may_not() {
        let bare = RewardDefinition {
            fixed_amount: Some(3_000),
            ..RewardDefinition::default()
        };

        validate_reward(&bare, false).expect("a draft is allowed to be incomplete");

        // STAMP-001: 활성화 시 리워드의 할인 내용, 사용 조건, 고객 고지 문구가 모두 있어야 한다.
        let error = validate_reward(&bare, true).expect_err("publishing must demand the wording");
        let fields: Vec<&str> = error
            .field_errors
            .iter()
            .map(|field| field.field.as_str())
            .collect();
        assert!(fields.contains(&"reward.title"));
        assert!(fields.contains(&"reward.description"));
        assert!(fields.contains(&"reward.customer_notice"));
    }

    #[test]
    fn a_benefit_must_carry_exactly_its_own_fields() {
        validate_reward(&reward(), true).expect("a well-formed fixed discount");

        let mixed = RewardDefinition {
            percentage: Some(10),
            ..reward()
        };
        assert!(
            validate_reward(&mixed, false).is_err(),
            "a fixed discount must not also carry a rate"
        );

        let percentage = RewardDefinition {
            benefit_type: BenefitType::Percentage,
            fixed_amount: None,
            percentage: Some(10),
            maximum_discount_amount: Some(5_000),
            ..reward()
        };
        validate_reward(&percentage, true).expect("a well-formed rate discount");

        let uncapped = RewardDefinition {
            maximum_discount_amount: None,
            ..percentage.clone()
        };
        assert!(
            validate_reward(&uncapped, false).is_err(),
            "§8.2 requires a cap on a percentage discount"
        );

        let free_item = RewardDefinition {
            benefit_type: BenefitType::FreeItem,
            fixed_amount: None,
            percentage: None,
            maximum_discount_amount: None,
            free_item_ids: vec![Uuid::from_u128(1)],
            ..reward()
        };
        validate_reward(&free_item, true).expect("a well-formed free item");

        let no_items = RewardDefinition {
            free_item_ids: Vec::new(),
            ..free_item
        };
        // Unfinished, not contradictory — so it is a draft's business, not the
        // validator's, until publishing (STAMP-001: 초안 저장 시 논리 오류만 검사한다).
        validate_reward(&no_items, false).expect("an empty draft is allowed");
        assert!(validate_reward(&no_items, true).is_err());
    }

    #[test]
    fn a_draft_is_checked_for_contradictions_but_not_for_being_unfinished() {
        // Nothing filled in at all: incomplete, but nothing about it is wrong yet.
        validate_reward(&RewardDefinition::default(), false)
            .expect("a blank draft reward is allowed");

        // A negative amount is wrong however unfinished the rest is.
        let nonsense = RewardDefinition {
            fixed_amount: Some(-1),
            ..RewardDefinition::default()
        };
        assert!(validate_reward(&nonsense, false).is_err());

        // So is a rate outside 1–100, or a rate with no cap (§8.2).
        let bad_rate = RewardDefinition {
            benefit_type: BenefitType::Percentage,
            percentage: Some(140),
            maximum_discount_amount: Some(5_000),
            ..RewardDefinition::default()
        };
        assert!(validate_reward(&bad_rate, false).is_err());

        let uncapped = RewardDefinition {
            benefit_type: BenefitType::Percentage,
            percentage: Some(10),
            maximum_discount_amount: None,
            ..RewardDefinition::default()
        };
        assert!(
            validate_reward(&uncapped, false).is_err(),
            "a rate without a cap is a logic error, not an omission"
        );
    }

    #[test]
    fn only_a_draft_is_editable() {
        assert!(PolicyStatus::Draft.is_editable());
        for status in [
            PolicyStatus::Scheduled,
            PolicyStatus::Active,
            PolicyStatus::Paused,
            PolicyStatus::Ended,
        ] {
            assert!(!status.is_editable(), "{status:?} must not be editable");
        }
        assert_eq!(PolicyStatus::from_db("SOMETHING"), PolicyStatus::Ended);
    }

    #[test]
    fn the_snapshot_round_trips_the_rules_and_the_reward() {
        let rules = PolicyRules {
            target_stamp_count: 12,
            eligible_item_ids: vec![Uuid::from_u128(1)],
            ..Default::default()
        };
        let snapshot = rule_snapshot("여름 정책", &rules, &reward());

        assert_eq!(snapshot["schema_version"], 1);
        let restored: PolicyRules =
            serde_json::from_value(snapshot["rules"].clone()).expect("rules round-trip");
        assert_eq!(restored, rules);
        let restored_reward: RewardDefinition =
            serde_json::from_value(snapshot["reward"].clone()).expect("reward round-trips");
        assert_eq!(restored_reward, reward());
    }

    #[test]
    fn derived_durations_match_the_configured_numbers() {
        let rules = PolicyRules {
            duplicate_warning_minutes: 5,
            stamp_validity_days: 180,
            ..Default::default()
        };

        assert_eq!(rules.duplicate_window().num_seconds(), 300);
        assert_eq!(rules.stamp_validity().num_days(), 180);
    }

    #[test]
    fn the_restriction_is_carried_over_unchanged() {
        let rules = PolicyRules {
            eligible_item_ids: vec![Uuid::from_u128(1)],
            excluded_item_ids: vec![Uuid::from_u128(2)],
            ..Default::default()
        };

        let restriction = rules.restriction();
        assert!(restriction.restricts_items());
        assert_eq!(restriction.eligible_item_ids, rules.eligible_item_ids);
        assert_eq!(restriction.excluded_item_ids, rules.excluded_item_ids);
    }
}
