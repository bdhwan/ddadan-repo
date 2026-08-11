//! HTTP transport: routing, middleware, and the response envelope shared by all
//! endpoints (§11.1).

pub mod concurrency;
pub mod health;
pub mod middleware;
pub mod pagination;
pub mod rate_limit;
pub mod request_id;
pub mod response;
pub mod router;

/// Every endpoint lives under this prefix.
pub const API_BASE_PATH: &str = "/api/coupon/v1";
