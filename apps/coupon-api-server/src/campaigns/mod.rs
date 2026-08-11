//! 캠페인·대상·수량 (§10.2 `campaigns`, §11.4, §13.2).
//!
//! Owns `campaigns`, `campaign_audience_members` and `campaign_counters`, and is the only
//! module that writes them. It also *creates* campaign coupon instances, for the same
//! reason `loyalty` creates reward instances: the quantity decision and the instance have
//! to be one transaction or §12.6-4 is not a real invariant.
//!
//! Three rules run through everything here.
//!
//! * **The campaign is not the coupon.** Publishing freezes the benefit into each
//!   instance's `condition_snapshot`; editing the campaign afterwards never reaches back
//!   (§8.5, product principle 4). That is why [`update`](CampaignService::update) refuses
//!   the fields that would be retroactive rather than quietly applying them going forward.
//! * **Quantity is decided by PostgreSQL, never by a counter cache.** §13.2 is explicit:
//!   the row lock and the conditional update are the source of truth. Redis holds rate
//!   limits, not stock.
//! * **Unlimited is a bounded expression.** §8.4 refuses `total_quantity = MAXINT`; an
//!   unlimited campaign names the operational ceiling it is allowed to reach.

pub mod audience;
pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{ActorType, AuditEntry, AuditService};
use crate::catalog::CatalogService;
use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::jobs::{JobKey, JobService, JobSpec};
use crate::loyalty::BenefitType;
use crate::redemptions::discount::{
    Benefit, CONDITION_SCHEMA_VERSION, CouponConditions, LocalTimeRange,
};
use crate::stores::business_day::resolve_timezone;
use crate::stores::{OwnedStore, StoreService};

pub use audience::{AudienceCriteria, AudienceType};
pub use routes::{campaign_claim_router, owner_campaign_router};

/// The largest total a campaign may name, unlimited or not (§8.4, CAMPAIGN-002).
///
/// Mirrors `ck_campaign_total_quantity` and `ck_campaign_unlimited_cap`. §8.4 forbids
/// using the integer maximum as "unlimited", so this is what a campaign that declines to
/// name a number gets bounded by instead.
pub const MAX_TOTAL_QUANTITY: i64 = 1_000_000;

/// `coupon.campaign_status` (§4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CampaignStatus {
    Draft,
    Scheduled,
    Issuing,
    Paused,
    Ended,
    Cancelled,
}

impl CampaignStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            CampaignStatus::Draft => "DRAFT",
            CampaignStatus::Scheduled => "SCHEDULED",
            CampaignStatus::Issuing => "ISSUING",
            CampaignStatus::Paused => "PAUSED",
            CampaignStatus::Ended => "ENDED",
            CampaignStatus::Cancelled => "CANCELLED",
        }
    }

    /// An unknown status reads as `CANCELLED`: a state this build cannot reason about
    /// must not be one that issues coupons.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "DRAFT" => CampaignStatus::Draft,
            "SCHEDULED" => CampaignStatus::Scheduled,
            "ISSUING" => CampaignStatus::Issuing,
            "PAUSED" => CampaignStatus::Paused,
            "ENDED" => CampaignStatus::Ended,
            _ => CampaignStatus::Cancelled,
        }
    }

    /// Whether anything has been published to customers yet. Once true, §8.5 restricts
    /// what may still be edited.
    pub fn is_published(self) -> bool {
        !matches!(self, CampaignStatus::Draft)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, CampaignStatus::Ended | CampaignStatus::Cancelled)
    }
}

/// `coupon.issue_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueMode {
    /// 대상자 직접 지급 — a worker issues to everyone in the audience (CAMPAIGN-003).
    Direct,
    /// 선착순 받기 — the consumer claims (CAMPAIGN-004).
    FirstCome,
}

impl IssueMode {
    pub fn as_db(self) -> &'static str {
        match self {
            IssueMode::Direct => "DIRECT",
            IssueMode::FirstCome => "FIRST_COME",
        }
    }

    pub fn from_db(raw: &str) -> Self {
        match raw {
            "FIRST_COME" => IssueMode::FirstCome,
            _ => IssueMode::Direct,
        }
    }
}

/// What cancelling does to coupons already in wallets (CAMPAIGN-007).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevokePolicy {
    /// 신규 발급만 중단하고 기존 쿠폰 유지. The default everywhere: taking a coupon back
    /// is the destructive choice and is never what an unset field means (CAMPAIGN-007).
    #[default]
    KeepIssued,
    /// 미사용 쿠폰 전부 회수. `USED` coupons are never touched.
    RevokeUnused,
}

impl RevokePolicy {
    pub fn as_db(self) -> &'static str {
        match self {
            RevokePolicy::KeepIssued => "KEEP_ISSUED",
            RevokePolicy::RevokeUnused => "REVOKE_UNUSED",
        }
    }

    /// Unknown reads as `KEEP_ISSUED`: taking a coupon away is the destructive option, so
    /// it is never the fallback.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "REVOKE_UNUSED" => RevokePolicy::RevokeUnused,
            _ => RevokePolicy::KeepIssued,
        }
    }
}

/// §8.4's two total-quantity expressions, kept apart on the wire.
///
/// The alternative — a nullable number where `null` means unlimited — is exactly what
/// §8.4 warns against, because "no ceiling named" and "no ceiling at all" then look the
/// same to every reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "mode", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TotalQuantity {
    Limited { quantity: i64 },
    Unlimited { operational_cap: i64 },
}

impl TotalQuantity {
    /// The number the counters are actually checked against.
    pub fn effective_cap(self) -> i64 {
        match self {
            TotalQuantity::Limited { quantity } => quantity,
            TotalQuantity::Unlimited { operational_cap } => operational_cap,
        }
    }

    pub fn is_unlimited(self) -> bool {
        matches!(self, TotalQuantity::Unlimited { .. })
    }

    fn columns(self) -> (Option<i64>, Option<i64>) {
        match self {
            TotalQuantity::Limited { quantity } => (Some(quantity), None),
            TotalQuantity::Unlimited { operational_cap } => (None, Some(operational_cap)),
        }
    }

    fn from_columns(total_quantity: Option<i64>, unlimited_total_cap: Option<i64>) -> Self {
        match (total_quantity, unlimited_total_cap) {
            (Some(quantity), _) => TotalQuantity::Limited { quantity },
            (None, Some(operational_cap)) => TotalQuantity::Unlimited { operational_cap },
            // `ck_campaign_quantity_expression` makes this unreachable; falling back to
            // the ceiling rather than to "infinite" keeps it safe if it ever is not.
            (None, None) => TotalQuantity::Unlimited {
                operational_cap: MAX_TOTAL_QUANTITY,
            },
        }
    }

    fn validate(self) -> ApiResult<()> {
        let value = self.effective_cap();
        if !(1..=MAX_TOTAL_QUANTITY).contains(&value) {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                format!("총 발급 수량은 1~{MAX_TOTAL_QUANTITY}장이어야 합니다."),
            ));
        }
        Ok(())
    }
}

/// A campaign as both the owner app and the admin app read it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Campaign {
    pub id: Uuid,
    pub store_id: Uuid,
    pub status: CampaignStatus,
    pub version_no: i32,
    pub name: String,
    pub customer_description: String,
    pub benefit: Benefit,
    pub minimum_order_amount: i64,
    pub eligible_item_ids: Vec<Uuid>,
    pub eligible_category_ids: Vec<Uuid>,
    pub excluded_item_ids: Vec<Uuid>,
    pub issue_mode: IssueMode,
    pub audience_type: AudienceType,
    pub audience_criteria: AudienceCriteria,
    pub audience_size: Option<i32>,
    pub audience_snapshot_at: Option<DateTime<Utc>>,
    pub total_quantity: TotalQuantity,
    pub per_user_quantity: i16,
    pub per_business_day_quantity: Option<i64>,
    /// Fixed at creation and never editable afterwards (§8.4).
    pub restore_quantity_on_revoke: bool,
    pub revoke_policy: RevokePolicy,
    pub issued_count: i64,
    pub reserved_count: i64,
    pub revoked_count: i64,
    /// `effective_cap - (issued + reserved)`.
    pub remaining_quantity: i64,
    pub issue_starts_at: DateTime<Utc>,
    pub issue_ends_at: DateTime<Utc>,
    pub usable_from: Option<DateTime<Utc>>,
    pub usable_until: Option<DateTime<Utc>>,
    pub relative_validity_days: Option<i16>,
    pub allowed_weekdays: Vec<i16>,
    pub allowed_local_time_ranges: Vec<LocalTimeRange>,
    pub external_discount_stackable: bool,
    pub notification_channels: Vec<String>,
    pub notification_message: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancellation_reason: Option<String>,
    pub emergency_stopped_at: Option<DateTime<Utc>>,
    pub issue_generation: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CampaignsResponse {
    pub campaigns: Vec<Campaign>,
}

/// The campaign draft form (CAMPAIGN-001), in the order the wizard collects it:
/// 혜택 → 사용 조건 → 대상 → 수량 → 일정 → 알림.
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreateCampaignRequest {
    #[validate(length(min = 1, max = 160, message = "캠페인 이름은 1~160자여야 합니다."))]
    pub name: String,
    #[validate(length(min = 1, max = 2000, message = "고객에게 보여줄 설명을 입력해 주세요."))]
    pub customer_description: String,

    // 혜택 (§8.2).
    pub benefit: Benefit,

    // 사용 조건 (§8.3).
    #[serde(default)]
    #[validate(range(min = 0, max = 100_000_000, message = "최소 주문 금액을 확인해 주세요."))]
    pub minimum_order_amount: i64,
    #[serde(default)]
    pub eligible_item_ids: Vec<Uuid>,
    #[serde(default)]
    pub eligible_category_ids: Vec<Uuid>,
    #[serde(default)]
    pub excluded_item_ids: Vec<Uuid>,
    #[serde(default)]
    pub allowed_weekdays: Vec<i16>,
    #[serde(default)]
    pub allowed_local_time_ranges: Vec<LocalTimeRange>,
    #[serde(default)]
    pub external_discount_stackable: bool,

    // 대상 (CAMPAIGN-001).
    pub issue_mode: IssueMode,
    #[serde(default)]
    pub audience_type: AudienceType,
    #[serde(default)]
    pub audience_criteria: AudienceCriteria,

    // 수량 (§8.4).
    pub total_quantity: TotalQuantity,
    #[serde(default = "default_per_user_quantity")]
    #[validate(range(min = 1, max = 100, message = "1인당 수량은 1~100장이어야 합니다."))]
    pub per_user_quantity: i16,
    #[validate(range(min = 1, max = 1_000_000, message = "영업일당 수량을 확인해 주세요."))]
    pub per_business_day_quantity: Option<i64>,
    /// §8.4: fixed on the campaign, default false. Not editable afterwards.
    #[serde(default)]
    pub restore_quantity_on_revoke: bool,

    // 일정 (§8.5).
    pub issue_starts_at: DateTime<Utc>,
    pub issue_ends_at: DateTime<Utc>,
    pub usable_from: Option<DateTime<Utc>>,
    pub usable_until: Option<DateTime<Utc>>,
    #[validate(range(min = 1, max = 365, message = "상대 유효기간은 1~365일이어야 합니다."))]
    pub relative_validity_days: Option<i16>,

    // 알림 (§15).
    #[serde(default)]
    pub notification_channels: Vec<String>,
    #[validate(length(max = 1000))]
    pub notification_message: Option<String>,
}

