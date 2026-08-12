//! `/me/consents` endpoints (§11.2).

use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use validator::Validate;

use crate::auth::extractors::CurrentUser;
use crate::consents::{ConsentEvidence, ConsentsResponse, UpdateConsentsRequest};
use crate::error::ApiResult;
use crate::http::client_ip;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::state::AppState;
use crate::telemetry;

pub fn consents_router() -> Router<AppState> {
    Router::new().route("/me/consents", get(get_consents).post(post_consents))
}

/// My current consent state, including scopes I have never touched.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/consents",
    tag = "consents",
    responses((status = 200, description = "Current consent state", body = ConsentsResponse)),
    security(("firebase" = [])),
)]
pub async fn get_consents(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<ConsentsResponse>> {
    let consents = state
        .consents
        .current(&state.pool, user.account.user_id)
        .await?;
    Ok(ApiOk(ConsentsResponse { consents }))
}

/// Grant or revoke consent. Each change appends an immutable event (§9.4).
#[utoipa::path(
    post,
    path = "/api/coupon/v1/me/consents",
    tag = "consents",
    request_body = UpdateConsentsRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "Consent state after the change", body = ConsentsResponse),
        (status = 422, description = "Required consent cannot be revoked here"),
    ),
    security(("firebase" = [])),
)]
pub async fn post_consents(
    State(state): State<AppState>,
    user: CurrentUser,
    headers: HeaderMap,
    Json(request): Json<UpdateConsentsRequest>,
) -> ApiResult<ApiMutation<ConsentsResponse>> {
    request.validate()?;

    let evidence = ConsentEvidence {
        ip: client_ip(&headers),
        user_agent_class: telemetry::classify_user_agent(
            headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
        ),
    };

    let transaction_id = TransactionId::new();
    let consents = state
        .consents
        .record(&state.pool, user.account.user_id, &request, &evidence)
        .await?;

    tracing::Span::current().record("transaction_id", tracing::field::display(transaction_id));
    tracing::info!(changes = request.consents.len(), "consents.recorded");

    Ok(ApiMutation::ok(
        ConsentsResponse { consents },
        transaction_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("valid header name"),
                HeaderValue::from_str(value).expect("valid header value"),
            );
        }
        map
    }

    #[test]
    fn the_first_forwarded_hop_is_the_client() {
        assert_eq!(
            client_ip(&headers(&[("x-forwarded-for", "203.0.113.7, 10.0.0.1")])),
            Some("203.0.113.7".to_owned())
        );
    }

    #[test]
    fn x_real_ip_is_the_fallback_and_absence_is_allowed() {
        assert_eq!(
            client_ip(&headers(&[("x-real-ip", "203.0.113.9")])),
            Some("203.0.113.9".to_owned())
        );
        assert_eq!(client_ip(&HeaderMap::new()), None);
        assert_eq!(client_ip(&headers(&[("x-forwarded-for", "  ")])), None);
    }
}
