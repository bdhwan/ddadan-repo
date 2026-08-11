//! 재시도 분류와 백오프 (§14.7, JOB-003).
//!
//! §14.7 draws one line and it is worth restating, because getting it wrong is expensive
//! in both directions: **retry what the world might fix, give up on what it will not.**
//!
//! * A serialization failure, a lock timeout, a dropped connection — the next attempt has
//!   a real chance, so retry.
//! * A malformed audience, a cancelled campaign, a coupon that does not exist — the next
//!   attempt will fail identically, so retrying only burns a queue slot and delays the
//!   alert somebody needs to see.
//!
//! A provider that sends `429` with `Retry-After` overrides our own schedule: it knows
//! when it will accept traffic again and we do not.

use std::time::Duration;

use crate::error::{ApiError, ErrorCode};
use crate::jobs::RetryBudget;

/// §14.7: 기본 5초부터 2배 지수 증가.
pub const BASE_DELAY: Duration = Duration::from_secs(5);
/// §14.7: 최대 30분.
pub const MAX_DELAY: Duration = Duration::from_secs(30 * 60);
/// §14.7: ±20% jitter, so a thundering herd of retries spreads out.
pub const JITTER_FRACTION: f64 = 0.20;

/// Why an attempt failed, in the only terms the scheduler cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    /// Retrying cannot help. §14.7: validation, authorization, not-found.
    Permanent,
    /// DB serialization, timeout, network — the next attempt may well succeed.
    Transient,
    /// A provider said when to come back. Its answer wins over ours.
    ProviderThrottled { retry_after_secs: u64 },
}

/// What the scheduler decided to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Retry { after: Duration },
    DeadLetter,
}

impl RetryClass {
    /// Combine the class, the attempt number and the job type's budget (§14.6, §14.7).
    pub fn decide(self, attempt_no: i32, budget: RetryBudget) -> RetryDecision {
        match self {
            RetryClass::Permanent => RetryDecision::DeadLetter,

            RetryClass::ProviderThrottled { retry_after_secs } => {
                // Still bounded by the budget: a provider that throttles us forever is a
                // permanent failure wearing a temporary hat.
                if budget.exhausted(attempt_no) {
                    RetryDecision::DeadLetter
                } else {
                    RetryDecision::Retry {
                        after: Duration::from_secs(retry_after_secs).min(MAX_DELAY),
                    }
                }
            }

            RetryClass::Transient => {
                if budget.exhausted(attempt_no) {
                    RetryDecision::DeadLetter
                } else {
                    RetryDecision::Retry {
                        after: backoff_for_attempt(attempt_no),
                    }
                }
            }
        }
    }
}

/// The §14.7 schedule with jitter sampled from the thread RNG.
pub fn backoff_for_attempt(attempt_no: i32) -> Duration {
    backoff_with_jitter(attempt_no, sample_jitter())
}

/// The same schedule with the jitter supplied, so the curve itself is testable.
///
/// `jitter` is a fraction in `[-1.0, 1.0]` scaled by [`JITTER_FRACTION`].
pub fn backoff_with_jitter(attempt_no: i32, jitter: f64) -> Duration {
    let exponent = attempt_no.max(1).saturating_sub(1).min(20);
    let scaled = BASE_DELAY
        .as_secs_f64()
        .mul_add(2f64.powi(exponent), 0.0)
        .min(MAX_DELAY.as_secs_f64());

    let factor = 1.0 + jitter.clamp(-1.0, 1.0) * JITTER_FRACTION;
    // Never below a second: a "retry immediately" loop against a struggling dependency is
    // how a transient fault becomes an outage.
    Duration::from_secs_f64((scaled * factor).clamp(1.0, MAX_DELAY.as_secs_f64()))
}

fn sample_jitter() -> f64 {
    use rand::Rng;
    rand::thread_rng().gen_range(-1.0..=1.0)
}

