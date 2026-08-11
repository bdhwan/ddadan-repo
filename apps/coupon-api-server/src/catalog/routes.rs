//! `/owner/catalog/*` (§11.4).

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;
use validator::Validate;

use crate::auth::extractors::StoreOwner;
use crate::catalog::{
    CatalogCategoriesResponse, CatalogCategory, CatalogItem, CatalogItemsResponse,
    CreateCategoryRequest, CreateItemRequest, UpdateCategoryRequest, UpdateItemRequest,
};
use crate::error::ApiResult;
use crate::http::concurrency;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::state::AppState;

pub fn owner_catalog_router() -> Router<AppState> {
    Router::new()
        .route(
            "/owner/catalog/items",
            get(list_items).post(create_item),
        )
        .route("/owner/catalog/items/{item_id}", patch(patch_item))
        .route(
            "/owner/catalog/categories",
            get(list_categories).post(create_category),
        )
        .route(
            "/owner/catalog/categories/{category_id}",
            patch(patch_category),
        )
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct ListItemsQuery {
    /// Include deactivated items. Off by default, because the common case is picking an
    /// item for a new policy, where only active items may be chosen (§8.3).
    pub include_inactive: Option<bool>,
}

/// My catalogue items.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/catalog/items",
    tag = "catalog",
    params(ListItemsQuery),
    responses((status = 200, description = "Catalogue items", body = CatalogItemsResponse)),
    security(("firebase" = [])),
)]
pub async fn list_items(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Query(query): Query<ListItemsQuery>,
) -> ApiResult<ApiOk<CatalogItemsResponse>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    let items = state
        .catalog
        .list_items(
            &state.pool,
            store.id,
            query.include_inactive.unwrap_or(false),
        )
        .await?;

    Ok(ApiOk(CatalogItemsResponse { items }))
}

/// Add a catalogue item.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/catalog/items",
    tag = "catalog",
    request_body = CreateItemRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Item created", body = CatalogItem),
        (status = 409, description = "The SKU is already in use"),
    ),
    security(("firebase" = [])),
)]
pub async fn create_item(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Json(request): Json<CreateItemRequest>,
) -> ApiResult<ApiMutation<CatalogItem>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let item = state
        .catalog
        .create_item(&state.pool, store.id, &request)
        .await?;

    let transaction_id = TransactionId::new();
    tracing::Span::current().record("store_id", tracing::field::display(store.id));
    tracing::info!(item_id = %item.id, "catalog.item_created");

    Ok(ApiMutation::created(item, transaction_id))
}

/// Edit a catalogue item, including deactivating it.
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/owner/catalog/items/{item_id}",
    tag = "catalog",
    request_body = UpdateItemRequest,
    params(
        ("item_id" = Uuid, Path, description = "Catalogue item id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
        ("If-Match" = Option<String>, Header, description = "Expected version"),
    ),
    responses(
        (status = 200, description = "Updated item", body = CatalogItem),
        (status = 404, description = "No such item in this store"),
        (status = 409, description = "Someone else changed it first"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_item(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(item_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<UpdateItemRequest>,
) -> ApiResult<ApiMutation<CatalogItem>> {
    request.validate()?;
    let expected_version = concurrency::expected_version(&headers, request.version)?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let item = state
        .catalog
        .update_item(&state.pool, store.id, item_id, &request, expected_version)
        .await?;

    tracing::info!(item_id = %item.id, "catalog.item_updated");
    Ok(ApiMutation::ok(item, TransactionId::new()))
}

/// My catalogue categories.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/catalog/categories",
    tag = "catalog",
    responses((status = 200, description = "Categories", body = CatalogCategoriesResponse)),
    security(("firebase" = [])),
)]
pub async fn list_categories(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
) -> ApiResult<ApiOk<CatalogCategoriesResponse>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let categories = state.catalog.list_categories(&state.pool, store.id).await?;

    Ok(ApiOk(CatalogCategoriesResponse { categories }))
}

/// Add a category.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/catalog/categories",
    tag = "catalog",
    request_body = CreateCategoryRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Category created", body = CatalogCategory),
        (status = 409, description = "The name is already used in this store"),
    ),
    security(("firebase" = [])),
)]
pub async fn create_category(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Json(request): Json<CreateCategoryRequest>,
) -> ApiResult<ApiMutation<CatalogCategory>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let category = state
        .catalog
        .create_category(&state.pool, store.id, &request)
        .await?;

    tracing::info!(category_id = %category.id, "catalog.category_created");
    Ok(ApiMutation::created(category, TransactionId::new()))
}

/// Edit a category.
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/owner/catalog/categories/{category_id}",
    tag = "catalog",
    request_body = UpdateCategoryRequest,
    params(
        ("category_id" = Uuid, Path, description = "Category id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
        ("If-Match" = Option<String>, Header, description = "Expected version"),
    ),
    responses(
        (status = 200, description = "Updated category", body = CatalogCategory),
        (status = 404, description = "No such category in this store"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_category(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(category_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<UpdateCategoryRequest>,
) -> ApiResult<ApiMutation<CatalogCategory>> {
    request.validate()?;
    let expected_version = concurrency::expected_version(&headers, request.version)?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let category = state
        .catalog
        .update_category(
            &state.pool,
            store.id,
            category_id,
            &request,
            expected_version,
        )
        .await?;

    tracing::info!(category_id = %category.id, "catalog.category_updated");
    Ok(ApiMutation::ok(category, TransactionId::new()))
}
