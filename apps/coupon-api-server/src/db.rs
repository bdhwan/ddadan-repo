//! PostgreSQL pool and readiness probes.
//!
//! The application never runs migrations (§10.3): it only reports whether the database
//! is at the version this binary expects.

use sqlx::postgres::{PgPoolOptions, PgQueryResult};
use sqlx::{PgPool, Postgres, Transaction};

use crate::config::Config;

pub type Tx<'a> = Transaction<'a, Postgres>;

pub async fn connect(config: &Config) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .acquire_timeout(config.database_connect_timeout())
        .connect(&config.database_url)
        .await
}

/// Highest migration version sqlx has applied, or `None` on an empty database.
pub async fn applied_migration_version(pool: &PgPool) -> Result<Option<i64>, sqlx::Error> {
    // The table is absent until the first migration runs, which is a legitimate state to
    // report rather than an error to propagate.
    //
    // `to_regclass` resolves through `search_path`, exactly like the unqualified name the
    // migrator itself uses. Pinning a schema here would be wrong: `current_schema()` is
    // the role-named schema when one exists (our `coupon` role owns a `coupon` schema),
    // while sqlx's bookkeeping table lands in `public`.
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(pool)
        .await?;

    if !exists {
        return Ok(None);
    }

    sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await
}

/// The version this binary was built against.
pub fn expected_migration_version() -> Option<i64> {
    crate::MIGRATOR
        .migrations
        .iter()
        .map(|migration| migration.version)
        .max()
}

/// `true` when exactly one row changed. Used to turn optimistic-concurrency UPDATEs
/// (`WHERE id = $1 AND version = $2`) into a clear conflict signal.
pub fn changed_one_row(result: &PgQueryResult) -> bool {
    result.rows_affected() == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_knows_which_migration_it_expects() {
        assert_eq!(
            expected_migration_version(),
            Some(20260812000300),
            "embedded migrations must include the phase 3 campaign, redemption and job core"
        );
    }
}