/// Classify one of our own [`ApiError`]s (§14.7).
///
/// Deliberately keyed on the error *code* rather than on the HTTP status: two 409s can
/// mean opposite things, and "somebody else got there first" is retryable while "this
/// campaign was cancelled" is not.
pub fn classify_api_error(error: &ApiError) -> RetryClass {
    match error.code {
        // Retrying cannot make a bad request good, cannot grant a permission, and cannot
        // conjure a row that does not exist.
        ErrorCode::InvalidRequest
        | ErrorCode::ValidationFailed
        | ErrorCode::IdempotencyKeyRequired
        | ErrorCode::IdempotencyKeyInvalid
        | ErrorCode::InvalidCursor
        | ErrorCode::InvalidVersion
        | ErrorCode::Unauthenticated
        | ErrorCode::TokenExpired
        | ErrorCode::TokenInvalid
        | ErrorCode::Forbidden
        | ErrorCode::RoleRequired
        | ErrorCode::AccountSuspended
        | ErrorCode::AccountWithdrawn
        | ErrorCode::ConsentRequired
        | ErrorCode::ReauthenticationRequired
        | ErrorCode::OriginNotAllowed
        | ErrorCode::StoreNotActive
        | ErrorCode::NotFound
        | ErrorCode::StoreNotFound
        | ErrorCode::UserNotFound
        | ErrorCode::CatalogItemNotFound
        | ErrorCode::LoyaltyPolicyNotFound
        | ErrorCode::TransactionNotFound
        | ErrorCode::CouponNotFound
        | ErrorCode::CampaignNotFound
        | ErrorCode::ReservationNotFound
        // Business refusals. The campaign really is cancelled; the quantity really is gone.
        | ErrorCode::UnprocessableRequest
        | ErrorCode::StoreNotReadyForReview
        | ErrorCode::InvalidStateTransition
        | ErrorCode::QrTokenInvalid
        | ErrorCode::QrTokenExpired
        | ErrorCode::NoActivePolicy
        | ErrorCode::PolicyNotEditable
        | ErrorCode::MinimumOrderNotMet
        | ErrorCode::ItemNotEligible
        | ErrorCode::DailyLimitExceeded
        | ErrorCode::VoidWindowExpired
        | ErrorCode::RequiresAdminReview
        | ErrorCode::CampaignNotIssuing
        | ErrorCode::CampaignPaused
        | ErrorCode::CampaignSoldOut
        | ErrorCode::CampaignNotEditable
        | ErrorCode::QuantityBelowIssued
        | ErrorCode::CouponNotAvailable
        | ErrorCode::CouponNotYetUsable
        | ErrorCode::CouponExpired
        | ErrorCode::CouponOutsideUsageWindow
        | ErrorCode::OrderAlreadyDiscounted
        | ErrorCode::AudienceNotEligible
        | ErrorCode::ApprovalSeparationRequired
        // Phase 4. §14.6 names 수신 거부·템플릿 거절 and 법적 hold 활성 as the permanent
        // failures for notification sending and erasure; the rest are the same shape as
        // the refusals above — a retry reproduces them exactly.
        | ErrorCode::WebhookSignatureInvalid
        | ErrorCode::NotificationNotFound
        | ErrorCode::CaseNotFound
        | ErrorCode::RetentionPolicyNotFound
        | ErrorCode::CaseReferenceRequired
        | ErrorCode::LegalHoldActive
        | ErrorCode::CohortTooSmall => RetryClass::Permanent,

        ErrorCode::RateLimited => RetryClass::ProviderThrottled { retry_after_secs: 60 },

        // Contention and infrastructure. All of these can resolve on their own.
        // A live sanction is somebody else having got there first, which is contention.
        ErrorCode::SanctionAlreadyActive
        | ErrorCode::Conflict
        | ErrorCode::VersionConflict
        | ErrorCode::IdempotencyKeyReused
        | ErrorCode::IdempotencyRequestInProgress
        | ErrorCode::StoreAlreadyExists
        | ErrorCode::StoreSlugTaken
        | ErrorCode::ReviewAlreadyPending
        | ErrorCode::QrAlreadyUsed
        | ErrorCode::DuplicateTransactionSuspected
        | ErrorCode::PolicyAlreadyScheduled
        | ErrorCode::PreviewExpired
        | ErrorCode::ReservationExpired
        | ErrorCode::ReservationAlreadyActive
        | ErrorCode::ServiceUnavailable
        | ErrorCode::DependencyUnavailable => RetryClass::Transient,
    }
}

