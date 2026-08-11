//! 검수·제재·보정·민원 (§10.2 `admin`, §11.5).
//!
//! Phase 2 onward. Owns `admin_cases`, `admin_case_notes` and `admin_adjustments`.
//!
//! Phase 1 writes `store_reviews` from the `stores` module (the owner side); the
//! reviewer side — approve, request changes, reject — lands here.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-2): review queue and decisions, moving a store to ACTIVE and stamping
//!   `activated_at`.
//! - TODO(phase-4): adjustment preview → approval → execution, with every high-risk action
//!   written to `audit_logs`.