fn default_per_user_quantity() -> i16 {
    1
}

/// A draft edit, or one of the few post-publication changes CAMPAIGN-008 allows.
#[derive(Debug, Clone, Default, Deserialize, ToSchema, Validate)]
pub struct UpdateCampaignRequest {
    #[validate(length(min = 1, max = 160))]
    pub name: Option<String>,
    #[validate(length(min = 1, max = 2000))]
    pub customer_description: Option<String>,

    // Retroactive-if-applied fields. Editable while `DRAFT`, refused afterwards (§8.5).
    pub benefit: Option<Benefit>,
    #[validate(range(min = 0, max = 100_000_000))]
    pub minimum_order_amount: Option<i64>,
    pub eligible_item_ids: Option<Vec<Uuid>>,
    pub eligible_category_ids: Option<Vec<Uuid>>,
    pub excluded_item_ids: Option<Vec<Uuid>>,
    pub allowed_weekdays: Option<Vec<i16>>,
    pub allowed_local_time_ranges: Option<Vec<LocalTimeRange>>,
    pub usable_from: Option<DateTime<Utc>>,
    pub usable_until: Option<DateTime<Utc>>,
    #[validate(range(min = 1, max = 365))]
    pub relative_validity_days: Option<i16>,
    pub issue_starts_at: Option<DateTime<Utc>>,
    pub issue_mode: Option<IssueMode>,
    pub audience_type: Option<AudienceType>,
    pub audience_criteria: Option<AudienceCriteria>,

    // Forward-looking fields, editable after publication (CAMPAIGN-008).
    pub total_quantity: Option<TotalQuantity>,
    #[validate(range(min = 1, max = 100))]
    pub per_user_quantity: Option<i16>,
    #[validate(range(min = 1, max = 1_000_000))]
    pub per_business_day_quantity: Option<i64>,
    pub issue_ends_at: Option<DateTime<Utc>>,
    #[validate(length(max = 1000))]
    pub notification_message: Option<String>,

    pub version: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema, Validate)]
pub struct CancelCampaignRequest {
    /// CAMPAIGN-007: 발급 후 취소는 회수 정책을 명시한다.
    #[serde(default = "default_revoke_policy")]
    pub revoke_policy: RevokePolicy,
    #[validate(length(min = 1, max = 1000, message = "취소 사유를 입력해 주세요."))]
    pub reason: Option<String>,
}

fn default_revoke_policy() -> RevokePolicy {
    RevokePolicy::KeepIssued
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema, Validate)]
pub struct PauseCampaignRequest {
    #[validate(length(max = 1000))]
    pub reason: Option<String>,
}

/// What publishing would cost, shown before the confirmation modal (CAMPAIGN-002).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PublishEstimate {
    pub audience_size: i64,
    /// 최대 발급 비용의 참고값 — the ceiling, not a prediction.
    pub maximum_issued_quantity: i64,
    pub maximum_discount_cost: Option<i64>,
    pub estimated_notification_count: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PublishedCampaign {
    #[serde(flatten)]
    pub campaign: Campaign,
    pub estimate: PublishEstimate,
    /// The issuing job, for `DIRECT` campaigns. `None` for 선착순.
    pub job_id: Option<Uuid>,
}

/// The answer to `POST /campaigns/:id/claims` (§11.3).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ClaimedCoupon {
    pub coupon_id: Uuid,
    pub campaign_id: Uuid,
    pub store_id: Uuid,
    pub title: String,
    pub usable_from: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// True when this call returned a coupon the customer already had. §11.3 requires a
    /// repeat request to answer with the existing coupon id rather than an error.
    pub already_claimed: bool,
    /// What is left after this claim, or `None` while the campaign is unlimited.
    pub remaining_quantity: Option<i64>,
}

pub struct CampaignService {
    stores: Arc<StoreService>,
    catalog: Arc<CatalogService>,
    audit: Arc<AuditService>,
    jobs: Arc<JobService>,
}

impl CampaignService {
    pub fn new(
        stores: Arc<StoreService>,
        catalog: Arc<CatalogService>,
        audit: Arc<AuditService>,
        jobs: Arc<JobService>,
    ) -> Self {
        Self {
            stores,
            catalog,
            audit,
            jobs,
        }
    }

    /// `GET /owner/campaigns`.
    pub async fn list(&self, pool: &PgPool, store_id: Uuid) -> ApiResult<CampaignsResponse> {
        Ok(CampaignsResponse {
            campaigns: load(pool, store_id, None).await?,
        })
    }

