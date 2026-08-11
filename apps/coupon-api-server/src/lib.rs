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

// Later-phase modules: names and boundaries are reserved now so that cross-module
// wiring does not have to be reshuffled when the phase lands.
pub mod admin;
pub mod analytics;
pub mod audit;
pub mod campaigns;
pub mod catalog;
pub mod jobs;
pub mod loyalty;
pub mod notifications;
pub mod qr;
pub mod redemptions;
pub mod wallet;

/// Migrations are applied out-of-band (`sqlx migrate run`), never by the application at
/// boot. This embeds them only so `/health/ready` can compare the expected head version
/// against what the database actually has (§10.3).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
