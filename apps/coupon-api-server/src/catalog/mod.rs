//! 품목·카테고리 (§10.2 `catalog`).
//!
//! Phase 2. Owns `coupon.catalog_categories` and `coupon.catalog_items`, which decide
//! whether a coupon's item restriction is satisfied.
//!
//! Reserved now so that cross-module wiring and the §10.2 boundary are settled before the
//! phase lands. This module has no public surface yet.
//!
//! - TODO(phase-2): item and category CRUD under `/owner/catalog/items` (§11.4).
//! - TODO(phase-2): an item-eligibility service `redemptions` can call without touching
//!   these tables itself.
