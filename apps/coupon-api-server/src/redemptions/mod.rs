//! 예약·사용·취소와 할인 계산 (§10.2 `redemptions`, §13.3).
//!
//! Phase 3. Owns `redemption_reservations` and `redemption_transactions`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-3): reserve → confirm with at most one active reservation and one confirmed
//!   use per coupon (§12.6-6), both enforced by partial unique indexes.
//! - TODO(phase-3): discount calculation from the coupon's frozen `condition_snapshot`, not
//!   from the campaign as it stands today.
