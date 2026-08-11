//! `/admin/*` (§11.5).

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use uuid::Uuid;
use validator::Validate;

use crate::admin::{AdjustmentPreview, AdjustmentPreviewRequest, AdminTransactionDetail};
use crate::auth::extractors::SystemAdmin;
use crate::error::ApiResult;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::state::AppState;
use crate::users::AccountRole;

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/admin/transactions/{transaction_id}", get(get_transaction))
        .route("/admin/adjustments/preview", post(preview_adjustment))
}

/// One transaction, its ledger and its whole event history (§2.1 목표 5).
///
/// Readable by every administrative role: answering "what happened to this customer" is
/// support's daily work, and the response carries no unmasked personal data.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/admin/transactions/{transaction_id}",
    tag = "admin",
    params(("transaction_id" = Uuid, Path, description = "Stamp transaction id")),
    responses(
        (status = 200, description = "Ledger and timeline", body = AdminTransactionDetail),
        (status = 403, description = "Not an administrator"),
        (status = 404, description = "No such transaction"),
    ),
    security(("firebase" = [])),
)]
pub async fn get_transaction(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Path(transaction_id): Path<Uuid>,
) -> ApiResult<ApiOk<AdminTransactionDetail>> {
    let detail = state
        .admin
        .transaction_detail(&state.pool, admin.user.account.user_id, transaction_id)
        .await?;

    tracing::info!(%transaction_id, "admin.transaction_viewed");
    Ok(ApiOk(detail))
}

/// Simulate a correction before anyone is allowed to run it (ADMIN-003, §13.4).
///
/// Restricted to the roles §3.3 lets request a correction; approving and executing one is
/// a separate act with its own separation-of-duties rule.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/admin/adjustments/preview",
    tag = "admin",
    request_body = AdjustmentPreviewRequest,
    params(("Idempotency-Key" = String, Header, description = "UUID, required on every mutation")),
    responses(
        (status = 201, description = "A snapshot of what the correction would do", body = AdjustmentPreview),
        (status = 403, description = "This role may not request corrections"),
        (status = 404, description = "No such case or transaction"),
    ),
    security(("firebase" = [])),
)]
pub async fn preview_adjustment(
    State(state): State<AppState>,
    admin: SystemAdmin,
    Json(request): Json<AdjustmentPreviewRequest>,
) -> ApiResult<ApiMutation<AdjustmentPreview>> {
    request.validate()?;
    admin.require_any(&[
        AccountRole::Operations,
        AccountRole::Security,
        AccountRole::SuperAdmin,
    ])?;

    let preview = state
        .admin
        .preview_adjustment(&state.pool, admin.user.account.user_id, &request)
        .await?;

    tracing::info!(
        adjustment_id = %preview.adjustment_id,
        executable = preview.executable,
        "admin.adjustment_previewed"
    );

    Ok(ApiMutation::created(preview, TransactionId::new()))
}
