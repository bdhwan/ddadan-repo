//! 쿠폰 인스턴스와 소비자 지갑 조회 (§10.2 `wallet`).
//!
//! Phase 3. Owns `coupon_instances` and `coupon_status_events`.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-3): `/me/wallet/coupons` and `/me/wallet/stamps` with the cursor pagination
//!   helper in `http::pagination`.
//! - TODO(phase-3): status transitions written as append-only events whose `from_status`
//!   must match the instance's status at the time (§12.6-8).
