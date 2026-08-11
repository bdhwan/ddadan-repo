//! `GET /owner/analytics` (§11.4, §6.3, §19).

use axum::extract::State;

use crate::http::query::Query;
use axum::routing::get;
use axum::Router;
use chrono::NaiveDate;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::analytics::AnalyticsResponse;
use crate::auth::extractors::StoreOwner;
use crate::error::ApiResult;
use crate::http::response::ApiOk;
use crate::state::AppState;

/// §6.3's default view is the last 30 business days.
const DEFAULT_WINDOW_DAYS: i64 = 30;

pub fn owner_analytics_router() -> Router<AppState> {
    Router::new().route("/owner/analytics", get(get_analytics))
}

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct AnalyticsQuery {
    /// First business day, inclusive. Defaults to 29 days before `to`.
    pub from: Option<NaiveDate>,
    /// Last business day, inclusive. Defaults to today in the store's own timezone.
    pub to: Option<NaiveDate>,
}

/// 기간별 적립·리워드·캠페인·취소 지표 (§6.3, §19).
///
/// Every day in the range appears in the response, including the ones the nightly batch
/// has not reached — those carry `state: PENDING` and no numbers at all, because §19 draws
/// the line between 실시간 수치 and 확정 배치 수치 and a zero would erase it.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/analytics",
    tag = "analytics",
    params(
        ("from" = Option<NaiveDate>, Query, description = "First business day (inclusive)"),
        ("to" = Option<NaiveDate>, Query, description = "Last business day (inclusive)"),
    ),
    responses(
        (status = 200, description = "Per-day and total metrics", body = AnalyticsResponse),
        (status = 400, description = "The range is inverted or longer than 366 days"),
        (status = 403, description = "Not a store owner"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_analytics(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Query(query): Query<AnalyticsQuery>,
) -> ApiResult<ApiOk<AnalyticsResponse>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    // The default window is expressed in the store's own business days (§5.2), not in UTC
    // dates: a shop whose day rolls over at 05:00 would otherwise see "today" start in the
    // middle of last night's trading.
    let now = crate::qr::database_now(&state.pool).await?;
    let today = store.calendar()?.business_day(now);

    let to = query.to.unwrap_or(today);
    let from = query
        .from
        .unwrap_or_else(|| to - chrono::Duration::days(DEFAULT_WINDOW_DAYS - 1));

    Ok(ApiOk(
        state.analytics.range(&state.pool, store.id, from, to).await?,
    ))
}
