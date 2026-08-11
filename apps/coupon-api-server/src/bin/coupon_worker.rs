//! `coupon-worker` — asynchronous job execution (§10.3, §14).
//!
//! Phase 4 fills this in. The skeleton exists now so deployment, configuration, logging
//! and graceful shutdown are settled and identical to `coupon-api` before any job logic
//! depends on them.
//!
//! The process starts, validates the same configuration, connects to the same database,
//! and idles on a heartbeat. It deliberately does *not* poll Redis or claim jobs yet.
//!
//! TODO(phase-4): consume the Apalis queues, honouring §14.5 — one runner per job unique
//!   key, `job_registry` as the source of truth, Redis as transport only. Recover missed
//!   enqueues from `outbox_events`, checkpoint long jobs, and dead-letter after the §14.7
//!   retry budget.
//! TODO(phase-4): expose worker health as last-heartbeat plus per-queue poll time
//!   (§18.2), rather than reusing the API's readiness contract.

use std::time::Duration;

use anyhow::Context;
use coupon_api_server::config::Config;
use coupon_api_server::{db, telemetry};
use tokio::signal;

/// How often the idle skeleton reports that it is alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    telemetry::init(&config);

    tracing::info!(
        env = config.env.as_str(),
        "starting coupon-worker (phase 4 skeleton: no queues are consumed yet)",
    );

    let pool = db::connect(&config)
        .await
        .context("failed to connect to PostgreSQL")?;

    match db::applied_migration_version(&pool).await {
        Ok(applied) => tracing::info!(?applied, "connected to PostgreSQL"),
        Err(error) => tracing::error!(%error, "could not read the applied migration version"),
    }

    if config.redis_url.is_none() {
        tracing::warn!("COUPON_REDIS_URL is not set; job transport will be unavailable in phase 4");
    }

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await; // The first tick completes immediately.

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                tracing::info!(queues = 0, "coupon-worker heartbeat");
            }
            _ = shutdown_signal() => break,
        }
    }

    pool.close().await;
    tracing::info!("coupon-worker stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT, shutting down"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}
