//! 도장 정책·적립 원장 (§10.2 `loyalty`, §13.1).
//!
//! Owns `loyalty_policies`, `loyalty_reward_definitions`, `stamp_lots`, `stamp_ledger` and
//! `stamp_transactions`, and is the only module that writes them.
//!
//! * [`policy`] — versioning. At most one `ACTIVE` version per store (§12.6-2), and a new
//!   version never reaches back into stamps already earned (STAMP-008).
//! * [`stamps`] — accrual, goal completion and reversal. Balance is `SUM(quantity_delta)`
//!   over an append-only ledger, so a lot can never be consumed past what it earned
//!   (§12.6-3) and the whole history is reconstructible.
//!
//! Reward coupons are created here but *owned* by `wallet`: this module inserts the
//! instance in the same transaction as the ledger rows that paid for it (STAMP-004), and
//! everything afterwards — listing, expiry, redemption — belongs to the wallet.

pub mod policy;
pub mod routes;
pub mod stamps;

pub use policy::{
    BenefitType, CreatePolicyRequest, LoyaltyPoliciesResponse, LoyaltyPolicy, PolicyRules,
    PolicyService, PolicyStatus, PublishPolicyRequest, RewardDefinition, UpdatePolicyRequest,
};
pub use routes::owner_loyalty_router;
pub use stamps::{
    ConfirmStampRequest, IssuedReward, OrderInput, PolicySummary, PreviewIssue,
    PseudonymousCustomer, ScanRequest, ScanResolution, StampBoard, StampPreview,
    StampPreviewRequest, StampService, StampTransaction, StampTransactionStatus, VoidStampRequest,
};
