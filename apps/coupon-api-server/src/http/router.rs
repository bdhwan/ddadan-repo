//! Router assembly.
//!
//! Two trees: `public` (health, and later the public store catalogue) and `protected`,
//! which carries the auth and idempotency layers. Routes are mounted on the tree that
//! matches their access rule, so a new endpoint cannot accidentally skip authentication.

use std::time::Duration;

use axum::Router;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware;
use axum::response::IntoResponse;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use crate::consents::consents_router;
use crate::error::{ApiError, ErrorCode};
use crate::http::middleware as coupon_middleware;
use crate::http::{API_BASE_PATH, health};
use crate::openapi::swagger_router;
use crate::state::AppState;
use crate::stores::owner_store_router;
use crate::users::{me_router, users_router};

/// Requests are given this long before the server gives up on them.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard ceiling on request bodies, independent of the idempotency middleware's own cap.
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub fn build(state: AppState) -> Router {
    let public = health::health_router();

    let protected = Router::new()
        .merge(users_router())
        .merge(me_router())
        .merge(consents_router())
        .merge(owner_store_router())
        // Applied bottom-up: auth runs first, then origin, then idempotency — which needs
        // the actor auth established.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            coupon_middleware::idempotency::layer,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            coupon_middleware::origin::layer,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            coupon_middleware::auth::layer,
        ));

    let api = Router::new().merge(public).merge(protected);

    Router::new()
        .nest(API_BASE_PATH, api)
        // Infrastructure probes are usually configured against a bare path, so the health
        // endpoints answer there too.
        .merge(health::health_router())
        .merge(swagger_router())
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(middleware::from_fn(coupon_middleware::request_id::layer))
        .layer(cors_layer(&state))
        // 503 rather than tower-http's default 408: from the client's point of view this
        // is a transient server-side failure that is safe to retry (§11.1).
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            REQUEST_TIMEOUT,
        ))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        // A panicking handler becomes a 500 with the standard envelope rather than a
        // dropped connection.
        .layer(CatchPanicLayer::custom(handle_panic))
        .with_state(state)
}

/// CORS for exactly the three app origins (§16.3).
fn cors_layer(state: &AppState) -> CorsLayer {
    let origins: Vec<HeaderValue> = state
        .config
        .allowed_origins
        .as_slice()
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();

    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::IF_MATCH,
            HeaderName::from_static("idempotency-key"),
            HeaderName::from_static("x-request-id"),
        ])
        .expose_headers([
            axum::http::header::ETAG,
            HeaderName::from_static("x-request-id"),
            HeaderName::from_static("idempotent-replay"),
        ])
        .max_age(Duration::from_secs(600));

    if origins.is_empty() {
        // Only reachable outside production, where `Config::validate` does not demand an
        // allowlist. No credentials are allowed, so this stays a same-origin-equivalent
        // grant for bearer-token clients.
        layer.allow_origin(AllowOrigin::any())
    } else {
        layer.allow_origin(origins)
    }
}

async fn not_found() -> impl IntoResponse {
    ApiError::new(ErrorCode::NotFound)
}

async fn method_not_allowed() -> impl IntoResponse {
    ApiError::with_message(
        ErrorCode::InvalidRequest,
        "이 경로에서는 사용할 수 없는 메서드입니다.",
    )
}

fn handle_panic(panic: Box<dyn std::any::Any + Send + 'static>) -> axum::response::Response {
    let detail = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic");

    tracing::error!(detail, "handler panicked");

    let mut response = ApiError::new(ErrorCode::ServiceUnavailable)
        .internal(detail)
        .into_response();
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response
}
