//! 관리자·고위험 행위 감사 (§10.2 `audit`).
//!
//! Phase 2 onward. Owns `audit_logs`, which an append-only trigger protects.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-2): record actor, action, resource, before/after hashes, reason and
//!   `request_id` for every administrative and high-risk user action.
//! - TODO(phase-4): tamper detection over the hash chain.
