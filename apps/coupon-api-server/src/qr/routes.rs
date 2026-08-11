//! `/me/qr-tokens` (§11.3, WALLET-003).

use axum::extract::State;
use axum::routing::post;
use axum::Router;

use crate::auth::extractors::CurrentUser;
use crate::error::ApiResult;
use crate::http::rate_limit::Bucket;
use crate::http::response::{ApiMutation, TransactionId};
use crate::qr::{AUDIENCE_STAMP, QrTokenResponse};
use crate::state::AppState;

pub fn qr_router() -> Router<AppState> {
    Router::new().route("/me/qr-tokens", post(issue_qr_token))
}

/// Issue a 60-second rotating QR and its manual code.
///
/// Rate limited to 20/minute per user (§16.4): a legitimate screen refreshes every 30
/// seconds, so anything approaching the ceiling is a script harvesting nonces.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/me/qr-tokens",
    tag = "qr",
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 201, description = "A fresh rotating token", body = QrTokenResponse),
        (status = 403, description = "The account may not present a QR"),
        (status = 429, description = "Too many tokens requested"),
    ),
    security(("firebase" = [])),
)]
pub async fn issue_qr_token(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiMutation<QrTokenResponse>> {
    state
        .rate_limiter
        .check(
            Bucket::QrIssue,
            &user.account.user_id.to_string(),
            state.config.rate_limit_qr_issue_per_min,
            chrono::Utc::now(),
        )
        .await?;

    let token = state
        .qr
        .issue(
            &state.pool,
            user.account.user_id,
            user.account.consumer_key,
            AUDIENCE_STAMP,
        )
        .await?;

    let transaction_id = TransactionId::new();
    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    // The token and the code are secrets for the length of one scan; only the metadata is
    // ever logged (§16.3).
    tracing::info!(
        key_id = %token.key_id,
        expires_at = %token.expires_at,
        "qr.token_issued"
    );

    Ok(ApiMutation::created(token, transaction_id))
}

/// The response body deliberately has no `Deserialize`, so this is checked by shape.
#[cfg(test)]
mod tests {
    use crate::qr::QrTokenResponse;
    use chrono::{Duration, Utc};

    #[test]
    fn the_response_tells_the_screen_when_to_refresh_before_it_expires() {
        let issued_at = Utc::now();
        let response = QrTokenResponse {
            token: "header.payload.signature".to_owned(),
            fallback_code: "01234567".to_owned(),
            issued_at,
            expires_at: issued_at + Duration::seconds(60),
            expires_in_seconds: 60,
            refresh_after_seconds: 30,
            key_id: "abcd".to_owned(),
        };

        assert!(
            response.refresh_after_seconds < response.expires_in_seconds,
            "a screen that waits for expiry would show a dead QR (WALLET-003)"
        );

        let json = serde_json::to_value(&response).expect("serialises");
        assert_eq!(json["fallback_code"], "01234567");
        assert!(
            json.get("nonce").is_none(),
            "the raw nonce is never returned outside the token itself"
        );
    }
}
