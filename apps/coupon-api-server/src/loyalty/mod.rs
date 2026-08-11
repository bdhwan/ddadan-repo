//! 도장 정책·적립 원장 (§10.2 `loyalty`, §13.1).
//!
//! Phase 2. Owns `loyalty_policies`, `loyalty_reward_definitions`, `stamp_lots`,
//! `stamp_ledger` and `stamp_transactions`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-2): policy versioning with at most one ACTIVE version per store (§12.6-2).
//! - TODO(phase-2): accrual as an append-only ledger; a lot's consumption may never exceed
//!   what was earned (§12.6-3). Balance is `SUM(quantity_delta)`, never a stored counter
//!   that can drift.
//! - TODO(phase-2): goal completion issuing a reward coupon in the same transaction as the
//!   ledger rows that paid for it.
