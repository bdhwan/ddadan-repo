//! HTTP transport: routing, middleware, and the response envelope shared by all
//! endpoints (§11.1).

pub mod concurrency;
pub mod health;
pub mod metrics;
pub mod middleware;
pub mod pagination;
pub mod query;
pub mod rate_limit;
pub mod request_id;
pub mod response;
pub mod router;

/// Every endpoint lives under this prefix.
pub const API_BASE_PATH: &str = "/api/coupon/v1";

/// The caller's IP, as our reverse proxy reports it.
///
/// Used for the §9.4 consent record (where it is only ever hashed) and as one half of the
/// §16.4 login rate-limit keys. `X-Forwarded-For` is trusted because the API sits behind
/// our own proxy; a direct-to-internet deployment would need the proxy to overwrite the
/// header.
pub fn client_ip(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
        })
        .map(str::trim)
        .filter(|ip| !ip.is_empty())
        .map(str::to_owned)
}