    /// One campaign, scoped to its store so an id from another shop reads as absent
    /// (SEC-001).
    pub async fn find<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
        campaign_id: Uuid,
    ) -> ApiResult<Campaign>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        load(executor, store_id, Some(campaign_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::new(ErrorCode::CampaignNotFound))
    }

    /// `POST /owner/campaigns` — the draft (CAMPAIGN-001).
    pub async fn create(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        request: &CreateCampaignRequest,
    ) -> ApiResult<Campaign> {
        request.benefit.validate()?;
        request.total_quantity.validate()?;
        validate_schedule(
            request.issue_starts_at,
            request.issue_ends_at,
            request.usable_from,
            request.usable_until,
            request.relative_validity_days,
        )?;
        validate_weekdays(&request.allowed_weekdays)?;

        // Every catalogue id must belong to this store and still be selectable (§8.3).
        // Checked here rather than only at publish so the wizard fails on the step that
        // owns the mistake.
        self.ensure_items(pool, store.id, request).await?;

        let (total_quantity, unlimited_cap) = request.total_quantity.columns();

        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.campaigns
                (store_id, status, name, customer_description, benefit_type, fixed_amount,
                 percentage, maximum_discount_amount, free_item_ids, minimum_order_amount,
                 eligible_item_ids, eligible_category_ids, excluded_item_ids, issue_mode,
                 audience_type, audience_criteria, total_quantity, unlimited_total_cap,
                 per_user_quantity, per_business_day_quantity, restore_quantity_on_revoke,
                 issue_starts_at, issue_ends_at, usable_from, usable_until,
                 relative_validity_days, allowed_weekdays, allowed_local_time_ranges,
                 external_discount_stackable, notification_channels, notification_message,
                 created_by_user_id)
            VALUES ($1, 'DRAFT', $2, $3, $4::text::coupon.benefit_type, $5, $6, $7, $8, $9,
                    $10, $11, $12, $13::text::coupon.issue_mode, $14, $15, $16, $17, $18,
                    $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31)
            RETURNING id
            "#,
            store.id,
            request.name.trim(),
            request.customer_description.trim(),
            request.benefit.benefit_type.as_db(),
            request.benefit.fixed_amount,
            request.benefit.percentage,
            request.benefit.maximum_discount_amount,
            &request.benefit.free_item_ids,
            request.minimum_order_amount,
            &request.eligible_item_ids,
            &request.eligible_category_ids,
            &request.excluded_item_ids,
            request.issue_mode.as_db(),
            request.audience_type.as_db(),
            serde_json::to_value(&request.audience_criteria).unwrap_or_default(),
            total_quantity,
            unlimited_cap,
            request.per_user_quantity,
            request.per_business_day_quantity,
            request.restore_quantity_on_revoke,
            request.issue_starts_at,
            request.issue_ends_at,
            request.usable_from,
            request.usable_until,
            request.relative_validity_days,
            &request.allowed_weekdays,
            serde_json::to_value(&request.allowed_local_time_ranges).unwrap_or_default(),
            request.external_discount_stackable,
            serde_json::to_value(&request.notification_channels).unwrap_or_default(),
            request.notification_message.as_deref(),
            owner_user_id,
        )
        .fetch_one(pool)
        .await?;

        self.find(pool, store.id, id).await
    }

    /// `PATCH /owner/campaigns/:id`.
    ///
    /// The interesting half is what it *refuses*. §8.5 and product principle 4 say a
    /// campaign edit must never change the meaning of a coupon someone already holds; the
    /// snapshot already guarantees that mechanically, so the remaining risk is an owner
    /// who edits the benefit and believes it applied to coupons already out there.
    /// CAMPAIGN-008 resolves it by refusing the edit and asking for a new campaign.
    pub async fn update(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        campaign_id: Uuid,
        request: &UpdateCampaignRequest,
        expected_version: Option<i64>,
    ) -> ApiResult<Campaign> {
        let mut tx = pool.begin().await?;
        let current = self.lock(&mut tx, store.id, campaign_id).await?;

        if let Some(expected) = expected_version
            && current.version != expected
        {
            return Err(ApiError::new(ErrorCode::VersionConflict));
        }

        if current.status.is_terminal() {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "종료되었거나 취소된 캠페인은 수정할 수 없습니다.",
            ));
        }

        if current.status.is_published() && request.touches_frozen_fields() {
            return Err(ApiError::new(ErrorCode::CampaignNotEditable));
        }

        if let Some(benefit) = &request.benefit {
            benefit.validate()?;
        }

        // CAMPAIGN-008: 시작 후 증량은 가능하지만 이미 발급·예약된 수량 미만으로 낮출 수 없다.
        if let Some(total) = request.total_quantity {
            total.validate()?;
            let committed = current.issued_count + current.reserved_count;
            if total.effective_cap() < committed {
                return Err(ApiError::with_message(
                    ErrorCode::QuantityBelowIssued,
                    format!("이미 {committed}장이 발급·예약되어 그보다 적게 줄일 수 없습니다."),
                ));
            }
        }

        let benefit = request.benefit.clone().unwrap_or(current.benefit.clone());
        let (total_quantity, unlimited_cap) = request
            .total_quantity
            .unwrap_or(current.total_quantity)
            .columns();

        let issue_starts_at = request.issue_starts_at.unwrap_or(current.issue_starts_at);
        let issue_ends_at = request.issue_ends_at.unwrap_or(current.issue_ends_at);
        let usable_from = request.usable_from.or(current.usable_from);
        let usable_until = request.usable_until.or(current.usable_until);
        let relative_validity_days = request
            .relative_validity_days
            .or(current.relative_validity_days);

        validate_schedule(
            issue_starts_at,
            issue_ends_at,
            usable_from,
            usable_until,
            relative_validity_days,
        )?;

        if let Some(weekdays) = &request.allowed_weekdays {
            validate_weekdays(weekdays)?;
        }

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET name = COALESCE($3, name),
                customer_description = COALESCE($4, customer_description),
                benefit_type = $5::text::coupon.benefit_type,
                fixed_amount = $6,
                percentage = $7,
                maximum_discount_amount = $8,
                free_item_ids = $9,
                minimum_order_amount = COALESCE($10, minimum_order_amount),
                eligible_item_ids = COALESCE($11, eligible_item_ids),
                eligible_category_ids = COALESCE($12, eligible_category_ids),
                excluded_item_ids = COALESCE($13, excluded_item_ids),
                issue_mode = COALESCE($14::text::coupon.issue_mode, issue_mode),
                audience_type = COALESCE($15, audience_type),
                audience_criteria = COALESCE($16, audience_criteria),
                total_quantity = $17,
                unlimited_total_cap = $18,
                per_user_quantity = COALESCE($19, per_user_quantity),
                per_business_day_quantity = COALESCE($20, per_business_day_quantity),
                issue_starts_at = $21,
                issue_ends_at = $22,
                usable_from = $23,
                usable_until = $24,
                relative_validity_days = $25,
                allowed_weekdays = COALESCE($26, allowed_weekdays),
                allowed_local_time_ranges = COALESCE($27, allowed_local_time_ranges),
                notification_message = COALESCE($28, notification_message),
                version_no = version_no + 1
            WHERE id = $1 AND store_id = $2
            "#,
            campaign_id,
            store.id,
            request.name.as_deref().map(str::trim),
            request.customer_description.as_deref().map(str::trim),
            benefit.benefit_type.as_db(),
            benefit.fixed_amount,
            benefit.percentage,
            benefit.maximum_discount_amount,
            &benefit.free_item_ids,
            request.minimum_order_amount,
            request.eligible_item_ids.as_deref(),
            request.eligible_category_ids.as_deref(),
            request.excluded_item_ids.as_deref(),
            request.issue_mode.map(IssueMode::as_db),
            request.audience_type.map(AudienceType::as_db),
            request
                .audience_criteria
                .as_ref()
                .map(|criteria| serde_json::to_value(criteria).unwrap_or_default()),
            total_quantity,
            unlimited_cap,
            request.per_user_quantity,
            request.per_business_day_quantity,
            issue_starts_at,
            issue_ends_at,
            usable_from,
            usable_until,
            relative_validity_days,
            request.allowed_weekdays.as_deref(),
            request
                .allowed_local_time_ranges
                .as_ref()
                .map(|ranges| serde_json::to_value(ranges).unwrap_or_default()),
            request.notification_message.as_deref(),
        )
        .execute(&mut *tx)
        .await?;

        let updated = self.find(&mut *tx, store.id, campaign_id).await?;
        tx.commit().await?;

        Ok(updated)
    }

    /// What publishing would commit the store to (CAMPAIGN-002).
    pub async fn estimate(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        campaign_id: Uuid,
    ) -> ApiResult<PublishEstimate> {
        let campaign = self.find(pool, store.id, campaign_id).await?;
        let audience_size = audience::size(pool, store.id, &campaign).await?;

        Ok(build_estimate(&campaign, audience_size))
    }

    /// `POST /owner/campaigns/:id/publish` (§11.4, CAMPAIGN-002, CAMPAIGN-003).
    ///
    /// The whole thing is one transaction: the status change, the audience size, and the
    /// registration of the issuing job commit together. §14.2's outbox is what closes the
    /// remaining gap — a Redis outage after this commit delays the job rather than
    /// leaving a published campaign nobody will ever issue.
    pub async fn publish(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        campaign_id: Uuid,
    ) -> ApiResult<PublishedCampaign> {
        store.ensure_operating()?;

        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;
        let campaign = self.lock(&mut tx, store.id, campaign_id).await?;

        if campaign.status != CampaignStatus::Draft {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "초안 상태의 캠페인만 게시할 수 있습니다.",
            ));
        }

        // CAMPAIGN-002, every rule, at the last moment where refusing is still cheap.
        campaign.benefit.validate()?;
        campaign.total_quantity.validate()?;
        validate_schedule(
            campaign.issue_starts_at,
            campaign.issue_ends_at,
            campaign.usable_from,
            campaign.usable_until,
            campaign.relative_validity_days,
        )?;
        if campaign.issue_ends_at <= now {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "이미 지난 발급 종료 시각으로는 게시할 수 없습니다.",
            ));
        }
        // 무료 품목은 하나 이상의 *활성* 품목을 지정한다.
        if campaign.benefit.benefit_type == BenefitType::FreeItem {
            self.catalog
                .ensure_selectable(&mut *tx, store.id, &campaign.benefit.free_item_ids)
                .await?;
        }
        audience::validate(&campaign)?;

        let audience_size = audience::size(&mut *tx, store.id, &campaign).await?;

        let status = if campaign.issue_starts_at > now {
            CampaignStatus::Scheduled
        } else {
            CampaignStatus::Issuing
        };

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET status = $2::text::coupon.campaign_status,
                published_at = $3,
                audience_size = $4
            WHERE id = $1
            "#,
            campaign_id,
            status.as_db(),
            now,
            i32::try_from(audience_size).unwrap_or(i32::MAX),
        )
        .execute(&mut *tx)
        .await?;

        // CAMPAIGN-003 step 1: 게시 트랜잭션이 발급 작업을 한 번만 등록한다. 선착순은
        // 소비자가 직접 받으므로 등록할 작업이 없다.
        let job_id = if campaign.issue_mode == IssueMode::Direct {
            let spec = JobSpec::new(
                JobKey::build_audience(store.id, campaign_id, campaign.issue_generation),
                serde_json::json!({
                    "campaign_id": campaign_id,
                    "store_id": store.id,
                    "generation": campaign.issue_generation,
                }),
            )
            .store(store.id)
            .resource(campaign_id)
            .requested_by(owner_user_id)
            .at(campaign.issue_starts_at.max(now));

            Some(self.jobs.enqueue(&mut tx, &spec).await?.job_id)
        } else {
            None
        };

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::StoreOwner, "campaign.published", "campaign")
                    .actor(owner_user_id)
                    .resource(campaign_id)
                    .store(store.id)
                    .transition(
                        &serde_json::json!({ "status": "DRAFT" }),
                        &serde_json::json!({ "status": status.as_db() }),
                    )
                    .metadata(serde_json::json!({
                        "audience_size": audience_size,
                        "issue_mode": campaign.issue_mode.as_db(),
                        "job_id": job_id,
                    })),
            )
            .await?;

        let published = self.find(&mut *tx, store.id, campaign_id).await?;
        tx.commit().await?;

        tracing::info!(
            %campaign_id,
            store_id = %store.id,
            status = ?status,
            audience_size,
            "campaign.published"
        );

        Ok(PublishedCampaign {
            estimate: build_estimate(&published, audience_size),
            campaign: published,
            job_id,
        })
    }

    /// `POST /owner/campaigns/:id/pause` (CAMPAIGN-006).
    ///
    /// Stops new issuance only. Coupons already in wallets are untouched — that is the
    /// difference between pausing and cancelling, and the reason pausing needs no
    /// confirmation ceremony.
    pub async fn pause(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        campaign_id: Uuid,
        request: &PauseCampaignRequest,
    ) -> ApiResult<Campaign> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;
        let campaign = self.lock(&mut tx, store.id, campaign_id).await?;

        if !matches!(
            campaign.status,
            CampaignStatus::Scheduled | CampaignStatus::Issuing
        ) {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "게시 중인 캠페인만 중지할 수 있습니다.",
            ));
        }

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET status = 'PAUSED', paused_at = $2
            WHERE id = $1
            "#,
            campaign_id,
            now,
        )
        .execute(&mut *tx)
        .await?;

        // The batch worker asks between batches and stops on a boundary of its own
        // choosing (CAMPAIGN-006), so this only records the intent.
        let paused_job = self.pause_active_job(&mut tx, campaign_id).await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::StoreOwner, "campaign.paused", "campaign")
                    .actor(owner_user_id)
                    .resource(campaign_id)
                    .store(store.id)
                    .reason(request.reason.clone().unwrap_or_else(|| "점주 중지".to_owned()))
                    .metadata(serde_json::json!({ "job_id": paused_job })),
            )
            .await?;

        let paused = self.find(&mut *tx, store.id, campaign_id).await?;
        tx.commit().await?;

        Ok(paused)
    }

    /// `POST /owner/campaigns/:id/resume` (CAMPAIGN-006).
    ///
    /// Same job key, same checkpoint: issuing continues from the first target it had not
    /// reached rather than starting the campaign over.
    pub async fn resume(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        campaign_id: Uuid,
    ) -> ApiResult<Campaign> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;
        let campaign = self.lock(&mut tx, store.id, campaign_id).await?;

        if campaign.status != CampaignStatus::Paused {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "중지된 캠페인만 재개할 수 있습니다.",
            ));
        }
        if campaign.issue_ends_at <= now {
            return Err(ApiError::with_message(
                ErrorCode::CampaignNotIssuing,
                "발급 기간이 이미 끝난 캠페인은 재개할 수 없습니다.",
            ));
        }

        let status = if campaign.issue_starts_at > now {
            CampaignStatus::Scheduled
        } else {
            CampaignStatus::Issuing
        };

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET status = $2::text::coupon.campaign_status, paused_at = NULL
            WHERE id = $1
            "#,
            campaign_id,
            status.as_db(),
        )
        .execute(&mut *tx)
        .await?;

        let resumed_job = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.job_registry
            WHERE resource_id = $1
              AND job_type IN ('build_campaign_audience', 'issue_campaign')
              AND status IN ('PAUSED', 'PAUSE_REQUESTED')
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            campaign_id,
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(job_id) = resumed_job {
            self.jobs.resume(&mut tx, job_id).await?;
        }

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::StoreOwner, "campaign.resumed", "campaign")
                    .actor(owner_user_id)
                    .resource(campaign_id)
                    .store(store.id)
                    .metadata(serde_json::json!({ "job_id": resumed_job })),
            )
            .await?;

        let resumed = self.find(&mut *tx, store.id, campaign_id).await?;
        tx.commit().await?;

        Ok(resumed)
    }

    /// `POST /owner/campaigns/:id/cancel` (CAMPAIGN-007).
    pub async fn cancel(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        campaign_id: Uuid,
        request: &CancelCampaignRequest,
    ) -> ApiResult<Campaign> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;
        let campaign = self.lock(&mut tx, store.id, campaign_id).await?;

        if campaign.status.is_terminal() {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "이미 종료되었거나 취소된 캠페인입니다.",
            ));
        }

        // 게시 전 취소는 단순 CANCELLED다. There is nothing in anyone's wallet, so asking
        // for a revocation policy would be theatre.
        let revoke_policy = if campaign.status.is_published() {
            request.revoke_policy
        } else {
            RevokePolicy::KeepIssued
        };

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET status = 'CANCELLED', cancelled_at = $2, cancellation_reason = $3,
                revoke_policy = $4
            WHERE id = $1
            "#,
            campaign_id,
            now,
            request.reason.as_deref(),
            revoke_policy.as_db(),
        )
        .execute(&mut *tx)
        .await?;

        self.pause_active_job(&mut tx, campaign_id).await?;

        let revoke_job_id = if revoke_policy == RevokePolicy::RevokeUnused {
            Some(
                self.enqueue_revocation(
                    &mut tx,
                    store.id,
                    campaign_id,
                    &format!("owner-cancel-{campaign_id}"),
                    owner_user_id,
                    request.reason.as_deref().unwrap_or("점주 캠페인 취소"),
                )
                .await?,
            )
        } else {
            None
        };

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::StoreOwner, "campaign.cancelled", "campaign")
                    .actor(owner_user_id)
                    .resource(campaign_id)
                    .store(store.id)
                    .reason(
                        request
                            .reason
                            .clone()
                            .unwrap_or_else(|| "점주 캠페인 취소".to_owned()),
                    )
                    .metadata(serde_json::json!({
                        "revoke_policy": revoke_policy.as_db(),
                        "revoke_job_id": revoke_job_id,
                    })),
            )
            .await?;

        let cancelled = self.find(&mut *tx, store.id, campaign_id).await?;
        tx.commit().await?;

        tracing::info!(%campaign_id, ?revoke_policy, "campaign.cancelled");
        Ok(cancelled)
    }

    /// `POST /admin/campaigns/:id/emergency-stop` (§11.5, ADMIN-005).
    ///
    /// Unlike the owner's pause this is not scoped to one store and does not ask whether
    /// the campaign is in a convenient state: 악성/오류 캠페인은 즉시 신규 발급을 막는다.
    pub async fn emergency_stop(
        &self,
        pool: &PgPool,
        admin_user_id: Uuid,
        campaign_id: Uuid,
        reason: &str,
    ) -> ApiResult<Campaign> {
        if reason.trim().is_empty() {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "긴급 중단 사유를 입력해야 합니다.",
            ));
        }

        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        let row = sqlx::query!(
            r#"
            SELECT store_id, status::text AS "status!"
            FROM coupon.campaigns
            WHERE id = $1
            FOR UPDATE
            "#,
            campaign_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::CampaignNotFound))?;

        if CampaignStatus::from_db(&row.status).is_terminal() {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "이미 종료된 캠페인입니다.",
            ));
        }

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET status = 'PAUSED',
                paused_at = $2,
                emergency_stopped_at = $2,
                emergency_stopped_by_user_id = $3
            WHERE id = $1
            "#,
            campaign_id,
            now,
            admin_user_id,
        )
        .execute(&mut *tx)
        .await?;

        self.pause_active_job(&mut tx, campaign_id).await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::SystemAdmin,
                    "campaign.emergency_stopped",
                    "campaign",
                )
                .actor(admin_user_id)
                .resource(campaign_id)
                .store(row.store_id)
                .reason(reason.to_owned()),
            )
            .await?;

        let stopped = self.find(&mut *tx, row.store_id, campaign_id).await?;
        tx.commit().await?;

        tracing::warn!(%campaign_id, %admin_user_id, "campaign.emergency_stopped");
        Ok(stopped)
    }

    /// `POST /admin/campaigns/:id/revoke-job` (§11.5, ADMIN-005).
    pub async fn request_revocation(
        &self,
        pool: &PgPool,
        admin_user_id: Uuid,
        campaign_id: Uuid,
        case_id: Option<Uuid>,
        reason: &str,
    ) -> ApiResult<Uuid> {
        if reason.trim().is_empty() {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "회수 사유를 입력해야 합니다.",
            ));
        }

        let mut tx = pool.begin().await?;

        let row = sqlx::query!(
            r#"SELECT store_id FROM coupon.campaigns WHERE id = $1 FOR UPDATE"#,
            campaign_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::CampaignNotFound))?;

        // ADMIN-005: 회수 작업은 동일 캠페인 키로 하나만 실행한다. The operation version
        // is the case, so a second revocation needs a second case to authorise it.
        let operation = case_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("admin-{campaign_id}"));

        let job_id = self
            .enqueue_revocation(
                &mut tx,
                row.store_id,
                campaign_id,
                &operation,
                admin_user_id,
                reason,
            )
            .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::SystemAdmin, "campaign.revoke_requested", "campaign")
                    .actor(admin_user_id)
                    .resource(campaign_id)
                    .store(row.store_id)
                    .reason(reason.to_owned())
                    .metadata(serde_json::json!({ "job_id": job_id, "case_id": case_id })),
            )
            .await?;

        tx.commit().await?;
        Ok(job_id)
    }

    /// `POST /campaigns/:id/claims` — 선착순 받기 (§13.2, CAMPAIGN-004).
    ///
    /// The §13.2 order, unabbreviated:
    ///
    /// 1. campaign row lock (after the store, keeping §13.1's global lock order)
    /// 2. `(campaign_id, business_day)` counter lock
    /// 3. the consumer's existing ordinal
    /// 4. total, daily and per-person limits — all three, all under the locks
    /// 5. dedup row, coupon, counter increment, outbox
    /// 6. a unique violation resolves to the existing coupon or to sold-out, never to a 500
    pub async fn claim(
        &self,
        pool: &PgPool,
        campaign_id: Uuid,
        user_id: Uuid,
        idempotency_key: Uuid,
    ) -> ApiResult<ClaimedCoupon> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        // §13.1's lock order is `store → campaign → user → coupon`, and it is global: a
        // claim and an accrual touching the same store must queue rather than deadlock.
        let store_id = sqlx::query_scalar!(
            "SELECT store_id FROM coupon.campaigns WHERE id = $1",
            campaign_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::CampaignNotFound))?;

        self.stores.lock_store(&mut tx, store_id).await?;
        let store = self.stores.find_public(&mut *tx, store_id).await?;
        store.ensure_operating()?;

        let campaign = self.lock(&mut tx, store_id, campaign_id).await?;

        if campaign.issue_mode != IssueMode::FirstCome {
            return Err(ApiError::with_message(
                ErrorCode::CampaignNotIssuing,
                "직접 받을 수 있는 캠페인이 아닙니다.",
            ));
        }

        match campaign.status {
            CampaignStatus::Issuing => {}
            // A scheduled campaign whose start has arrived is issuing; nobody should have
            // to wait for a batch to notice (§5.2: the server clock decides).
            CampaignStatus::Scheduled if campaign.issue_starts_at <= now => {}
            CampaignStatus::Paused => return Err(ApiError::new(ErrorCode::CampaignPaused)),
            _ => return Err(ApiError::new(ErrorCode::CampaignNotIssuing)),
        }

        // §5.2: `[start, end)`.
        if now < campaign.issue_starts_at {
            return Err(ApiError::with_message(
                ErrorCode::CampaignNotIssuing,
                "아직 발급이 시작되지 않았습니다.",
            ));
        }
        if now >= campaign.issue_ends_at {
            return Err(ApiError::with_message(
                ErrorCode::CampaignNotIssuing,
                "발급이 종료된 캠페인입니다.",
            ));
        }

        if !audience::is_eligible(&mut tx, &campaign, user_id, now).await? {
            return Err(ApiError::new(ErrorCode::AudienceNotEligible));
        }

        // Step 3. `issued_count` counts PENDING/AVAILABLE/RESERVED/USED/EXPIRED and
        // excludes ISSUE_FAILED (§8.4).
        let existing = self.existing_issuance(&mut tx, campaign_id, user_id).await?;

        // §11.3: 중복 요청이면 기존 쿠폰 ID를 반환한다. A customer who already holds
        // everything the campaign allows them is asking for the coupon they have, not
        // making an error — the button in front of them says "받기" either way.
        if existing.count >= i64::from(campaign.per_user_quantity)
            && let Some(coupon_id) = existing.latest_coupon_id
        {
            let coupon = self.describe_coupon(&mut tx, coupon_id).await?;
            tx.commit().await?;
            return Ok(ClaimedCoupon {
                already_claimed: true,
                remaining_quantity: remaining(&campaign),
                ..coupon
            });
        }

        // Step 2, and the reason this is a lock rather than a read: two claims for the
        // same campaign on the same business day must not both see the same count.
        let business_day = store.calendar()?.business_day(now);
        let counter = self
            .lock_counter(&mut tx, campaign_id, business_day)
            .await?;

        // Step 4. All three limits, all against numbers read under a lock (§12.6-4/5).
        let cap = campaign.total_quantity.effective_cap();
        if campaign.issued_count + campaign.reserved_count >= cap {
            return Err(ApiError::new(ErrorCode::CampaignSoldOut));
        }
        if let Some(daily) = campaign.per_business_day_quantity
            && counter.issued_count + counter.reserved_count >= daily
        {
            return Err(ApiError::with_message(
                ErrorCode::CampaignSoldOut,
                "오늘 준비된 수량이 모두 소진되었습니다. 내일 다시 시도해 주세요.",
            ));
        }

        // Step 5.
        let ordinal = i16::try_from(existing.count + 1).unwrap_or(i16::MAX);
        let issued = self
            .issue_instance(
                &mut tx,
                &store,
                &campaign,
                user_id,
                ordinal,
                now,
                Some(idempotency_key),
                None,
            )
            .await;

        let issued = match issued {
            Ok(issued) => issued,
            // Step 6: 고유 제약 충돌은 기존 발급 결과로 정상 변환한다. Somebody else's
            // request for the same customer won; the answer is their coupon.
            Err(error) if error.code == ErrorCode::Conflict => {
                let existing = self.existing_issuance(&mut tx, campaign_id, user_id).await?;
                let Some(coupon_id) = existing.latest_coupon_id else {
                    return Err(error);
                };
                let coupon = self.describe_coupon(&mut tx, coupon_id).await?;
                tx.commit().await?;
                return Ok(ClaimedCoupon {
                    already_claimed: true,
                    remaining_quantity: remaining(&campaign),
                    ..coupon
                });
            }
            Err(error) => return Err(error),
        };

        self.bump_counters(&mut tx, campaign_id, business_day, 1)
            .await?;

        let claimed = ClaimedCoupon {
            coupon_id: issued.coupon_id,
            campaign_id,
            store_id,
            title: issued.title,
            usable_from: issued.usable_from,
            expires_at: issued.expires_at,
            already_claimed: false,
            remaining_quantity: if campaign.total_quantity.is_unlimited() {
                None
            } else {
                Some(cap - (campaign.issued_count + campaign.reserved_count + 1))
            },
        };

        tx.commit().await?;

        tracing::info!(
            coupon_id = %claimed.coupon_id,
            %campaign_id,
            "campaign.claimed"
        );

        Ok(claimed)
    }

    // -----------------------------------------------------------------------
    // Shared with the worker (§10.2: the tables stay owned here)
    // -----------------------------------------------------------------------

    /// Take the campaign's row lock. Always called before the counter, so every writer
    /// takes the two in the same order.
    pub async fn lock(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        campaign_id: Uuid,
    ) -> ApiResult<Campaign> {
        sqlx::query_as!(
            CampaignRow,
            r#"
            SELECT
                id, store_id, status::text AS "status!", version_no, name,
                customer_description, benefit_type::text AS "benefit_type!", fixed_amount,
                percentage, maximum_discount_amount, free_item_ids, minimum_order_amount,
                eligible_item_ids, eligible_category_ids, excluded_item_ids,
                issue_mode::text AS "issue_mode!", audience_type, audience_criteria,
                audience_size, audience_snapshot_at, total_quantity, unlimited_total_cap,
                per_user_quantity, per_business_day_quantity, restore_quantity_on_revoke,
                revoke_policy, global_issued_count, global_reserved_count,
                global_revoked_count, issue_starts_at, issue_ends_at, usable_from,
                usable_until, relative_validity_days, allowed_weekdays,
                allowed_local_time_ranges, external_discount_stackable,
                notification_channels, notification_message, published_at, paused_at,
                cancelled_at, cancellation_reason, emergency_stopped_at, issue_generation,
                created_at, updated_at, version
            FROM coupon.campaigns
            WHERE id = $1 AND store_id = $2
            FOR UPDATE
            "#,
            campaign_id,
            store_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .map(Campaign::from)
        .ok_or_else(|| ApiError::new(ErrorCode::CampaignNotFound))
    }

    /// Create the day's counter row if this is the first claim, then lock it (§13.2-2).
    pub async fn lock_counter(
        &self,
        tx: &mut Tx<'_>,
        campaign_id: Uuid,
        business_day: NaiveDate,
    ) -> ApiResult<CampaignCounter> {
        sqlx::query!(
            r#"
            INSERT INTO coupon.campaign_counters (campaign_id, business_day)
            VALUES ($1, $2)
            ON CONFLICT (campaign_id, business_day) DO NOTHING
            "#,
            campaign_id,
            business_day,
        )
        .execute(&mut **tx)
        .await?;

        let row = sqlx::query!(
            r#"
            SELECT reserved_count, issued_count, revoked_count
            FROM coupon.campaign_counters
            WHERE campaign_id = $1 AND business_day = $2
            FOR UPDATE
            "#,
            campaign_id,
            business_day,
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(CampaignCounter {
            business_day,
            reserved_count: row.reserved_count,
            issued_count: row.issued_count,
            revoked_count: row.revoked_count,
        })
    }

    /// Record one more issued coupon, on both the day counter and the campaign.
    ///
    /// The campaign update is what `ck_campaign_global_counts` guards, so an issue that
    /// would cross the ceiling fails here even if every check above were wrong.
    pub async fn bump_counters(
        &self,
        tx: &mut Tx<'_>,
        campaign_id: Uuid,
        business_day: NaiveDate,
        delta: i64,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.campaign_counters
            SET issued_count = issued_count + $3
            WHERE campaign_id = $1 AND business_day = $2
            "#,
            campaign_id,
            business_day,
            delta,
        )
        .execute(&mut **tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE coupon.campaigns
            SET global_issued_count = global_issued_count + $2
            WHERE id = $1
            "#,
            campaign_id,
            delta,
        )
        .execute(&mut **tx)
        .await
        .map_err(|error| match &error {
            // `ck_campaign_global_counts`. Reaching it means the application's own
            // quantity check and the ledger disagreed — which is exactly the case §12.6-4
            // exists for, and it must read as "sold out", not as a 500.
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23514") => {
                ApiError::new(ErrorCode::CampaignSoldOut).internal(db.to_string())
            }
            _ => ApiError::from(error),
        })?;

        Ok(())
    }

    /// Create one campaign coupon, with its conditions frozen (§8.5).
    ///
    /// Shared by the claim path and the bulk-issue worker so the two cannot produce
    /// differently shaped coupons.
    #[allow(clippy::too_many_arguments)]
    pub async fn issue_instance(
        &self,
        tx: &mut Tx<'_>,
        store: &OwnedStore,
        campaign: &Campaign,
        user_id: Uuid,
        ordinal: i16,
        now: DateTime<Utc>,
        idempotency_key: Option<Uuid>,
        job_id: Option<Uuid>,
    ) -> ApiResult<IssuedCoupon> {
        let usable_from = campaign.usable_from.unwrap_or(now).max(now);
        let expires_at = expiry_for(campaign, now, &store.timezone)?;

        if expires_at <= usable_from {
            return Err(ApiError::with_message(
                ErrorCode::UnprocessableRequest,
                "사용 기간이 이미 지난 캠페인입니다.",
            ));
        }

        let conditions = conditions_of(campaign, &store.timezone);
        let condition_snapshot = serde_json::json!({
            "schema_version": CONDITION_SCHEMA_VERSION,
            "source": "CAMPAIGN",
            "store": { "id": store.id, "name": store.name, "timezone": store.timezone },
            "campaign": {
                "id": campaign.id,
                "version_no": campaign.version_no,
                "name": campaign.name,
            },
            "issued_at": now,
            "conditions": serde_json::to_value(&conditions).unwrap_or_default(),
        });

        let coupon_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.coupon_instances
                (store_id, user_id, source_type, campaign_id, issuance_ordinal, status,
                 title, description, benefit_type, usable_from, expires_at,
                 condition_snapshot, issued_at, source_job_id)
            VALUES ($1, $2, 'CAMPAIGN', $3, $4, 'AVAILABLE', $5, $6,
                    $7::text::coupon.benefit_type, $8, $9, $10, $11, $12)
            RETURNING id
            "#,
            store.id,
            user_id,
            campaign.id,
            ordinal,
            campaign.name,
            campaign.customer_description,
            campaign.benefit.benefit_type.as_db(),
            usable_from,
            expires_at,
            condition_snapshot,
            now,
            job_id,
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(map_issuance_conflict)?;

        // The dedup row is the §13.2-6 backstop: even a worker that ran twice cannot
        // produce a second coupon for the same `(campaign, user, ordinal)`.
        sqlx::query!(
            r#"
            INSERT INTO coupon.issuance_deduplications
                (campaign_id, user_id, ordinal, coupon_id, idempotency_key)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            campaign.id,
            user_id,
            ordinal,
            coupon_id,
            idempotency_key,
        )
        .execute(&mut **tx)
        .await
        .map_err(map_issuance_conflict)?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.coupon_status_events
                (coupon_id, from_status, to_status, actor_type, reason_code, metadata, occurred_at)
            VALUES ($1, NULL, 'AVAILABLE', 'SYSTEM', 'CAMPAIGN_ISSUED', $2, $3)
            "#,
            coupon_id,
            serde_json::json!({ "campaign_id": campaign.id, "ordinal": ordinal }),
            now,
        )
        .execute(&mut **tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.outbox_events
                (aggregate_type, aggregate_id, aggregate_version, event_type, correlation_id, payload)
            VALUES ('coupon_instance', $1, 1, 'CAMPAIGN_COUPON_ISSUED', $2, $3)
            ON CONFLICT (aggregate_type, aggregate_id, aggregate_version, event_type) DO NOTHING
            "#,
            coupon_id,
            Uuid::new_v4(),
            serde_json::json!({
                "store_id": store.id,
                "user_id": user_id,
                "campaign_id": campaign.id,
                "expires_at": expires_at,
            }),
        )
        .execute(&mut **tx)
        .await?;

        Ok(IssuedCoupon {
            coupon_id,
            title: campaign.name.clone(),
            usable_from,
            expires_at,
        })
    }

    /// How many instances a consumer already holds from this campaign (§8.4).
    pub async fn existing_issuance(
        &self,
        tx: &mut Tx<'_>,
        campaign_id: Uuid,
        user_id: Uuid,
    ) -> ApiResult<ExistingIssuance> {
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) AS "count!",
                MAX(issuance_ordinal) AS "max_ordinal?",
                (ARRAY_AGG(id ORDER BY created_at DESC))[1] AS "latest?"
            FROM coupon.coupon_instances
            WHERE campaign_id = $1 AND user_id = $2 AND status <> 'ISSUE_FAILED'
            "#,
            campaign_id,
            user_id,
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(ExistingIssuance {
            count: row.count,
            // The next ordinal follows the highest one used, not the count: a failed
            // issuance leaves a gap, and reusing the gap would collide with the dedup row
            // that is still there.
            next_ordinal: row.max_ordinal.unwrap_or(0) + 1,
            latest_coupon_id: row.latest,
        })
    }

    async fn describe_coupon(&self, tx: &mut Tx<'_>, coupon_id: Uuid) -> ApiResult<ClaimedCoupon> {
        let row = sqlx::query!(
            r#"
            SELECT id, store_id, campaign_id, title, usable_from, expires_at
            FROM coupon.coupon_instances
            WHERE id = $1
            "#,
            coupon_id,
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(ClaimedCoupon {
            coupon_id: row.id,
            campaign_id: row.campaign_id.unwrap_or_default(),
            store_id: row.store_id,
            title: row.title,
            usable_from: row.usable_from,
            expires_at: row.expires_at,
            already_claimed: true,
            remaining_quantity: None,
        })
    }

    async fn ensure_items(
        &self,
        pool: &PgPool,
        store_id: Uuid,
        request: &CreateCampaignRequest,
    ) -> ApiResult<()> {
        self.catalog
            .ensure_selectable(pool, store_id, &request.eligible_item_ids)
            .await?;
        self.catalog
            .ensure_selectable(pool, store_id, &request.excluded_item_ids)
            .await?;
        self.catalog
            .ensure_selectable(pool, store_id, &request.benefit.free_item_ids)
            .await?;
        self.catalog
            .ensure_categories_selectable(pool, store_id, &request.eligible_category_ids)
            .await?;
        Ok(())
    }

    /// Ask the campaign's in-flight issuing job to stop between batches.
    async fn pause_active_job(
        &self,
        tx: &mut Tx<'_>,
        campaign_id: Uuid,
    ) -> ApiResult<Option<Uuid>> {
        let job_id = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.job_registry
            WHERE resource_id = $1
              AND job_type IN ('build_campaign_audience', 'issue_campaign')
              AND status IN ('PENDING_OUTBOX', 'QUEUED', 'RUNNING', 'RETRY_WAIT')
            ORDER BY created_at DESC
            LIMIT 1
            "#,
            campaign_id,
        )
        .fetch_optional(&mut **tx)
        .await?;

        if let Some(job_id) = job_id {
            sqlx::query!(
                r#"
                UPDATE coupon.job_registry
                SET status = CASE
                        WHEN status = 'RUNNING' THEN 'PAUSE_REQUESTED'::coupon.job_status
                        ELSE 'PAUSED'::coupon.job_status
                    END,
                    pause_requested_at = clock_timestamp(),
                    paused_at = CASE WHEN status <> 'RUNNING' THEN clock_timestamp() ELSE NULL END
                WHERE id = $1
                "#,
                job_id,
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(job_id)
    }

    async fn enqueue_revocation(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        campaign_id: Uuid,
        operation: &str,
        actor_user_id: Uuid,
        reason: &str,
    ) -> ApiResult<Uuid> {
        let spec = JobSpec::new(
            JobKey::revoke_campaign(store_id, campaign_id, operation),
            serde_json::json!({
                "campaign_id": campaign_id,
                "store_id": store_id,
                "reason": reason,
            }),
        )
        .store(store_id)
        .resource(campaign_id)
        .requested_by(actor_user_id);

        Ok(self.jobs.enqueue(tx, &spec).await?.job_id)
    }
}

/// One campaign's counters for one business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CampaignCounter {
    pub business_day: NaiveDate,
    pub reserved_count: i64,
    pub issued_count: i64,
    pub revoked_count: i64,
}

