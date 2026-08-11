//! `coupon-worker` — asynchronous job execution (§10.3, §14).
//!
//! Phase 4 brings the Apalis queues. What runs here today is the one background job
//! Phase 2 needs: the expiry sweep (§18.1).
//!
//! The sweep is deliberately *not* what decides whether a benefit is usable. Every online
//! read compares `expires_at` itself, so this only writes the `EXPIRE` rows and terminal
//! statuses that the reads already assume — which is why a worker that is down for an
//! hour is a tidiness problem rather than a correctness one (STAMP-006).
//!
//! TODO(phase-4): consume the Apalis queues, honouring §14.5 — one runner per job unique
//!   key, `job_registry` as the source of truth, Redis as transport only. Recover missed
//!   enqueues from `outbox_events`, checkpoint long jobs, and dead-letter after the §14.7
//!   retry budget. The sweep below then becomes one registered job among many rather than
//!   a bare interval.
//! TODO(phase-4): expose worker health as last-heartbeat plus per-queue poll time
//!   (§18.2), rather than reusing the API's readiness contract.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::state::AppState;
use coupon_api_server::{db, telemetry};
use tokio::signal;

/// How often the idle skeleton reports that it is alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
/// §18.1 allows five minutes for expiry state to catch up.
const SWEEP_INTERVAL: Duration = Duration::from_secs(300);
/// Rows per sweep. Bounded so one very overdue batch cannot hold a connection for
/// minutes; the next tick picks up whatever is left.
const SWEEP_BATCH: i64 = 1_000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    telemetry::init(&config);

    tracing::info!(
        env = config.env.as_str(),
        "starting coupon-worker (expiry sweep only; queues land in phase 4)",
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

    let sealer = Sealer::from_config(&config).context("invalid data encryption key")?;
    let lookup_hash = LookupHash::from_config(&config).context("invalid lookup hash secret")?;
    let state = AppState::new(Arc::new(config), pool.clone(), None, sealer, lookup_hash)
        .map_err(|error| anyhow::anyhow!("failed to build worker state: {error}"))?;

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await; // The first tick completes immediately.
    let mut sweep = tokio::time::interval(SWEEP_INTERVAL);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                tracing::info!(queues = 0, "coupon-worker heartbeat");
            }
            _ = sweep.tick() => run_expiry_sweep(&state).await,
            _ = shutdown_signal() => break,
        }
    }

    pool.close().await;
    tracing::info!("coupon-worker stopped");
    Ok(())
}

/// Write the `EXPIRE` ledger rows and terminal coupon statuses that online reads already
/// behave as though exist (§18.1).
///
/// A failure is logged and the next tick retries. There is nothing to roll back and
/// nothing a customer is waiting on, so a sweep that cannot run must not take the process
/// down with it.
async fn run_expiry_sweep(state: &AppState) {
    let now = chrono::Utc::now();

    match state
        .loyalty_stamps
        .expire_due_lots(&state.pool, now, SWEEP_BATCH)
        .await
    {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "loyalty.stamp_lots_expired"),
        Err(error) => tracing::error!(%error, "stamp expiry sweep failed"),
    }

    match state
        .wallet
        .expire_due_coupons(&state.pool, now, SWEEP_BATCH)
        .await
    {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "wallet.coupons_expired"),
        Err(error) => tracing::error!(%error, "coupon expiry sweep failed"),
    }
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
