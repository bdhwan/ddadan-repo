//! Per-minute request limits for the abuse-prone endpoints (§16.4).
//!
//! Three properties matter more than sophistication here:
//!
//! * **The limits are operational settings, not constants** — every ceiling comes from
//!   [`Config`], so a busy Saturday can be accommodated without a deploy.
//! * **A limiter outage must not become an API outage.** Redis is the shared counter when
//!   it is available; when it is not, an in-process counter still slows a single node's
//!   attacker down. Neither is allowed to turn a Redis hiccup into a 5xx.
//! * **An IP never bans an account.** §16.4 is explicit about shared IPv4/IPv6 networks,
//!   so an IP only ever appears *alongside* an account in a key, never alone as grounds
//!   for a lasting block.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};

use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::state::RedisHandle;

/// Window length. §16.4 states every stamp-flow limit per minute.
const WINDOW_SECONDS: i64 = 60;

/// The limits from §16.4 that Phase 2 endpoints enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// QR issuance, keyed by user.
    QrIssue,
    /// *Failed* QR resolution, keyed by owner and IP. Successes are not counted: a busy
    /// counter at lunchtime is not an attack.
    QrResolveFailure,
    /// Accrual approval, keyed by store and owner.
    StampApproval,
    /// §16.4 선착순 받기, keyed by user and campaign. SEC-003 wants a bot to hit a wall
    /// here rather than for a legitimate winner to have their coupon taken back later.
    CampaignClaim,
}

impl Bucket {
    pub fn as_str(self) -> &'static str {
        match self {
            Bucket::QrIssue => "qr_issue",
            Bucket::QrResolveFailure => "qr_resolve_failure",
            Bucket::StampApproval => "stamp_approval",
            Bucket::CampaignClaim => "campaign_claim",
        }
    }
}

/// Whether a request that brings the window's count to `count` is allowed.
///
/// `count` is the value *after* this request was counted, so a limit of 20 permits counts
/// 1 through 20 and rejects 21.
pub fn within_limit(count: u64, limit: u32) -> bool {
    // A limit of zero would lock the endpoint out entirely; treating it as "unlimited"
    // makes a blank configuration value fail open rather than take the feature down.
    limit == 0 || count <= u64::from(limit)
}

fn window_index(now: DateTime<Utc>) -> i64 {
    now.timestamp().div_euclid(WINDOW_SECONDS)
}

pub struct RateLimiter {
    redis: Option<RedisHandle>,
    /// Fallback counters. Bounded by pruning every window, so a long-running process
    /// cannot accumulate one entry per key seen since boot.
    local: Mutex<HashMap<String, u64>>,
    local_window: Mutex<i64>,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RateLimiter")
            .field("shared", &self.redis.is_some())
            .finish()
    }
}

impl RateLimiter {
    pub fn new(redis: Option<RedisHandle>) -> Self {
        Self {
            redis,
            local: Mutex::new(HashMap::new()),
            local_window: Mutex::new(0),
        }
    }

    /// Count one request and reject it if the window is already full.
    ///
    /// `key` identifies the subject — a user id, or `store:owner`. It is hashed into the
    /// Redis key as given, so callers must not pass anything they would not want in a
    /// Redis keyspace dump; ids are fine, personal data is not.
    pub async fn check(
        &self,
        bucket: Bucket,
        key: &str,
        limit: u32,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        if limit == 0 {
            return Ok(());
        }

        let window = window_index(now);
        let redis_key = format!("coupon:rl:{}:{}:{window}", bucket.as_str(), key);

        let count = match self.count_in_redis(&redis_key).await {
            Some(count) => count,
            // Redis is transport and cache (§18.2). Losing it degrades the limiter from
            // cluster-wide to per-process; it must not degrade the API.
            None => self.count_locally(&redis_key, window),
        };

        if within_limit(count, limit) {
            Ok(())
        } else {
            tracing::warn!(
                bucket = bucket.as_str(),
                limit,
                count,
                "rate limit exceeded"
            );
            Err(ApiError::new(ErrorCode::RateLimited)
                .internal(format!("{} over {limit}/min", bucket.as_str())))
        }
    }

    async fn count_in_redis(&self, key: &str) -> Option<u64> {
        let handle = self.redis.as_ref()?;
        let mut connection = match handle.client.get_multiplexed_async_connection().await {
            Ok(connection) => connection,
            Err(error) => {
                tracing::warn!(%error, "rate limiter falling back to in-process counters");
                return None;
            }
        };

        let count: u64 = redis::cmd("INCR")
            .arg(key)
            .query_async(&mut connection)
            .await
            .map_err(|error| tracing::warn!(%error, "rate limiter INCR failed"))
            .ok()?;

        if count == 1 {
            // Two windows of slack so a counter incremented at the very end of a window
            // still expires without a sweeper.
            let _: Result<(), _> = redis::cmd("EXPIRE")
                .arg(key)
                .arg(WINDOW_SECONDS * 2)
                .query_async(&mut connection)
                .await;
        }

        Some(count)
    }

    fn count_locally(&self, key: &str, window: i64) -> u64 {
        let mut current_window = self
            .local_window
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut counters = self
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if *current_window != window {
            counters.clear();
            *current_window = window;
        }

        let counter = counters.entry(key.to_owned()).or_insert(0);
        *counter += 1;
        *counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn the_limit_is_inclusive() {
        assert!(within_limit(20, 20), "the twentieth request still fits");
        assert!(!within_limit(21, 20));
        assert!(within_limit(1, 1));
        assert!(!within_limit(2, 1));
    }

    #[test]
    fn a_blank_limit_fails_open_rather_than_closing_the_endpoint() {
        assert!(within_limit(1_000_000, 0));
    }

    #[tokio::test]
    async fn requests_are_refused_once_the_window_is_full() {
        let limiter = RateLimiter::new(None);
        let now = at(1_000_000);

        for attempt in 1..=3 {
            limiter
                .check(Bucket::QrIssue, "user-1", 3, now)
                .await
                .unwrap_or_else(|_| panic!("request {attempt} is within the limit"));
        }

        let error = limiter
            .check(Bucket::QrIssue, "user-1", 3, now)
            .await
            .expect_err("the fourth request is over the limit");
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert_eq!(error.status().as_u16(), 429);
        assert!(error.code.retryable(), "the client should back off and retry");
    }

    #[tokio::test]
    async fn a_new_window_starts_fresh() {
        let limiter = RateLimiter::new(None);
        // Aligned to a window boundary so the second call is unambiguously inside the
        // same minute as the first.
        let window_start = at(1_000_020);

        limiter
            .check(Bucket::QrIssue, "user-1", 1, window_start)
            .await
            .expect("first request");
        limiter
            .check(Bucket::QrIssue, "user-1", 1, window_start + chrono::Duration::seconds(30))
            .await
            .expect_err("same window, over the limit");
        limiter
            .check(Bucket::QrIssue, "user-1", 1, window_start + chrono::Duration::seconds(60))
            .await
            .expect("the next minute is a new window");
    }

    #[tokio::test]
    async fn subjects_and_buckets_are_counted_separately() {
        let limiter = RateLimiter::new(None);
        let now = at(1_000_000);

        limiter
            .check(Bucket::QrIssue, "user-1", 1, now)
            .await
            .expect("first subject");
        limiter
            .check(Bucket::QrIssue, "user-2", 1, now)
            .await
            .expect("a different subject has its own budget");
        limiter
            .check(Bucket::StampApproval, "user-1", 1, now)
            .await
            .expect("a different bucket has its own budget");
    }
}