/// What a consumer already holds from one campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingIssuance {
    pub count: i64,
    pub next_ordinal: i16,
    pub latest_coupon_id: Option<Uuid>,
}

/// A freshly created instance.
#[derive(Debug, Clone)]
pub struct IssuedCoupon {
    pub coupon_id: Uuid,
    pub title: String,
    pub usable_from: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl UpdateCampaignRequest {
    /// Whether this edit would change what an already-issued coupon means (§8.5,
    /// CAMPAIGN-008).
    ///
    /// `per_user_quantity` and the quantities are deliberately *not* here: CAMPAIGN-008
    /// says reducing a personal limit applies to new issuance and does not claw anything
    /// back, which is a forward-looking change.
    pub fn touches_frozen_fields(&self) -> bool {
        self.benefit.is_some()
            || self.minimum_order_amount.is_some()
            || self.eligible_item_ids.is_some()
            || self.eligible_category_ids.is_some()
            || self.excluded_item_ids.is_some()
            || self.allowed_weekdays.is_some()
            || self.allowed_local_time_ranges.is_some()
            || self.usable_from.is_some()
            || self.usable_until.is_some()
            || self.relative_validity_days.is_some()
            || self.issue_starts_at.is_some()
            || self.issue_mode.is_some()
            || self.audience_type.is_some()
            || self.audience_criteria.is_some()
    }
}

/// The conditions a coupon issued from this campaign carries for the rest of its life.
pub fn conditions_of(campaign: &Campaign, timezone: &str) -> CouponConditions {
    CouponConditions {
        schema_version: CONDITION_SCHEMA_VERSION,
        benefit: campaign.benefit.clone(),
        minimum_order_amount: campaign.minimum_order_amount,
        eligible_item_ids: campaign.eligible_item_ids.clone(),
        eligible_category_ids: campaign.eligible_category_ids.clone(),
        excluded_item_ids: campaign.excluded_item_ids.clone(),
        allowed_weekdays: campaign.allowed_weekdays.clone(),
        allowed_local_time_ranges: campaign.allowed_local_time_ranges.clone(),
        timezone: timezone.to_owned(),
        external_discount_stackable: campaign.external_discount_stackable,
    }
}

/// When a coupon issued *now* stops being usable (§8.5, CAMPAIGN-005).
///
/// Two clocks can end a coupon and the earlier one wins. The relative one is measured in
/// the store's local calendar — `issued_at + N days` at the same wall-clock time — because
/// "일주일 뒤까지" means seven local days to the customer, not 168 hours.
pub fn expiry_for(
    campaign: &Campaign,
    issued_at: DateTime<Utc>,
    timezone: &str,
) -> ApiResult<DateTime<Utc>> {
    let relative = campaign.relative_validity_days.map(|days| {
        let zone = resolve_timezone(timezone);
        let local = issued_at.with_timezone(&zone);
        local
            .checked_add_days(Days::new(u64::from(days.max(0) as u16)))
            .map(|local| local.with_timezone(&Utc))
            // A date arithmetic overflow is not a reason to hand out an immortal coupon.
            .unwrap_or(issued_at)
    });

    match (campaign.usable_until, relative) {
        (Some(absolute), Some(relative)) => Ok(absolute.min(relative)),
        (Some(absolute), None) => Ok(absolute),
        (None, Some(relative)) => Ok(relative),
        // `ck_campaign_validity_mode` makes this unreachable; a coupon with no end at all
        // is not something to invent a default for.
        (None, None) => Err(ApiError::with_message(
            ErrorCode::UnprocessableRequest,
            "캠페인에 사용 종료 조건이 없습니다.",
        )),
    }
}

fn remaining(campaign: &Campaign) -> Option<i64> {
    if campaign.total_quantity.is_unlimited() {
        None
    } else {
        Some(
            (campaign.total_quantity.effective_cap()
                - campaign.issued_count
                - campaign.reserved_count)
                .max(0),
        )
    }
}

fn build_estimate(campaign: &Campaign, audience_size: i64) -> PublishEstimate {
    let cap = campaign.total_quantity.effective_cap();
    let per_person_ceiling = audience_size.saturating_mul(i64::from(campaign.per_user_quantity));
    let maximum_issued_quantity = match campaign.issue_mode {
        // 선착순 is open to whoever qualifies, so the quantity itself is the ceiling.
        IssueMode::FirstCome => cap,
        IssueMode::Direct => cap.min(per_person_ceiling),
    };

    // 최대 발급 비용의 참고값. Only meaningful where the benefit has a monetary ceiling —
    // a free-item coupon's cost depends on what the customer orders, and inventing a
    // number for it would be worse than admitting we cannot say.
    let unit_cost = match campaign.benefit.benefit_type {
        BenefitType::FixedAmount => campaign.benefit.fixed_amount,
        BenefitType::Percentage => campaign.benefit.maximum_discount_amount,
        BenefitType::FreeItem => None,
    };

    PublishEstimate {
        audience_size,
        maximum_issued_quantity,
        maximum_discount_cost: unit_cost.map(|cost| cost.saturating_mul(maximum_issued_quantity)),
        estimated_notification_count: if campaign.notification_channels.is_empty() {
            0
        } else {
            audience_size.saturating_mul(campaign.notification_channels.len() as i64)
        },
    }
}

/// CAMPAIGN-002's schedule rules.
fn validate_schedule(
    issue_starts_at: DateTime<Utc>,
    issue_ends_at: DateTime<Utc>,
    usable_from: Option<DateTime<Utc>>,
    usable_until: Option<DateTime<Utc>>,
    relative_validity_days: Option<i16>,
) -> ApiResult<()> {
    if issue_ends_at <= issue_starts_at {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "발급 종료는 발급 시작보다 늦어야 합니다.",
        ));
    }

    // 사용 종료는 발급 시작보다 늦어야 한다 — otherwise the campaign issues coupons that
    // were already expired when they were created.
    if let Some(until) = usable_until
        && until <= issue_starts_at
    {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "사용 종료는 발급 시작보다 늦어야 합니다.",
        ));
    }

    if let (Some(from), Some(until)) = (usable_from, usable_until)
        && until <= from
    {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "사용 종료는 사용 시작보다 늦어야 합니다.",
        ));
    }

    if usable_until.is_none() && relative_validity_days.is_none() {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "사용 종료 시각 또는 발급 후 유효기간 중 하나는 반드시 설정해야 합니다.",
        ));
    }

    Ok(())
}

