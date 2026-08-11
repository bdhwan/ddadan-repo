//! 품목·카테고리 (§10.2 `catalog`, §8.3, §11.4).
//!
//! Owns `coupon.catalog_categories` and `coupon.catalog_items`, and — more importantly —
//! owns the *judgement* of whether an order satisfies an item restriction. `loyalty` and
//! later `redemptions` ask this module rather than reading the tables themselves (§10.2).
//!
//! Two rules from §8.3 drive the evaluator:
//!
//! * **Exclusion beats inclusion.** An item that is both a target and an exclusion is
//!   excluded. Anything else would let a careless configuration give away more than the
//!   owner intended.
//! * **An unlisted order cannot satisfy an item restriction.** If the policy names items
//!   and the owner typed no line items, there is nothing to check against, so it fails —
//!   it does not pass by default.

pub mod routes;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::db::changed_one_row;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::concurrency;

pub use routes::owner_catalog_router;

/// `coupon.resource_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResourceStatus {
    Active,
    Inactive,
    Archived,
}

impl ResourceStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            ResourceStatus::Active => "ACTIVE",
            ResourceStatus::Inactive => "INACTIVE",
            ResourceStatus::Archived => "ARCHIVED",
        }
    }

    /// An unrecognised status is treated as archived: unknown means "do not offer it".
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "ACTIVE" => ResourceStatus::Active,
            "INACTIVE" => ResourceStatus::Inactive,
            _ => ResourceStatus::Archived,
        }
    }

    /// Whether a *new* policy may name this item (§8.3). Existing snapshots keep it
    /// either way.
    pub fn is_selectable(self) -> bool {
        self == ResourceStatus::Active
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogCategory {
    pub id: Uuid,
    pub name: String,
    pub status: ResourceStatus,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogItem {
    pub id: Uuid,
    pub category_id: Option<Uuid>,
    pub category_name: Option<String>,
    /// The store's own SKU. Optional — most small shops do not have one (§8.3).
    pub sku: Option<String>,
    pub name: String,
    /// Reference price only. The price that counts is the one entered at the till (§8.3).
    pub reference_price: Option<i64>,
    pub status: ResourceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreateCategoryRequest {
    #[validate(length(min = 1, max = 120, message = "카테고리 이름은 1~120자여야 합니다."))]
    pub name: String,
    pub sort_order: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateCategoryRequest {
    #[validate(length(min = 1, max = 120, message = "카테고리 이름은 1~120자여야 합니다."))]
    pub name: Option<String>,
    pub sort_order: Option<i32>,
    pub status: Option<ResourceStatus>,
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreateItemRequest {
    #[validate(length(min = 1, max = 160, message = "품목 이름은 1~160자여야 합니다."))]
    pub name: String,
    pub category_id: Option<Uuid>,
    #[validate(length(min = 1, max = 100))]
    pub sku: Option<String>,
    #[validate(range(min = 0, max = 100_000_000, message = "참고 가격을 확인해 주세요."))]
    pub reference_price: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateItemRequest {
    #[validate(length(min = 1, max = 160, message = "품목 이름은 1~160자여야 합니다."))]
    pub name: Option<String>,
    pub category_id: Option<Uuid>,
    #[validate(length(min = 1, max = 100))]
    pub sku: Option<String>,
    #[validate(range(min = 0, max = 100_000_000, message = "참고 가격을 확인해 주세요."))]
    pub reference_price: Option<i64>,
    /// Deactivating keeps the item in every snapshot that already names it (§8.3).
    pub status: Option<ResourceStatus>,
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogCategoriesResponse {
    pub categories: Vec<CatalogCategory>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CatalogItemsResponse {
    pub items: Vec<CatalogItem>,
}

/// One line the owner typed at the till, after catalogue lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLine {
    pub catalog_item_id: Option<Uuid>,
    pub category_id: Option<Uuid>,
    pub name_snapshot: String,
    pub quantity: i64,
    pub unit_price: i64,
}

impl OrderLine {
    pub fn line_amount(&self) -> i64 {
        self.quantity.saturating_mul(self.unit_price)
    }
}

/// A policy's or campaign's item conditions, in the form the evaluator needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemRestriction {
    pub eligible_item_ids: Vec<Uuid>,
    pub eligible_category_ids: Vec<Uuid>,
    pub excluded_item_ids: Vec<Uuid>,
}

impl ItemRestriction {
    /// Whether the restriction says anything at all. An empty restriction means "the
    /// whole menu" (§8.3: 대상 품목 기본값 전체).
    pub fn restricts_items(&self) -> bool {
        !self.eligible_item_ids.is_empty()
            || !self.eligible_category_ids.is_empty()
            || !self.excluded_item_ids.is_empty()
    }

    /// Whether one line counts towards the benefit.
    pub fn accepts(&self, line: &OrderLine) -> bool {
        // §8.3: 대상과 제외가 겹치면 제외가 우선한다.
        if let Some(item_id) = line.catalog_item_id
            && self.excluded_item_ids.contains(&item_id)
        {
            return false;
        }

        if self.eligible_item_ids.is_empty() && self.eligible_category_ids.is_empty() {
            // Only exclusions were configured, so everything not excluded qualifies.
            return true;
        }

        let by_item = line
            .catalog_item_id
            .is_some_and(|id| self.eligible_item_ids.contains(&id));
        let by_category = line
            .category_id
            .is_some_and(|id| self.eligible_category_ids.contains(&id));

        by_item || by_category
    }

    /// Judge a whole order.
    pub fn evaluate(&self, lines: &[OrderLine], gross_amount: i64) -> Eligibility {
        if !self.restricts_items() {
            // Nothing is restricted, so the whole order is the eligible amount — even
            // when the owner typed no line items at all.
            return Eligibility {
                eligible_amount: gross_amount,
                eligible_line_count: lines.len(),
                excluded_line_count: 0,
                requires_line_items: false,
            };
        }

        let (eligible, excluded): (Vec<&OrderLine>, Vec<&OrderLine>) =
            lines.iter().partition(|line| self.accepts(line));

        Eligibility {
            eligible_amount: eligible.iter().map(|line| line.line_amount()).sum(),
            eligible_line_count: eligible.len(),
            excluded_line_count: excluded.len(),
            requires_line_items: true,
        }
    }
}

/// The verdict on one order against one restriction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eligibility {
    /// Amount that counts towards a minimum, and later towards a discount base (§5.3).
    pub eligible_amount: i64,
    pub eligible_line_count: usize,
    pub excluded_line_count: usize,
    /// Whether the restriction needed line items to be judged at all.
    pub requires_line_items: bool,
}

impl Eligibility {
    pub fn is_satisfied(&self) -> bool {
        !self.requires_line_items || self.eligible_line_count > 0
    }

    /// The message STAMP-005 shows when an order has nothing that qualifies.
    pub fn rejection(&self) -> Option<ApiError> {
        if self.is_satisfied() {
            return None;
        }

        Some(if self.excluded_line_count > 0 {
            ApiError::with_message(
                ErrorCode::ItemNotEligible,
                "주문에 적립 제외 품목만 포함되어 있습니다.",
            )
        } else {
            ApiError::with_message(
                ErrorCode::ItemNotEligible,
                "적립 대상 품목을 주문에 입력해 주세요.",
            )
        })
    }
}

pub struct CatalogService;

impl Default for CatalogService {
    fn default() -> Self {
        Self::new()
    }
}

impl CatalogService {
    pub fn new() -> Self {
        Self
    }

    pub async fn list_categories<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
    ) -> ApiResult<Vec<CatalogCategory>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT id, name, status::text AS "status!", sort_order, created_at, updated_at, version
            FROM coupon.catalog_categories
            WHERE store_id = $1 AND status <> 'ARCHIVED'
            ORDER BY sort_order, name
            "#,
            store_id,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| CatalogCategory {
                id: row.id,
                name: row.name,
                status: ResourceStatus::from_db(&row.status),
                sort_order: row.sort_order,
                created_at: row.created_at,
                updated_at: row.updated_at,
                version: row.version,
            })
            .collect())
    }

    pub async fn create_category<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
        request: &CreateCategoryRequest,
    ) -> ApiResult<CatalogCategory>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            INSERT INTO coupon.catalog_categories (store_id, name, sort_order)
            VALUES ($1, $2, COALESCE($3, 0))
            RETURNING id, name, status::text AS "status!", sort_order, created_at, updated_at, version
            "#,
            store_id,
            request.name.trim(),
            request.sort_order,
        )
        .fetch_one(executor)
        .await
        .map_err(|error| map_unique_violation(error, "이미 같은 이름의 카테고리가 있습니다."))?;

        Ok(CatalogCategory {
            id: row.id,
            name: row.name,
            status: ResourceStatus::from_db(&row.status),
            sort_order: row.sort_order,
            created_at: row.created_at,
            updated_at: row.updated_at,
            version: row.version,
        })
    }

    pub async fn update_category(
        &self,
        pool: &sqlx::PgPool,
        store_id: Uuid,
        category_id: Uuid,
        request: &UpdateCategoryRequest,
        expected_version: Option<i64>,
    ) -> ApiResult<CatalogCategory> {
        let result = sqlx::query!(
            r#"
            UPDATE coupon.catalog_categories
            SET name = COALESCE($3, name),
                sort_order = COALESCE($4, sort_order),
                status = COALESCE($5::text::coupon.resource_status, status)
            WHERE id = $1 AND store_id = $2
              AND ($6::bigint IS NULL OR version = $6)
            "#,
            category_id,
            store_id,
            request.name.as_deref().map(str::trim),
            request.sort_order,
            request.status.map(ResourceStatus::as_db),
            expected_version,
        )
        .execute(pool)
        .await
        .map_err(|error| map_unique_violation(error, "이미 같은 이름의 카테고리가 있습니다."))?;

        if !changed_one_row(&result) {
            let exists = self.category_exists(pool, store_id, category_id).await?;
            concurrency::ensure_updated(result.rows_affected(), exists)?;
        }

        self.list_categories(pool, store_id)
            .await?
            .into_iter()
            .find(|category| category.id == category_id)
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound))
    }

    async fn category_exists(
        &self,
        pool: &sqlx::PgPool,
        store_id: Uuid,
        category_id: Uuid,
    ) -> ApiResult<bool> {
        Ok(sqlx::query_scalar!(
            "SELECT 1 FROM coupon.catalog_categories WHERE id = $1 AND store_id = $2",
            category_id,
            store_id,
        )
        .fetch_optional(pool)
        .await?
        .is_some())
    }

    pub async fn list_items<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
        include_inactive: bool,
    ) -> ApiResult<Vec<CatalogItem>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query!(
            r#"
            SELECT
                i.id,
                i.category_id,
                c.name AS "category_name?",
                i.sku,
                i.name,
                i.reference_price,
                i.status::text AS "status!",
                i.created_at,
                i.updated_at,
                i.version
            FROM coupon.catalog_items i
            LEFT JOIN coupon.catalog_categories c ON c.id = i.category_id
            WHERE i.store_id = $1
              AND (i.status = 'ACTIVE' OR $2)
              AND i.status <> 'ARCHIVED'
            ORDER BY i.name
            "#,
            store_id,
            include_inactive,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| CatalogItem {
                id: row.id,
                category_id: row.category_id,
                category_name: row.category_name,
                sku: row.sku,
                name: row.name,
                reference_price: row.reference_price,
                status: ResourceStatus::from_db(&row.status),
                created_at: row.created_at,
                updated_at: row.updated_at,
                version: row.version,
            })
            .collect())
    }

    pub async fn create_item(
        &self,
        pool: &sqlx::PgPool,
        store_id: Uuid,
        request: &CreateItemRequest,
    ) -> ApiResult<CatalogItem> {
        if let Some(category_id) = request.category_id
            && !self.category_exists(pool, store_id, category_id).await?
        {
            return Err(ApiError::with_message(
                ErrorCode::NotFound,
                "카테고리를 찾을 수 없습니다.",
            ));
        }

        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.catalog_items (store_id, category_id, sku, name, reference_price)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
            store_id,
            request.category_id,
            request.sku.as_deref().map(str::trim),
            request.name.trim(),
            request.reference_price,
        )
        .fetch_one(pool)
        .await
        .map_err(|error| map_unique_violation(error, "이미 사용 중인 SKU 입니다."))?;

        self.find_item(pool, store_id, id).await
    }

    pub async fn update_item(
        &self,
        pool: &sqlx::PgPool,
        store_id: Uuid,
        item_id: Uuid,
        request: &UpdateItemRequest,
        expected_version: Option<i64>,
    ) -> ApiResult<CatalogItem> {
        if let Some(category_id) = request.category_id
            && !self.category_exists(pool, store_id, category_id).await?
        {
            return Err(ApiError::with_message(
                ErrorCode::NotFound,
                "카테고리를 찾을 수 없습니다.",
            ));
        }

        let result = sqlx::query!(
            r#"
            UPDATE coupon.catalog_items
            SET name = COALESCE($3, name),
                category_id = COALESCE($4, category_id),
                sku = COALESCE($5, sku),
                reference_price = COALESCE($6, reference_price),
                status = COALESCE($7::text::coupon.resource_status, status)
            WHERE id = $1 AND store_id = $2
              AND ($8::bigint IS NULL OR version = $8)
            "#,
            item_id,
            store_id,
            request.name.as_deref().map(str::trim),
            request.category_id,
            request.sku.as_deref().map(str::trim),
            request.reference_price,
            request.status.map(ResourceStatus::as_db),
            expected_version,
        )
        .execute(pool)
        .await
        .map_err(|error| map_unique_violation(error, "이미 사용 중인 SKU 입니다."))?;

        if !changed_one_row(&result) {
            let exists = sqlx::query_scalar!(
                "SELECT 1 FROM coupon.catalog_items WHERE id = $1 AND store_id = $2",
                item_id,
                store_id,
            )
            .fetch_optional(pool)
            .await?
            .is_some();
            concurrency::ensure_updated(result.rows_affected(), exists)?;
        }

        self.find_item(pool, store_id, item_id).await
    }

    async fn find_item(
        &self,
        pool: &sqlx::PgPool,
        store_id: Uuid,
        item_id: Uuid,
    ) -> ApiResult<CatalogItem> {
        self.list_items(pool, store_id, true)
            .await?
            .into_iter()
            .find(|item| item.id == item_id)
            .ok_or_else(|| ApiError::new(ErrorCode::CatalogItemNotFound))
    }

    /// Check that every id a new policy names is this store's and still selectable (§8.3).
    pub async fn ensure_selectable<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
        item_ids: &[Uuid],
    ) -> ApiResult<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if item_ids.is_empty() {
            return Ok(());
        }

        let selectable: Vec<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.catalog_items
            WHERE store_id = $1 AND id = ANY($2) AND status = 'ACTIVE'
            "#,
            store_id,
            item_ids,
        )
        .fetch_all(executor)
        .await?;

        if let Some(missing) = item_ids.iter().find(|id| !selectable.contains(id)) {
            return Err(ApiError::with_message(
                ErrorCode::CatalogItemNotFound,
                "비활성이거나 존재하지 않는 품목은 새 정책에 넣을 수 없습니다.",
            )
            .internal(format!("catalog item {missing} is not selectable")));
        }

        Ok(())
    }

    /// Same, for categories.
    pub async fn ensure_categories_selectable<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
        category_ids: &[Uuid],
    ) -> ApiResult<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if category_ids.is_empty() {
            return Ok(());
        }

        let selectable: Vec<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.catalog_categories
            WHERE store_id = $1 AND id = ANY($2) AND status = 'ACTIVE'
            "#,
            store_id,
            category_ids,
        )
        .fetch_all(executor)
        .await?;

        if let Some(missing) = category_ids.iter().find(|id| !selectable.contains(id)) {
            return Err(ApiError::with_message(
                ErrorCode::CatalogItemNotFound,
                "비활성이거나 존재하지 않는 카테고리는 새 정책에 넣을 수 없습니다.",
            )
            .internal(format!("catalog category {missing} is not selectable")));
        }

        Ok(())
    }

    /// Turn what the owner typed into evaluable lines.
    ///
    /// A line may name a catalogue item or be free text. A named item must belong to this
    /// store — otherwise an owner could reference another store's catalogue to satisfy a
    /// restriction (SEC-001). Deactivated items are still accepted here: the customer did
    /// buy one, and the restriction snapshot may well still name it (§8.3).
    pub async fn resolve_order_lines<'e, E>(
        &self,
        executor: E,
        store_id: Uuid,
        inputs: &[OrderItemInput],
    ) -> ApiResult<Vec<OrderLine>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let referenced: Vec<Uuid> = inputs.iter().filter_map(|input| input.catalog_item_id).collect();

        let known = if referenced.is_empty() {
            Vec::new()
        } else {
            sqlx::query!(
                r#"
                SELECT id, category_id, name
                FROM coupon.catalog_items
                WHERE store_id = $1 AND id = ANY($2)
                "#,
                store_id,
                &referenced,
            )
            .fetch_all(executor)
            .await?
        };

        inputs
            .iter()
            .map(|input| match input.catalog_item_id {
                Some(item_id) => {
                    let row = known.iter().find(|row| row.id == item_id).ok_or_else(|| {
                        ApiError::with_message(
                            ErrorCode::CatalogItemNotFound,
                            "이 상점의 품목이 아닙니다.",
                        )
                        .internal(format!("catalog item {item_id} is not in store {store_id}"))
                    })?;

                    Ok(OrderLine {
                        catalog_item_id: Some(item_id),
                        category_id: row.category_id,
                        name_snapshot: input
                            .name_snapshot
                            .clone()
                            .unwrap_or_else(|| row.name.clone()),
                        quantity: input.quantity,
                        unit_price: input.unit_price,
                    })
                }
                None => Ok(OrderLine {
                    catalog_item_id: None,
                    category_id: None,
                    name_snapshot: input
                        .name_snapshot
                        .clone()
                        .unwrap_or_else(|| "미등록 품목".to_owned()),
                    quantity: input.quantity,
                    unit_price: input.unit_price,
                }),
            })
            .collect()
    }
}

