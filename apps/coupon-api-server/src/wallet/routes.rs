//! `/me/wallet/*` (§11.3, §6.2).

use axum::extract::{Path, State};

use crate::http::query::Query;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::auth::extractors::CurrentUser;
use crate::error::ApiResult;
use crate::http::pagination::{Page, PageQuery};
use crate::http::response::ApiOk;
use crate::state::AppState;
use crate::wallet::{WalletCoupon, WalletCouponDetail, WalletFilter, WalletStampsResponse};

pub fn wallet_router() -> Router<AppState> {
    Router::new()
        .route("/me/wallet/stamps", get(get_stamps))
        .route("/me/wallet/coupons", get(list_coupons))
        .route("/me/wallet/coupons/{coupon_id}", get(get_coupon))
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct WalletCouponQuery {
    /// `AVAILABLE`, `HISTORY` or `ALL`. Defaults to `ALL` — the wallet never hides a
    /// benefit, it only groups it (§6.2).
    pub status: Option<WalletFilter>,
    pub store_id: Option<Uuid>,
    /// 1–100. Defaults to 20 (§11.1).
    #[serde(default, deserialize_with = "crate::http::pagination::page_size")]
    pub limit: Option<u32>,
    /// `next_cursor` from the previous page.
    pub cursor: Option<String>,
}

impl WalletCouponQuery {
    /// The shared paging parameters. Spelled out rather than flattened so the generated
    /// OpenAPI lists `limit` and `cursor` as real query parameters.
    fn page(&self) -> PageQuery {
        PageQuery {
            limit: self.limit,
            cursor: self.cursor.clone(),
        }
    }
}

/// My stamp boards, one per store.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/wallet/stamps",
    tag = "wallet",
    responses((status = 200, description = "Stamp boards", body = WalletStampsResponse)),
    security(("firebase" = [])),
)]
pub async fn get_stamps(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<WalletStampsResponse>> {
    let stamps = state
        .wallet
        .stamps(&state.pool, user.account.user_id)
        .await?;

    Ok(ApiOk(stamps))
}

/// My coupons.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/wallet/coupons",
    tag = "wallet",
    params(WalletCouponQuery),
    responses((status = 200, description = "One page of coupons", body = Page<WalletCoupon>)),
    security(("firebase" = [])),
)]
pub async fn list_coupons(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<WalletCouponQuery>,
) -> ApiResult<ApiOk<Page<WalletCoupon>>> {
    let page = state
        .wallet
        .coupons(
            &state.pool,
            user.account.user_id,
            query.store_id,
            query.status.unwrap_or_default(),
            &query.page(),
        )
        .await?;

    Ok(ApiOk(page))
}

/// One coupon, with the conditions frozen at issuance and its full history.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/wallet/coupons/{coupon_id}",
    tag = "wallet",
    params(("coupon_id" = Uuid, Path, description = "Coupon instance id")),
    responses(
        (status = 200, description = "Coupon detail", body = WalletCouponDetail),
        (status = 404, description = "No such coupon, or it is not the caller's"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_coupon(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(coupon_id): Path<Uuid>,
) -> ApiResult<ApiOk<WalletCouponDetail>> {
    let coupon = state
        .wallet
        .coupon_detail(&state.pool, user.account.user_id, coupon_id)
        .await?;

    Ok(ApiOk(coupon))
}