fn validate_weekdays(weekdays: &[i16]) -> ApiResult<()> {
    if weekdays.iter().any(|day| !(0..=6).contains(day)) {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "사용 가능 요일은 0(일요일)~6(토요일) 사이여야 합니다.",
        ));
    }
    Ok(())
}

fn map_issuance_conflict(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            ApiError::new(ErrorCode::Conflict).internal(format!("issuance conflict: {db}"))
        }
        _ => ApiError::from(error),
    }
}

// ---------------------------------------------------------------------------
// Row mapping
// ---------------------------------------------------------------------------

/// Read campaigns for one store, optionally narrowed to one id.
///
/// The two shapes — "the list" and "one of them" — share a query so the forty-odd columns
/// are written once rather than drifting apart. `FOR UPDATE` cannot be folded in the same
/// way, so [`CampaignService::lock`] carries the only other copy.
async fn load<'e, E>(
    executor: E,
    store_id: Uuid,
    campaign_id: Option<Uuid>,
) -> ApiResult<Vec<Campaign>>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let rows = sqlx::query_as!(
        CampaignRow,
        r#"
        SELECT
            id, store_id, status::text AS "status!", version_no, name, customer_description,
            benefit_type::text AS "benefit_type!", fixed_amount, percentage,
            maximum_discount_amount, free_item_ids, minimum_order_amount, eligible_item_ids,
            eligible_category_ids, excluded_item_ids, issue_mode::text AS "issue_mode!",
            audience_type, audience_criteria, audience_size, audience_snapshot_at,
            total_quantity, unlimited_total_cap, per_user_quantity,
            per_business_day_quantity, restore_quantity_on_revoke, revoke_policy,
            global_issued_count, global_reserved_count, global_revoked_count,
            issue_starts_at, issue_ends_at, usable_from, usable_until,
            relative_validity_days, allowed_weekdays, allowed_local_time_ranges,
            external_discount_stackable, notification_channels, notification_message,
            published_at, paused_at, cancelled_at, cancellation_reason,
            emergency_stopped_at, issue_generation, created_at, updated_at, version
        FROM coupon.campaigns
        WHERE store_id = $1 AND ($2::uuid IS NULL OR id = $2)
        ORDER BY created_at DESC
        "#,
        store_id,
        campaign_id,
    )
    .fetch_all(executor)
    .await?;

    Ok(rows.into_iter().map(Campaign::from).collect())
}