/// Classify a raw database error, for the paths that never became an [`ApiError`].
pub fn classify_sqlx_error(error: &sqlx::Error) -> RetryClass {
    match error {
        sqlx::Error::RowNotFound => RetryClass::Permanent,
        sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::Tls(_) => {
            RetryClass::Transient
        }
        sqlx::Error::Database(db) => match db.code().as_deref() {
            // 40001 serialization_failure, 40P01 deadlock_detected, 55P03 lock_not_available,
            // 57014 query_canceled, 53300 too_many_connections, 08006 connection_failure.
            Some("40001") | Some("40P01") | Some("55P03") | Some("57014") | Some("53300")
            | Some("08006") | Some("08000") | Some("08003") => RetryClass::Transient,
            // 23xxx integrity violations mean the data says no, and it will keep saying no.
            _ => RetryClass::Permanent,
        },
        _ => RetryClass::Transient,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_doubles_from_five_seconds() {
        // §14.7: 5초부터 2배 지수 증가.
        assert_eq!(backoff_with_jitter(1, 0.0), Duration::from_secs(5));
        assert_eq!(backoff_with_jitter(2, 0.0), Duration::from_secs(10));
        assert_eq!(backoff_with_jitter(3, 0.0), Duration::from_secs(20));
        assert_eq!(backoff_with_jitter(4, 0.0), Duration::from_secs(40));
    }

    #[test]
    fn the_backoff_stops_at_thirty_minutes() {
        // §14.7: 최대 30분.
        for attempt in 10..40 {
            assert!(
                backoff_with_jitter(attempt, 0.0) <= MAX_DELAY,
                "attempt {attempt} exceeded the ceiling"
            );
        }
        assert_eq!(backoff_with_jitter(30, 0.0), MAX_DELAY);
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        // §14.7: ±20% jitter.
        let base = backoff_with_jitter(3, 0.0).as_secs_f64();

        assert!((backoff_with_jitter(3, 1.0).as_secs_f64() - base * 1.2).abs() < 0.001);
        assert!((backoff_with_jitter(3, -1.0).as_secs_f64() - base * 0.8).abs() < 0.001);

        for _ in 0..200 {
            let sampled = backoff_for_attempt(3).as_secs_f64();
            assert!(
                sampled >= base * 0.8 - 0.001 && sampled <= base * 1.2 + 0.001,
                "{sampled} is outside ±20% of {base}"
            );
        }
    }

    #[test]
    fn the_jittered_ceiling_is_still_a_ceiling() {
        assert!(backoff_with_jitter(30, 1.0) <= MAX_DELAY);
    }

    #[test]
    fn a_zeroth_attempt_is_treated_as_the_first() {
        assert_eq!(backoff_with_jitter(0, 0.0), backoff_with_jitter(1, 0.0));
        assert_eq!(backoff_with_jitter(-5, 0.0), backoff_with_jitter(1, 0.0));
    }

    #[test]
    fn validation_authorization_and_not_found_are_never_retried() {
        // §14.7, named exactly.
        for code in [
            ErrorCode::ValidationFailed,
            ErrorCode::InvalidRequest,
            ErrorCode::Forbidden,
            ErrorCode::RoleRequired,
            ErrorCode::NotFound,
            ErrorCode::CouponNotFound,
            ErrorCode::CampaignNotFound,
        ] {
            assert_eq!(
                classify_api_error(&ApiError::new(code)),
                RetryClass::Permanent,
                "{} must not be retried",
                code.as_str()
            );
        }
    }

    #[test]
    fn contention_and_infrastructure_are_retried() {
        for code in [
            ErrorCode::VersionConflict,
            ErrorCode::Conflict,
            ErrorCode::ServiceUnavailable,
            ErrorCode::DependencyUnavailable,
        ] {
            assert_eq!(
                classify_api_error(&ApiError::new(code)),
                RetryClass::Transient,
                "{} should be retried",
                code.as_str()
            );
        }
    }

    #[test]
    fn a_cancelled_campaign_is_a_permanent_failure() {
        // §14.6's 영구 실패 예 for 쿠폰 대량 발급 is literally "캠페인 취소".
        assert_eq!(
            classify_api_error(&ApiError::new(ErrorCode::CampaignNotIssuing)),
            RetryClass::Permanent
        );
    }

    #[test]
    fn rate_limiting_defers_to_the_provider() {
        assert_eq!(
            classify_api_error(&ApiError::new(ErrorCode::RateLimited)),
            RetryClass::ProviderThrottled { retry_after_secs: 60 }
        );

        // §14.7: provider 429/Retry-After 는 제공자 값을 우선한다.
        let decision = RetryClass::ProviderThrottled { retry_after_secs: 90 }
            .decide(1, RetryBudget::Limited(5));
        assert_eq!(
            decision,
            RetryDecision::Retry {
                after: Duration::from_secs(90)
            },
            "the provider's own delay wins over our exponential schedule"
        );
    }

    #[test]
    fn a_provider_delay_is_still_capped() {
        let decision = RetryClass::ProviderThrottled {
            retry_after_secs: 86_400,
        }
        .decide(1, RetryBudget::Limited(5));
        assert_eq!(decision, RetryDecision::Retry { after: MAX_DELAY });
    }

    #[test]
    fn a_permanent_failure_dead_letters_on_the_first_attempt() {
        assert_eq!(
            RetryClass::Permanent.decide(1, RetryBudget::Limited(10)),
            RetryDecision::DeadLetter
        );
    }

    #[test]
    fn a_transient_failure_dead_letters_only_once_the_budget_is_spent() {
        let budget = RetryBudget::Limited(3);
        assert!(matches!(
            RetryClass::Transient.decide(1, budget),
            RetryDecision::Retry { .. }
        ));
        assert!(matches!(
            RetryClass::Transient.decide(2, budget),
            RetryDecision::Retry { .. }
        ));
        assert_eq!(
            RetryClass::Transient.decide(3, budget),
            RetryDecision::DeadLetter
        );
    }

    #[test]
    fn an_unlimited_budget_keeps_retrying() {
        // §14.6: 만료 처리는 무제한 지연 재시도.
        let budget = RetryBudget::UnlimitedDelayed { alert_after: 20 };
        assert!(matches!(
            RetryClass::Transient.decide(500, budget),
            RetryDecision::Retry { .. }
        ));
        // But a permanent failure still stops: forever-retrying a schema mismatch helps
        // nobody, and §14.6 lists exactly that as expiry's 영구 실패 예.
        assert_eq!(
            RetryClass::Permanent.decide(1, budget),
            RetryDecision::DeadLetter
        );
    }

    #[test]
    fn database_contention_is_retried_and_integrity_violations_are_not() {
        assert_eq!(
            classify_sqlx_error(&sqlx::Error::RowNotFound),
            RetryClass::Permanent
        );
        assert_eq!(
            classify_sqlx_error(&sqlx::Error::PoolTimedOut),
            RetryClass::Transient
        );
    }
}
