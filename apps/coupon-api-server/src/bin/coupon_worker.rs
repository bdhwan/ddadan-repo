//! `coupon-worker` — asynchronous job execution (§10.3, §14).
//!
//! Three loops, each doing one thing:
//!
//! * **relay** — publish `outbox_events` the API committed but could not deliver. This is
//!   what makes §14.2's "커밋과 enqueue 사이 유실 방지" true rather than aspirational.
//! * **poll** — claim and run whatever the registry says is due. It is both the JOB-005
//!   fallback when Redis is unavailable and the recovery path for a message that was
//!   published and then lost.
//! * **schedule** — register the recurring expiry shard (§14.6 시간 shard).
//!
//! Redis, when configured, delivers messages promptly through Apalis so a job does not
//! wait for the next poll tick. When it is not, everything above still works: PostgreSQL
//! is the source of truth and the poll interval becomes the worst-case start latency.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use apalis::prelude::{Data, Monitor, WorkerBuilder, WorkerFactoryFn};
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::jobs::transport::{
    JobMessage, JobTransport, RedisJobTransport, RegistryOnlyTransport,
};
use coupon_api_server::jobs::worker::JobRuntime;
use coupon_api_server::jobs::{JobKey, JobSpec};
use coupon_api_server::state::AppState;
use coupon_api_server::{db, telemetry};
use tokio::signal;

/// How often the idle skeleton reports that it is alive.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
/// How often the registry is polled for due work. Short enough that a job whose Redis
/// message was lost still starts promptly; long enough not to hammer the database.
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// How often unpublished outbox rows are retried.
const RELAY_INTERVAL: Duration = Duration::from_secs(5);
/// §18.1 allows five minutes for expiry state to catch up.
const SWEEP_INTERVAL: Duration = Duration::from_secs(300);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    telemetry::init(&config);

    let pool = db::connect(&config)
        .await
        .context("failed to connect to PostgreSQL")?;

    match db::applied_migration_version(&pool).await {
        Ok(applied) => tracing::info!(?applied, "connected to PostgreSQL"),
        Err(error) => tracing::error!(%error, "could not read the applied migration version"),
    }

    // The transport is chosen once at boot. Losing Redis later degrades delivery latency
    // rather than correctness, so this is not a readiness gate (§18.2, JOB-005).
    let (transport, redis_storage): (Arc<dyn JobTransport>, _) = match &config.redis_url {
        Some(url) => match RedisJobTransport::connect(url).await {
            Ok(redis) => {
                tracing::info!("job transport: apalis over Redis");
                let storage = redis.storage();
                (Arc::new(redis), Some(storage))
            }
            Err(error) => {
                tracing::error!(%error, "could not reach Redis; falling back to the registry poll");
                (Arc::new(RegistryOnlyTransport), None)
            }
        },
        None => {
            tracing::warn!(
                "COUPON_REDIS_URL is not set; jobs will start on the registry poll interval"
            );
            (Arc::new(RegistryOnlyTransport), None)
        }
    };

    let sealer = Sealer::from_config(&config).context("invalid data encryption key")?;
    let lookup_hash = LookupHash::from_config(&config).context("invalid lookup hash secret")?;
    let state = AppState::new(Arc::new(config), pool.clone(), None, sealer, lookup_hash)
        .map_err(|error| anyhow::anyhow!("failed to build worker state: {error}"))?;

    let runtime = Arc::new(JobRuntime::new(state.clone(), transport));
    tracing::info!(worker_id = runtime.worker_id(), "starting coupon-worker");

    // The Apalis side is delivery only: the handler hands the id straight to the runtime,
    // which re-reads the registry and takes the advisory lock (§14.5-4/5). Apalis never
    // decides whether the work should run.
    let queue = redis_storage.map(|storage| {
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let monitor = Monitor::new().register(
                WorkerBuilder::new("coupon-jobs")
                    .data(runtime)
                    .backend(storage)
                    .build_fn(deliver),
            );

            if let Err(error) = monitor.run().await {
                tracing::error!(%error, "the Redis job consumer stopped");
            }
        })
    });

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.tick().await; // The first tick completes immediately.
    let mut relay = tokio::time::interval(RELAY_INTERVAL);
    let mut poll = tokio::time::interval(POLL_INTERVAL);
    let mut sweep = tokio::time::interval(SWEEP_INTERVAL);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                tracing::info!(worker_id = runtime.worker_id(), "coupon-worker heartbeat");
            }
            _ = relay.tick() => {
                match runtime.relay().await {
                    Ok(0) => {}
                    Ok(count) => tracing::info!(count, "jobs.relayed"),
                    Err(error) => tracing::error!(%error, "outbox relay failed"),
                }
            }
            _ = poll.tick() => {
                match runtime.poll().await {
                    Ok(0) => {}
                    Ok(count) => tracing::debug!(count, "jobs.ran"),
                    Err(error) => tracing::error!(%error, "job poll failed"),
                }
            }
            _ = sweep.tick() => schedule_expiry(&state).await,
            _ = shutdown_signal() => break,
        }
    }

    if let Some(queue) = queue {
        queue.abort();
    }
    pool.close().await;
    tracing::info!("coupon-worker stopped");
    Ok(())
}

/// Apalis's side of the contract: receive an id, run it, and never fail.
///
/// Returning `Ok` unconditionally is deliberate. Retries, dead-lettering and backoff all
/// live in `job_registry` (§14.4, §14.7); if Apalis retried as well, one failure would be
/// counted twice and two schedules would fight over the same job.
async fn deliver(
    message: JobMessage,
    runtime: Data<Arc<JobRuntime>>,
) -> Result<(), apalis::prelude::Error> {
    if let Err(error) = runtime.run_once(message.job_id).await {
        tracing::error!(%error, job_id = %message.job_id, "job delivery failed");
    }
    Ok(())
}

/// Register this hour's expiry shard (§14.6 시간 shard, JOB-004).
///
/// Registering the same shard twice is the ordinary case — every worker tries, every five
/// minutes — and the active-key unique index makes all but the first a no-op (§12.6-10).
async fn schedule_expiry(state: &AppState) {
    let now = chrono::Utc::now();
    let spec = JobSpec::new(
        JobKey::expire_coupons(now),
        serde_json::json!({ "shard": now.format("%Y-%m-%dT%H:00Z").to_string() }),
    );

    let mut tx = match state.pool.begin().await {
        Ok(tx) => tx,
        Err(error) => {
            tracing::error!(%error, "could not open a transaction to schedule the expiry sweep");
            return;
        }
    };

    match state.jobs.enqueue(&mut tx, &spec).await {
        Ok(job) => {
            if let Err(error) = tx.commit().await {
                tracing::error!(%error, "could not schedule the expiry sweep");
            } else if !job.deduplicated {
                tracing::info!(job_id = %job.job_id, "jobs.expiry_shard_scheduled");
            }
        }
        Err(error) => tracing::error!(%error, "could not register the expiry sweep"),
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