struct CampaignRow {
    id: Uuid,
    store_id: Uuid,
    status: String,
    version_no: i32,
    name: String,
    customer_description: String,
    benefit_type: String,
    fixed_amount: Option<i64>,
    percentage: Option<i16>,
    maximum_discount_amount: Option<i64>,
    free_item_ids: Vec<Uuid>,
    minimum_order_amount: i64,
    eligible_item_ids: Vec<Uuid>,
    eligible_category_ids: Vec<Uuid>,
    excluded_item_ids: Vec<Uuid>,
    issue_mode: String,
    audience_type: String,
    audience_criteria: serde_json::Value,
    audience_size: Option<i32>,
    audience_snapshot_at: Option<DateTime<Utc>>,
    total_quantity: Option<i64>,
    unlimited_total_cap: Option<i64>,
    per_user_quantity: i16,
    per_business_day_quantity: Option<i64>,
    restore_quantity_on_revoke: bool,
    revoke_policy: String,
    global_issued_count: i64,
    global_reserved_count: i64,
    global_revoked_count: i64,
    issue_starts_at: DateTime<Utc>,
    issue_ends_at: DateTime<Utc>,
    usable_from: Option<DateTime<Utc>>,
    usable_until: Option<DateTime<Utc>>,
    relative_validity_days: Option<i16>,
    allowed_weekdays: Vec<i16>,
    allowed_local_time_ranges: serde_json::Value,
    external_discount_stackable: bool,
    notification_channels: serde_json::Value,
    notification_message: Option<String>,
    published_at: Option<DateTime<Utc>>,
    paused_at: Option<DateTime<Utc>>,
    cancelled_at: Option<DateTime<Utc>>,
    cancellation_reason: Option<String>,
    emergency_stopped_at: Option<DateTime<Utc>>,
    issue_generation: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    version: i64,
}