/// One `order.items[]` entry from a scan request (§11.4).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, Validate)]
pub struct OrderItemInput {
    /// Optional: an owner may type an item that is not in the catalogue.
    pub catalog_item_id: Option<Uuid>,
    #[validate(length(max = 160))]
    pub name_snapshot: Option<String>,
    #[validate(range(min = 1, max = 1000, message = "수량은 1~1000 이어야 합니다."))]
    pub quantity: i64,
    #[validate(range(min = 0, max = 100_000_000, message = "단가를 확인해 주세요."))]
    pub unit_price: i64,
}

fn map_unique_violation(error: sqlx::Error, message: &str) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            ApiError::with_message(ErrorCode::Conflict, message).internal(db.to_string())
        }
        _ => ApiError::from(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u128) -> Uuid {
        Uuid::from_u128(id)
    }

    fn line(item_id: Option<u128>, category_id: Option<u128>, quantity: i64, price: i64) -> OrderLine {
        OrderLine {
            catalog_item_id: item_id.map(item),
            category_id: category_id.map(item),
            name_snapshot: "품목".to_owned(),
            quantity,
            unit_price: price,
        }
    }

    #[test]
    fn an_empty_restriction_accepts_the_whole_order() {
        let restriction = ItemRestriction::default();
        assert!(!restriction.restricts_items());

        let verdict = restriction.evaluate(&[line(Some(1), None, 2, 6_000)], 12_000);
        assert_eq!(verdict.eligible_amount, 12_000);
        assert!(verdict.is_satisfied());
        assert!(verdict.rejection().is_none());
    }

    #[test]
    fn an_unrestricted_policy_does_not_need_line_items_at_all() {
        let verdict = ItemRestriction::default().evaluate(&[], 12_000);

        assert!(verdict.is_satisfied());
        assert_eq!(verdict.eligible_amount, 12_000);
    }

    #[test]
    fn a_restricted_policy_refuses_an_order_with_no_line_items() {
        // §8.3: 주문 품목을 입력하지 않은 경우 품목 제한 쿠폰을 승인할 수 없다.
        let restriction = ItemRestriction {
            eligible_item_ids: vec![item(1)],
            ..Default::default()
        };

        let verdict = restriction.evaluate(&[], 12_000);
        assert!(!verdict.is_satisfied());
        assert_eq!(
            verdict.rejection().expect("rejected").code,
            ErrorCode::ItemNotEligible
        );
    }

    #[test]
    fn only_the_eligible_lines_count_towards_the_amount() {
        let restriction = ItemRestriction {
            eligible_item_ids: vec![item(1)],
            ..Default::default()
        };

        let verdict = restriction.evaluate(
            &[line(Some(1), None, 2, 6_000), line(Some(2), None, 1, 9_000)],
            21_000,
        );

        assert_eq!(verdict.eligible_amount, 12_000);
        assert_eq!(verdict.eligible_line_count, 1);
        assert_eq!(verdict.excluded_line_count, 1);
    }

    #[test]
    fn a_category_target_accepts_every_item_in_it() {
        let restriction = ItemRestriction {
            eligible_category_ids: vec![item(10)],
            ..Default::default()
        };

        assert!(restriction.accepts(&line(Some(1), Some(10), 1, 5_000)));
        assert!(!restriction.accepts(&line(Some(2), Some(11), 1, 5_000)));
    }

    #[test]
    fn exclusion_wins_over_inclusion() {
        // §8.3, stated twice in the spec because it is the rule people get wrong.
        let restriction = ItemRestriction {
            eligible_item_ids: vec![item(1)],
            eligible_category_ids: vec![item(10)],
            excluded_item_ids: vec![item(1)],
        };

        assert!(
            !restriction.accepts(&line(Some(1), Some(10), 1, 5_000)),
            "an item that is both a target and an exclusion must be excluded"
        );
    }

    #[test]
    fn an_exclusion_only_restriction_accepts_everything_else() {
        let restriction = ItemRestriction {
            excluded_item_ids: vec![item(9)],
            ..Default::default()
        };

        assert!(restriction.restricts_items());
        assert!(restriction.accepts(&line(Some(1), None, 1, 5_000)));
        assert!(!restriction.accepts(&line(Some(9), None, 1, 5_000)));
    }

    #[test]
    fn an_order_of_only_excluded_items_says_so() {
        let restriction = ItemRestriction {
            excluded_item_ids: vec![item(9)],
            ..Default::default()
        };

        let verdict = restriction.evaluate(&[line(Some(9), None, 1, 5_000)], 5_000);
        let rejection = verdict.rejection().expect("rejected");
        assert_eq!(rejection.code, ErrorCode::ItemNotEligible);
        assert!(
            rejection.message.contains("제외"),
            "STAMP-005 distinguishes 'only excluded items' from 'no target items'"
        );
    }

    #[test]
    fn free_text_lines_cannot_satisfy_a_named_restriction() {
        let restriction = ItemRestriction {
            eligible_item_ids: vec![item(1)],
            ..Default::default()
        };

        assert!(!restriction.accepts(&line(None, None, 1, 5_000)));
    }

    #[test]
    fn line_amounts_do_not_overflow_on_absurd_input() {
        let line = OrderLine {
            quantity: i64::MAX,
            unit_price: i64::MAX,
            ..line(None, None, 1, 1)
        };
        assert_eq!(line.line_amount(), i64::MAX, "saturates rather than panics");
    }

    #[test]
    fn only_active_items_may_be_named_by_a_new_policy() {
        assert!(ResourceStatus::Active.is_selectable());
        assert!(!ResourceStatus::Inactive.is_selectable());
        assert!(!ResourceStatus::Archived.is_selectable());
        assert_eq!(ResourceStatus::from_db("SOMETHING"), ResourceStatus::Archived);
    }
}
