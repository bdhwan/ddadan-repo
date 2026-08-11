//! 캠페인·대상·수량 (§10.2 `campaigns`, §13.2).
//!
//! Phase 3. Owns `campaigns`, `campaign_audience_members` and `campaign_counters`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-3): audience snapshotting at publish time, so a later profile change cannot
//!   retroactively alter who was eligible.
//! - TODO(phase-3): first-come issuing under a conditional UPDATE on `campaign_counters`,
//!   never a read-then-write, so total and daily caps hold under contention (§12.6-4).