impl From<CampaignRow> for Campaign {
    fn from(row: CampaignRow) -> Self {
        let total_quantity = TotalQuantity::from_columns(row.total_quantity, row.unlimited_total_cap);
        let remaining_quantity = (total_quantity.effective_cap()
            - row.global_issued_count
            - row.global_reserved_count)
            .max(0);

        Campaign {
            id: row.id,
            store_id: row.store_id,
            status: CampaignStatus::from_db(&row.status),
            version_no: row.version_no,
            name: row.name,
            customer_description: row.customer_description,
            benefit: Benefit {
                benefit_type: BenefitType::from_db(&row.benefit_type),
                fixed_amount: row.fixed_amount,
                percentage: row.percentage,
                maximum_discount_amount: row.maximum_discount_amount,
                free_item_ids: row.free_item_ids,
            },
            minimum_order_amount: row.minimum_order_amount,
            eligible_item_ids: row.eligible_item_ids,
            eligible_category_ids: row.eligible_category_ids,
            excluded_item_ids: row.excluded_item_ids,
            issue_mode: IssueMode::from_db(&row.issue_mode),
            audience_type: AudienceType::from_db(&row.audience_type),
            audience_criteria: serde_json::from_value(row.audience_criteria).unwrap_or_default(),
            audience_size: row.audience_size,
            audience_snapshot_at: row.audience_snapshot_at,
            total_quantity,
            per_user_quantity: row.per_user_quantity,
            per_business_day_quantity: row.per_business_day_quantity,
            restore_quantity_on_revoke: row.restore_quantity_on_revoke,
            revoke_policy: RevokePolicy::from_db(&row.revoke_policy),
            issued_count: row.global_issued_count,
            reserved_count: row.global_reserved_count,
            revoked_count: row.global_revoked_count,
            remaining_quantity,
            issue_starts_at: row.issue_starts_at,
            issue_ends_at: row.issue_ends_at,
            usable_from: row.usable_from,
            usable_until: row.usable_until,
            relative_validity_days: row.relative_validity_days,
            allowed_weekdays: row.allowed_weekdays,
            allowed_local_time_ranges: serde_json::from_value(row.allowed_local_time_ranges)
                .unwrap_or_default(),
            external_discount_stackable: row.external_discount_stackable,
            notification_channels: serde_json::from_value(row.notification_channels)
                .unwrap_or_default(),
            notification_message: row.notification_message,
            published_at: row.published_at,
            paused_at: row.paused_at,
            cancelled_at: row.cancelled_at,
            cancellation_reason: row.cancellation_reason,
            emergency_stopped_at: row.emergency_stopped_at,
            issue_generation: row.issue_generation,
            created_at: row.created_at,
            updated_at: row.updated_at,
            version: row.version,
        }
    }
}

