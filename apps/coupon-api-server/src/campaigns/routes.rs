//! `/owner/campaigns` (§11.4) and `/campaigns/:id/claims` (§11.3).

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;
use validator::Validate;

use crate::auth::extractors::{CurrentUser, RecentlyAuthenticated, StoreOwner};
use crate::campaigns::{
    Campaign, CancelCampaignRequest, ClaimedCoupon, CreateCampaignRequest, PauseCampaignRequest,
    PublishEstimate, PublishedCampaign, UpdateCampaignRequest,
};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::concurrency;
use crate::http::middleware::idempotency::IDEMPOTENCY_KEY_HEADER;
use crate::http::pagination::{Page, PageQuery};
use crate::http::query::Query;
use crate::http::rate_limit::Bucket;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::state::AppState;

pub fn owner_campaign_router() -> Router<AppState> {
    Router::new()
        .route("/owner/campaigns", get(list_campaigns).post(create_campaign))
        .route(
            "/owner/campaigns/{campaign_id}",
            get(get_campaign).patch(patch_campaign),
        )
        .route(
            "/owner/campaigns/{campaign_id}/estimate",
            get(estimate_campaign),
        )
        .route("/owner/campaigns/{campaign_id}/publish", post(publish_campaign))
        .route("/owner/campaigns/{campaign_id}/pause", post(pause_campaign))
        .route("/owner/campaigns/{campaign_id}/resume", post(resume_campaign))
        .route("/owner/campaigns/{campaign_id}/cancel", post(cancel_campaign))
}

pub fn campaign_claim_router() -> Router<AppState> {
    Router::new().route("/campaigns/{campaign_id}/claims", post(claim_campaign))
}

/// My campaigns, newest first, cursor-paginated (§11.1).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/campaigns",
    tag = "campaigns",
    params(
        ("limit" = Option<u32>, Query, description = "1–100, default 20"),
        ("cursor" = Option<String>, Query, description = "next_cursor from the previous page"),
    ),
    responses((status = 200, description = "Campaigns", body = Page<Campaign>)),
    security(("firebase" = [])),
)]
pub async fn list_campaigns(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Query(page): Query<PageQuery>,
) -> ApiResult<ApiOk<Page<Campaign>>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    Ok(ApiOk(
        state.campaigns.list(&state.pool, store.id, &page).await?,
    ))
}

/// One campaign.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}",
    tag = "campaigns",
    params(("campaign_id" = Uuid, Path, description = "Campaign id")),
    responses(
        (status = 200, description = "Campaign", body = Campaign),
        (status = 404, description = "No such campaign in this store"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_campaign(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(campaign_id): Path<Uuid>,
) -> ApiResult<ApiOk<Campaign>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    Ok(ApiOk(
        state
            .campaigns
            .find(&state.pool, store.id, campaign_id)
            .await?,
    ))
}

/// Draft a campaign (CAMPAIGN-001).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/campaigns",
    tag = "campaigns",
    request_body = CreateCampaignRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "Draft created", body = Campaign),
        (status = 400, description = "A benefit, quantity or schedule value is out of range"),
    ),
    security(("firebase" = [])),
)]
pub async fn create_campaign(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Json(request): Json<CreateCampaignRequest>,
) -> ApiResult<ApiMutation<Campaign>> {
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let campaign = state
        .campaigns
        .create(&state.pool, &store, user.account.user_id, &request)
        .await?;

    tracing::info!(campaign_id = %campaign.id, "campaign.drafted");
    Ok(ApiMutation::created(campaign, TransactionId::new()))
}

/// Edit a draft, or the forward-looking fields of a published campaign (CAMPAIGN-008).
#[utoipa::path(
    patch,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}",
    tag = "campaigns",
    request_body = UpdateCampaignRequest,
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
        ("If-Match" = Option<String>, Header, description = "Expected version"),
    ),
    responses(
        (status = 200, description = "Updated", body = Campaign),
        (status = 422, description = "The edit would apply retroactively to issued coupons"),
    ),
    security(("firebase" = [])),
)]
pub async fn patch_campaign(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(campaign_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<UpdateCampaignRequest>,
) -> ApiResult<ApiMutation<Campaign>> {
    request.validate()?;
    let expected_version = concurrency::expected_version(&headers, request.version)?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let campaign = state
        .campaigns
        .update(&state.pool, &store, campaign_id, &request, expected_version)
        .await?;

    Ok(ApiMutation::ok(campaign, TransactionId::new()))
}

/// 대상 예상 인원과 최대 발급 비용 (CAMPAIGN-002), before the confirmation modal.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}/estimate",
    tag = "campaigns",
    params(("campaign_id" = Uuid, Path, description = "Campaign id")),
    responses((status = 200, description = "What publishing would commit to", body = PublishEstimate)),
    security(("firebase" = [])),
)]
pub async fn estimate_campaign(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(campaign_id): Path<Uuid>,
) -> ApiResult<ApiOk<PublishEstimate>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;

    Ok(ApiOk(
        state
            .campaigns
            .estimate(&state.pool, &store, campaign_id)
            .await?,
    ))
}

