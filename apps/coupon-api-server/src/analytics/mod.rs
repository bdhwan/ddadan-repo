//! 집계와 개인정보 보호 임계값 (§10.2 `analytics`).
//!
//! Phase 4. Owns `analytics_daily_store`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-4): daily per-store rollups rebuilt idempotently from the ledgers.
//! - TODO(phase-4): suppress any bucket below the minimum cohort size before it reaches a
//!   store-facing dashboard.
