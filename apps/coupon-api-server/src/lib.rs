//! DDADAN coupon API server.
//!
//! Module layout follows the module table in the product spec (§10.2). Modules talk to
//! each other through the service structs published in their `mod.rs`; a module never
//! writes another module's tables directly.

pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod http;
pub mod openapi;
pub mod state;
pub mod telemetry;

// Phase 1 domain modules.
pub mod auth;
pub mod consents;
pub mod stores;
pub mod users;

// Phase 2 domain modules: catalogue, policy versions, rotating QR, the stamp ledger, the
// consumer wallet, and the administrative views over them.
pub mod admin;
pub mod audit;
pub mod catalog;
pub mod loyalty;
pub mod qr;
pub mod wallet;

// Phase 3 domain modules: discount campaigns, coupon redemption, and the asynchronous
// job queue that bulk issuance depends on (§14).
pub mod campaigns;
pub mod jobs;
pub mod redemptions;

// Phase 4 domain modules: notification delivery across three channels, the operational
// surface (cases, sanctions, audit search), per-store analytics, and retention and erasure
// (§15, §11.5, §19, §17.3).
pub mod analytics;
pub mod notifications;
pub mod privacy;

/// Migrations are applied out-of-band (`sqlx migrate run`), never by the application at
/// boot. This embeds them only so `/health/ready` can compare the expected head version
/// against what the database actually has (§10.3).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
