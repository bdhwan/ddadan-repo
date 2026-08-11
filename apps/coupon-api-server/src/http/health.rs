//! Liveness and readiness (§18.2).
//!
//! `/health/live` answers "is this process running". `/health/ready` answers "should this
//! process receive traffic": PostgreSQL reachable, migrations at the expected version,
//! required configuration present. Redis is a transport and cache, so losing it degrades
//! features rather than failing readiness.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use utoipa::ToSchema;

use crate::db;
use crate::state::AppState;

pub fn health_router() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LiveResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ComponentHealth {
    /// `ok`, `degraded`, `disabled` or `down`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ComponentHealth {
    fn ok() -> Self {
        Self {
            status: "ok",
            detail: None,
        }
    }

    fn with(status: &'static str, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: Some(detail.into()),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyResponse {
    /// `ready` or `not_ready`. `degraded` components do not change it.
    pub status: &'static str,
    pub postgres: ComponentHealth,
    pub redis: ComponentHealth,
    pub config: ComponentHealth,
    pub migration_version: Option<i64>,
    pub expected_migration_version: Option<i64>,
}

/// Liveness. Touches nothing external on purpose: a dependency outage must not make the
/// orchestrator kill a healthy process.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/health/live",
    tag = "health",
    responses((status = 200, description = "Process is running", body = LiveResponse)),
)]
pub async fn live() -> Json<LiveResponse> {
    Json(LiveResponse {
        status: "live",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Readiness.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/health/ready",
    tag = "health",
    responses(
        (status = 200, description = "Ready for traffic", body = ReadyResponse),
        (status = 503, description = "Not ready", body = ReadyResponse),
    ),
)]
pub async fn ready(State(state): State<AppState>) -> Response {
    let expected = db::expected_migration_version();

    let (postgres, applied) = match db::applied_migration_version(&state.pool).await {
        Ok(applied) if applied == expected => (ComponentHealth::ok(), applied),
        Ok(applied) => (
            ComponentHealth::with(
                "down",
                format!(
                    "migration version mismatch: database at {applied:?}, binary expects {expected:?}"
                ),
            ),
            applied,
        ),
        Err(error) => (ComponentHealth::with("down", error.to_string()), None),
    };

    let redis = check_redis(&state).await;

    // Configuration is validated at boot, so reaching here means it held. Re-report it so
    // one probe answers the whole §18.2 checklist.
    let config = ComponentHealth::ok();

    let ready = postgres.status == "ok";
    let body = ReadyResponse {
        status: if ready { "ready" } else { "not_ready" },
        postgres,
        redis,
        config,
        migration_version: applied,
        expected_migration_version: expected,
    };

    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(body)).into_response()
}

/// Redis is optional. A failure is reported as `degraded`, never as not-ready (§18.2).
pub async fn check_redis(state: &AppState) -> ComponentHealth {
    let Some(redis) = &state.redis else {
        return ComponentHealth::with("disabled", "COUPON_REDIS_URL is not configured");
    };

    match redis.client.get_multiplexed_async_connection().await {
        Ok(mut connection) => {
            match redis::cmd("PING")
                .query_async::<String>(&mut connection)
                .await
            {
                Ok(_) => ComponentHealth::ok(),
                Err(error) => ComponentHealth::with("degraded", error.to_string()),
            }
        }
        Err(error) => ComponentHealth::with("degraded", error.to_string()),
    }
}