/// Publish, and register the issuing job (§11.4, CAMPAIGN-003).
///
/// [`RecentlyAuthenticated`] rather than [`StoreOwner`]: CAMPAIGN-002 classes publishing
/// as 고위험 작업 requiring re-authentication, and this is the same treatment §9.3 gives
/// withdrawal and business-identity edits.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}/publish",
    tag = "campaigns",
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Published", body = PublishedCampaign),
        (status = 403, description = "Re-authentication is required for this action"),
        (status = 422, description = "A publication rule is not satisfied"),
    ),
    security(("firebase" = [])),
)]
pub async fn publish_campaign(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(campaign_id): Path<Uuid>,
) -> ApiResult<ApiMutation<PublishedCampaign>> {
    user.require_role(crate::users::AccountRole::StoreOwner)?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let published = state
        .campaigns
        .publish(&state.pool, &store, user.account.user_id, campaign_id)
        .await?;

    Ok(ApiMutation::ok(published, TransactionId::new()))
}

/// Stop new issuance, keeping what customers already hold (CAMPAIGN-006).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}/pause",
    tag = "campaigns",
    request_body = PauseCampaignRequest,
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Paused", body = Campaign),
        (status = 422, description = "Not a campaign that can be paused"),
    ),
    security(("firebase" = [])),
)]
pub async fn pause_campaign(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(campaign_id): Path<Uuid>,
    request: Option<Json<PauseCampaignRequest>>,
) -> ApiResult<ApiMutation<Campaign>> {
    let request = request.map(|Json(body)| body).unwrap_or_default();
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let campaign = state
        .campaigns
        .pause(
            &state.pool,
            &store,
            user.account.user_id,
            campaign_id,
            &request,
        )
        .await?;

    Ok(ApiMutation::ok(campaign, TransactionId::new()))
}

/// Continue from the checkpoint (CAMPAIGN-006).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}/resume",
    tag = "campaigns",
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Resumed", body = Campaign),
        (status = 422, description = "Not a paused campaign, or the issuing period has ended"),
    ),
    security(("firebase" = [])),
)]
pub async fn resume_campaign(
    State(state): State<AppState>,
    StoreOwner(user): StoreOwner,
    Path(campaign_id): Path<Uuid>,
) -> ApiResult<ApiMutation<Campaign>> {
    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let campaign = state
        .campaigns
        .resume(&state.pool, &store, user.account.user_id, campaign_id)
        .await?;

    Ok(ApiMutation::ok(campaign, TransactionId::new()))
}

/// Cancel, naming what happens to coupons already issued (CAMPAIGN-007).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/owner/campaigns/{campaign_id}/cancel",
    tag = "campaigns",
    request_body = CancelCampaignRequest,
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Cancelled", body = Campaign),
        (status = 422, description = "Already ended or cancelled"),
    ),
    security(("firebase" = [])),
)]
pub async fn cancel_campaign(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    Path(campaign_id): Path<Uuid>,
    request: Option<Json<CancelCampaignRequest>>,
) -> ApiResult<ApiMutation<Campaign>> {
    user.require_role(crate::users::AccountRole::StoreOwner)?;
    let request = request.map(|Json(body)| body).unwrap_or_default();
    request.validate()?;

    let store = state
        .stores
        .owned_store(&state.pool, user.account.user_id)
        .await?;
    let campaign = state
        .campaigns
        .cancel(
            &state.pool,
            &store,
            user.account.user_id,
            campaign_id,
            &request,
        )
        .await?;

    Ok(ApiMutation::ok(campaign, TransactionId::new()))
}

/// 선착순 쿠폰 받기 (§11.3, CAMPAIGN-004).
///
/// The rate limit runs *before* the transaction, not after: SEC-003 wants a bot slowed
/// down before it can take stock, and is explicit that a legitimate customer must never
/// have a coupon revoked after the fact.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/campaigns/{campaign_id}/claims",
    tag = "campaigns",
    params(
        ("campaign_id" = Uuid, Path, description = "Campaign id"),
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "Claimed, or the coupon already held", body = ClaimedCoupon),
        (status = 409, description = "CAMPAIGN_SOLD_OUT"),
        (status = 422, description = "Not eligible, or the campaign is not issuing"),
        (status = 429, description = "Too many attempts"),
    ),
    security(("firebase" = [])),
)]
pub async fn claim_campaign(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(campaign_id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<ApiMutation<ClaimedCoupon>> {
    state
        .rate_limiter
        .check(
            Bucket::CampaignClaim,
            &format!("{}:{campaign_id}", user.account.user_id),
            state.config.rate_limit_campaign_claim_per_min,
            chrono::Utc::now(),
        )
        .await?;

    let claim = state
        .campaigns
        .claim(
            &state.pool,
            campaign_id,
            user.account.user_id,
            idempotency_key(&headers)?,
        )
        .await?;

    Ok(ApiMutation::created(claim, TransactionId::new()))
}

/// The claim writes the key to `issuance_deduplications`, so one key produces at most one
/// coupon even if the middleware were bypassed (§12.6-9).
fn idempotency_key(headers: &HeaderMap) -> ApiResult<Uuid> {
    headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(ErrorCode::IdempotencyKeyRequired))
        .and_then(|raw| {
            Uuid::parse_str(raw.trim()).map_err(|_| ApiError::new(ErrorCode::IdempotencyKeyInvalid))
        })
}