/// A campaign fixture the tests in this module and in [`audience`] both build on.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("timestamp")
    }

    pub fn campaign() -> Campaign {
        Campaign {
            id: Uuid::from_u128(1),
            store_id: Uuid::from_u128(2),
            status: CampaignStatus::Draft,
            version_no: 1,
            name: "여름 할인".to_owned(),
            customer_description: "시원한 한 잔".to_owned(),
            benefit: Benefit {
                benefit_type: BenefitType::FixedAmount,
                fixed_amount: Some(2_000),
                percentage: None,
                maximum_discount_amount: None,
                free_item_ids: Vec::new(),
            },
            minimum_order_amount: 0,
            eligible_item_ids: Vec::new(),
            eligible_category_ids: Vec::new(),
            excluded_item_ids: Vec::new(),
            issue_mode: IssueMode::FirstCome,
            audience_type: AudienceType::AllCustomers,
            audience_criteria: AudienceCriteria::default(),
            audience_size: None,
            audience_snapshot_at: None,
            total_quantity: TotalQuantity::Limited { quantity: 100 },
            per_user_quantity: 1,
            per_business_day_quantity: None,
            restore_quantity_on_revoke: false,
            revoke_policy: RevokePolicy::KeepIssued,
            issued_count: 0,
            reserved_count: 0,
            revoked_count: 0,
            remaining_quantity: 100,
            issue_starts_at: at("2026-08-01T00:00:00Z"),
            issue_ends_at: at("2026-08-31T00:00:00Z"),
            usable_from: None,
            usable_until: Some(at("2026-09-30T00:00:00Z")),
            relative_validity_days: None,
            allowed_weekdays: Vec::new(),
            allowed_local_time_ranges: Vec::new(),
            external_discount_stackable: false,
            notification_channels: Vec::new(),
            notification_message: None,
            published_at: None,
            paused_at: None,
            cancelled_at: None,
            cancellation_reason: None,
            emergency_stopped_at: None,
            issue_generation: 1,
            created_at: at("2026-07-01T00:00:00Z"),
            updated_at: at("2026-07-01T00:00:00Z"),
            version: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{at, campaign};
    use super::*;

    #[test]
    fn unlimited_is_a_bounded_expression_not_an_integer_maximum() {
        // §8.4: 총수량 무제한은 운영 상한을 가진 별도 표현이며 DB 정수 최대값을 쓰지 않는다.
        let unlimited = TotalQuantity::Unlimited {
            operational_cap: 50_000,
        };
        assert!(unlimited.is_unlimited());
        assert_eq!(unlimited.effective_cap(), 50_000);
        assert!(unlimited.effective_cap() < i64::MAX);
        assert!(unlimited.effective_cap() < i64::from(i32::MAX));

        let limited = TotalQuantity::Limited { quantity: 100 };
        assert!(!limited.is_unlimited());
        assert_eq!(limited.effective_cap(), 100);
    }

    #[test]
    fn the_two_quantity_modes_are_distinguishable_on_the_wire() {
        let json = serde_json::to_value(TotalQuantity::Unlimited {
            operational_cap: 1_000,
        })
        .expect("serialises");
        assert_eq!(json["mode"], "UNLIMITED");
        assert_eq!(json["operational_cap"], 1_000);

        let json = serde_json::to_value(TotalQuantity::Limited { quantity: 30 })
            .expect("serialises");
        assert_eq!(json["mode"], "LIMITED");
        assert_eq!(json["quantity"], 30);
    }

    #[test]
    fn a_quantity_outside_the_operational_range_is_refused() {
        // CAMPAIGN-002: 총수량은 1~1,000,000 또는 UNLIMITED.
        assert!(TotalQuantity::Limited { quantity: 0 }.validate().is_err());
        assert!(
            TotalQuantity::Limited {
                quantity: MAX_TOTAL_QUANTITY + 1
            }
            .validate()
            .is_err()
        );
        assert!(
            TotalQuantity::Unlimited {
                operational_cap: MAX_TOTAL_QUANTITY
            }
            .validate()
            .is_ok()
        );
        assert!(
            TotalQuantity::Unlimited {
                operational_cap: i64::MAX
            }
            .validate()
            .is_err(),
            "an unlimited campaign still may not claim the integer maximum"
        );
    }

    #[test]
    fn the_columns_and_the_wire_representation_agree() {
        for quantity in [
            TotalQuantity::Limited { quantity: 42 },
            TotalQuantity::Unlimited {
                operational_cap: 42,
            },
        ] {
            let (total, cap) = quantity.columns();
            assert_eq!(TotalQuantity::from_columns(total, cap), quantity);
        }
    }

    #[test]
    fn an_absolute_end_and_a_relative_one_resolve_to_the_earlier() {
        // §8.5: 절대 사용 종료와 `발급 후 N일`을 함께 설정하면 더 이른 시각을 적용한다.
        let mut campaign = campaign();
        campaign.usable_until = Some(at("2026-08-10T00:00:00Z"));
        campaign.relative_validity_days = Some(30);

        let issued_at = at("2026-08-01T03:00:00Z");
        assert_eq!(
            expiry_for(&campaign, issued_at, "Asia/Seoul").expect("resolves"),
            at("2026-08-10T00:00:00Z"),
            "the absolute end is sooner, so it wins"
        );

        campaign.relative_validity_days = Some(3);
        assert_eq!(
            expiry_for(&campaign, issued_at, "Asia/Seoul").expect("resolves"),
            at("2026-08-04T03:00:00Z"),
            "now the relative one is sooner"
        );
    }

    #[test]
    fn a_relative_validity_counts_local_calendar_days() {
        // CAMPAIGN-005: MVP 기본은 `issued_at + N calendar days`의 같은 현지 시각이며
        // 최종 값은 UTC로 고정한다.
        let mut campaign = campaign();
        campaign.usable_until = None;
        campaign.relative_validity_days = Some(7);

        // 2026-08-01T15:30Z is 2026-08-02 00:30 in Seoul; seven local days later is
        // 2026-08-09 00:30 Seoul, i.e. 2026-08-08T15:30Z.
        let expires = expiry_for(&campaign, at("2026-08-01T15:30:00Z"), "Asia/Seoul")
            .expect("resolves");
        assert_eq!(expires, at("2026-08-08T15:30:00Z"));
    }

    #[test]
    fn a_campaign_with_no_end_at_all_is_refused_rather_than_defaulted() {
        let mut campaign = campaign();
        campaign.usable_until = None;
        campaign.relative_validity_days = None;

        assert_eq!(
            expiry_for(&campaign, at("2026-08-01T00:00:00Z"), "Asia/Seoul")
                .expect_err("no end")
                .code,
            ErrorCode::UnprocessableRequest
        );
    }

    #[test]
    fn the_schedule_rules_are_the_ones_campaign_002_lists() {
        let start = at("2026-08-01T00:00:00Z");
        let end = at("2026-08-31T00:00:00Z");

        // 종료가 시작보다 늦어야 한다.
        assert!(validate_schedule(end, start, None, Some(end), None).is_err());
        assert!(validate_schedule(start, start, None, Some(end), None).is_err());

        // 사용 종료는 발급 시작보다 늦어야 한다.
        assert!(
            validate_schedule(start, end, None, Some(at("2026-07-01T00:00:00Z")), None).is_err()
        );

        // 사용 종료 또는 상대 유효기간 중 하나는 필요하다.
        assert!(validate_schedule(start, end, None, None, None).is_err());
        assert!(validate_schedule(start, end, None, None, Some(7)).is_ok());
        assert!(validate_schedule(start, end, None, Some(end), None).is_ok());
    }

    #[test]
    fn only_real_weekdays_are_accepted() {
        assert!(validate_weekdays(&[0, 6]).is_ok());
        assert!(validate_weekdays(&[]).is_ok());
        assert!(validate_weekdays(&[7]).is_err());
        assert!(validate_weekdays(&[-1]).is_err());
    }

    #[test]
    fn an_edit_that_would_be_retroactive_is_recognised_as_one() {
        // §8.5 / CAMPAIGN-008. The left column must be refused after publication; the
        // right column is forward-looking and stays editable.
        let retroactive = [
            UpdateCampaignRequest {
                benefit: Some(Benefit {
                    benefit_type: BenefitType::FixedAmount,
                    fixed_amount: Some(1),
                    percentage: None,
                    maximum_discount_amount: None,
                    free_item_ids: Vec::new(),
                }),
                ..Default::default()
            },
            UpdateCampaignRequest {
                minimum_order_amount: Some(1_000),
                ..Default::default()
            },
            UpdateCampaignRequest {
                eligible_item_ids: Some(vec![Uuid::nil()]),
                ..Default::default()
            },
            UpdateCampaignRequest {
                usable_until: Some(at("2026-09-01T00:00:00Z")),
                ..Default::default()
            },
            UpdateCampaignRequest {
                relative_validity_days: Some(3),
                ..Default::default()
            },
        ];
        for request in retroactive {
            assert!(
                request.touches_frozen_fields(),
                "a published campaign must refuse this edit"
            );
        }

        let forward_looking = [
            UpdateCampaignRequest {
                total_quantity: Some(TotalQuantity::Limited { quantity: 500 }),
                ..Default::default()
            },
            UpdateCampaignRequest {
                per_user_quantity: Some(2),
                ..Default::default()
            },
            UpdateCampaignRequest {
                per_business_day_quantity: Some(10),
                ..Default::default()
            },
            UpdateCampaignRequest {
                issue_ends_at: Some(at("2026-09-01T00:00:00Z")),
                ..Default::default()
            },
            UpdateCampaignRequest {
                name: Some("새 이름".to_owned()),
                ..Default::default()
            },
        ];
        for request in forward_looking {
            assert!(
                !request.touches_frozen_fields(),
                "CAMPAIGN-008 allows this after publication"
            );
        }
    }

    #[test]
    fn campaign_statuses_round_trip_and_unknown_values_fail_closed() {
        for status in [
            CampaignStatus::Draft,
            CampaignStatus::Scheduled,
            CampaignStatus::Issuing,
            CampaignStatus::Paused,
            CampaignStatus::Ended,
            CampaignStatus::Cancelled,
        ] {
            assert_eq!(CampaignStatus::from_db(status.as_db()), status);
        }
        assert_eq!(
            CampaignStatus::from_db("SOMETHING_NEW"),
            CampaignStatus::Cancelled,
            "an unrecognised status must not be one that issues coupons"
        );
    }

    #[test]
    fn revocation_defaults_to_keeping_what_customers_already_have() {
        // CAMPAIGN-007 makes 전부 회수 the deliberate choice, never the fallback.
        assert_eq!(RevokePolicy::from_db("nonsense"), RevokePolicy::KeepIssued);
        assert_eq!(default_revoke_policy(), RevokePolicy::KeepIssued);
    }

    #[test]
    fn the_publish_estimate_bounds_direct_issuance_by_the_audience() {
        let mut campaign = campaign();
        campaign.issue_mode = IssueMode::Direct;
        campaign.total_quantity = TotalQuantity::Limited { quantity: 1_000 };
        campaign.per_user_quantity = 1;

        let estimate = build_estimate(&campaign, 120);
        assert_eq!(estimate.audience_size, 120);
        assert_eq!(estimate.maximum_issued_quantity, 120, "cap ∧ audience");
        assert_eq!(estimate.maximum_discount_cost, Some(2_000 * 120));
    }

    #[test]
    fn a_first_come_estimate_is_bounded_by_the_quantity_alone() {
        let mut campaign = campaign();
        campaign.issue_mode = IssueMode::FirstCome;
        campaign.total_quantity = TotalQuantity::Limited { quantity: 50 };

        let estimate = build_estimate(&campaign, 10_000);
        assert_eq!(estimate.maximum_issued_quantity, 50);
    }

    #[test]
    fn a_free_item_campaign_does_not_invent_a_cost() {
        let mut campaign = campaign();
        campaign.benefit = Benefit {
            benefit_type: BenefitType::FreeItem,
            fixed_amount: None,
            percentage: None,
            maximum_discount_amount: None,
            free_item_ids: vec![Uuid::nil()],
        };

        assert_eq!(build_estimate(&campaign, 100).maximum_discount_cost, None);
    }

    #[test]
    fn the_conditions_snapshot_carries_everything_redemption_will_need() {
        let mut campaign = campaign();
        campaign.minimum_order_amount = 10_000;
        campaign.allowed_weekdays = vec![1, 2];
        campaign.eligible_item_ids = vec![Uuid::from_u128(9)];

        let conditions = conditions_of(&campaign, "Asia/Seoul");
        assert_eq!(conditions.minimum_order_amount, 10_000);
        assert_eq!(conditions.allowed_weekdays, vec![1, 2]);
        assert_eq!(conditions.eligible_item_ids, vec![Uuid::from_u128(9)]);
        assert_eq!(conditions.timezone, "Asia/Seoul");
        assert_eq!(conditions.benefit, campaign.benefit);
    }

    #[test]
    fn remaining_quantity_is_hidden_for_an_unlimited_campaign() {
        let mut campaign = campaign();
        campaign.total_quantity = TotalQuantity::Unlimited {
            operational_cap: 1_000,
        };
        assert_eq!(remaining(&campaign), None);

        campaign.total_quantity = TotalQuantity::Limited { quantity: 10 };
        campaign.issued_count = 4;
        campaign.reserved_count = 1;
        assert_eq!(remaining(&campaign), Some(5));

        // Never negative, even if the counters somehow overshoot.
        campaign.issued_count = 20;
        assert_eq!(remaining(&campaign), Some(0));
    }
}
