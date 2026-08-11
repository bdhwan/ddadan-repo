//! `coupon-api` — the HTTP API server.
//!
//! Boot order matters: configuration is validated before anything is connected, so a
//! misconfigured process (an auth bypass in production, a missing key) fails loudly at
//! start rather than serving traffic it should not.

use std::sync::Arc;

use anyhow::Context;
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::state::{AppState, RedisHandle};
use coupon_api_server::{db, http, openapi, telemetry};
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("invalid configuration")?;
    telemetry::init(&config);

    // `--dump-openapi` writes the spec the Angular apps generate their DTOs from, then
    // exits without touching a database.
    if std::env::args().any(|arg| arg == "--dump-openapi") {
        let path = std::env::args()
            .skip_while(|arg| arg != "--dump-openapi")
            .nth(1)
            .filter(|arg| !arg.starts_with("--"))
            .unwrap_or_else(|| openapi::SPEC_FILE.to_owned());

        std::fs::write(&path, openapi::spec_json())
            .with_context(|| format!("failed to write {path}"))?;
        println!("wrote {path}");
        return Ok(());
    }

    tracing::info!(
        env = config.env.as_str(),
        bind = %config.bind_addr,
        auth_dev_bypass = config.auth_dev_bypass,
        "starting coupon-api",
    );

    if config.auth_dev_bypass {
        tracing::warn!(
            "COUPON_AUTH_DEV_BYPASS is enabled: any request may claim any identity. \
             This is refused in production."
        );
    }

    let pool = db::connect(&config)
        .await
        .context("failed to connect to PostgreSQL")?;

    // Report a migration mismatch at boot instead of leaving it to the first probe.
    match db::applied_migration_version(&pool).await {
        Ok(applied) if applied == db::expected_migration_version() => {
            tracing::info!(?applied, "database schema is at the expected version");
        }
        Ok(applied) => tracing::error!(
            ?applied,
            expected = ?db::expected_migration_version(),
            "database schema version mismatch: run `sqlx migrate run`",
        ),
        Err(error) => tracing::error!(%error, "could not read the applied migration version"),
    }

    let redis = match &config.redis_url {
        Some(url) => match redis::Client::open(url.as_str()) {
            Ok(client) => Some(RedisHandle { client }),
            // Redis is a transport, not a source of truth: a bad URL degrades features
            // rather than blocking startup (§18.2).
            Err(error) => {
                tracing::error!(%error, "invalid COUPON_REDIS_URL; continuing without Redis");
                None
            }
        },
        None => {
            tracing::warn!("COUPON_REDIS_URL is not set; Redis-backed features are disabled");
            None
        }
    };

    let sealer = Sealer::from_config(&config).context("invalid data encryption key")?;
    let lookup_hash = LookupHash::from_config(&config).context("invalid lookup hash secret")?;

    let bind_addr = config.bind_addr;
    let state = AppState::new(Arc::new(config), pool, redis, sealer, lookup_hash)
        .map_err(|error| anyhow::anyhow!("failed to build application state: {error}"))?;
    let app = http::router::build(state.clone());

    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;

    tracing::info!(addr = %bind_addr, base_path = http::API_BASE_PATH, "coupon-api listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    // Let in-flight database work finish before the process exits.
    state.pool.close().await;
    tracing::info!("coupon-api stopped");
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
