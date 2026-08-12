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

/// Window length for the per-minute buckets. §16.4 states every stamp-flow limit per
/// minute; the two authentication limits are stated per ten minutes instead, so the
/// window is a property of the bucket rather than a constant.
const WINDOW_SECONDS: i64 = 60;
const TEN_MINUTE_WINDOW_SECONDS: i64 = 600;

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
    /// §16.4 로그인/가입 시작 10회/10분, keyed by IP. Ten minutes, not one: starting a
    /// login is a human act, and a per-minute ceiling generous enough for a fumbled
    /// retry would be no obstacle at all to a script.
    LoginStart,
    /// §16.4 카카오 callback 실패 20회/10분, keyed by IP and `state` prefix. *Failures*
    /// only — a successful callback is somebody signing in.
    KakaoCallbackFailure,
}

impl Bucket {
    pub fn as_str(self) -> &'static str {
        match self {
            Bucket::QrIssue => "qr_issue",
            Bucket::QrResolveFailure => "qr_resolve_failure",
            Bucket::StampApproval => "stamp_approval",
            Bucket::CampaignClaim => "campaign_claim",
            Bucket::LoginStart => "login_start",
            Bucket::KakaoCallbackFailure => "kakao_callback_failure",
        }
    }

    /// How long this bucket's window is. §16.4 states the two authentication limits per
    /// ten minutes and everything else per minute.
    pub fn window_seconds(self) -> i64 {
        match self {
            Bucket::LoginStart | Bucket::KakaoCallbackFailure => TEN_MINUTE_WINDOW_SECONDS,
            Bucket::QrIssue
            | Bucket::QrResolveFailure
            | Bucket::StampApproval
            | Bucket::CampaignClaim => WINDOW_SECONDS,
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

fn window_index(now: DateTime<Utc>, window_seconds: i64) -> i64 {
    now.timestamp().div_euclid(window_seconds)
}

fn window_end(window: i64, window_seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp((window + 1) * window_seconds, 0).unwrap_or(DateTime::<Utc>::MAX_UTC)
}

pub struct RateLimiter {
    redis: Option<RedisHandle>,
    /// Fallback counters, each stored with the instant its window ends. Expired entries
    /// are dropped on every call, so a long-running process cannot accumulate one entry
    /// per key seen since boot.
    ///
    /// The expiry is per entry rather than one shared "current window" scalar because the
    /// buckets no longer share a window length: a per-minute rollover must not wipe a
    /// ten-minute counter that is only half spent.
    local: Mutex<HashMap<String, LocalCounter>>,
}

#[derive(Debug, Clone, Copy)]
struct LocalCounter {
    count: u64,
    expires_at: DateTime<Utc>,
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

        let window = window_index(now, bucket.window_seconds());
        let redis_key = format!("coupon:rl:{}:{}:{window}", bucket.as_str(), key);

        let count = match self.count_in_redis(&redis_key, bucket.window_seconds()).await {
            Some(count) => count,
            // Redis is transport and cache (§18.2). Losing it degrades the limiter from
            // cluster-wide to per-process; it must not degrade the API.
            None => self.count_locally(&redis_key, window_end(window, bucket.window_seconds()), now),
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
            Err(ApiError::new(ErrorCode::RateLimited).internal(format!(
                "{} over {limit} per {}s",
                bucket.as_str(),
                bucket.window_seconds()
            )))
        }
    }

    async fn count_in_redis(&self, key: &str, window_seconds: i64) -> Option<u64> {
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
                .arg(window_seconds * 2)
                .query_async(&mut connection)
                .await;
        }

        Some(count)
    }

    fn count_locally(&self, key: &str, expires_at: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
        let mut counters = self
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        counters.retain(|_, counter| counter.expires_at > now);

        let counter = counters.entry(key.to_owned()).or_insert(LocalCounter {
            count: 0,
            expires_at,
        });
        counter.count += 1;
        counter.count
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
    async fn a_ten_minute_window_is_not_reset_by_the_minute_buckets_rolling_over() {
        // §16.4 states the two authentication limits per ten minutes. The in-process
        // fallback used to keep one shared "current window" scalar, which meant a
        // per-minute bucket ticking over wiped the ten-minute counters with it — an
        // attacker only had to issue one QR request a minute to reset their callback
        // budget.
        let limiter = RateLimiter::new(None);
        let start = at(1_200_000);

        limiter
            .check(Bucket::KakaoCallbackFailure, "1.2.3.4:abcd", 2, start)
            .await
            .expect("first failure");

        // A minute later: a different bucket, and the minute has rolled over.
        let later = start + chrono::Duration::seconds(90);
        limiter
            .check(Bucket::QrIssue, "user-1", 100, later)
            .await
            .expect("unrelated traffic");

        limiter
            .check(Bucket::KakaoCallbackFailure, "1.2.3.4:abcd", 2, later)
            .await
            .expect("second failure still inside the ten minutes");
        limiter
            .check(Bucket::KakaoCallbackFailure, "1.2.3.4:abcd", 2, later)
            .await
            .expect_err("the third is over the ten-minute limit");

        limiter
            .check(
                Bucket::KakaoCallbackFailure,
                "1.2.3.4:abcd",
                2,
                start + chrono::Duration::seconds(600),
            )
            .await
            .expect("the next ten-minute window is fresh");
    }

    #[test]
    fn each_bucket_declares_the_window_length_the_spec_gives_it() {
        assert_eq!(Bucket::QrIssue.window_seconds(), 60);
        assert_eq!(Bucket::CampaignClaim.window_seconds(), 60);
        assert_eq!(Bucket::LoginStart.window_seconds(), 600);
        assert_eq!(Bucket::KakaoCallbackFailure.window_seconds(), 600);
    }

    #[tokio::test]
    async fn expired_local_counters_do_not_accumulate() {
        let limiter = RateLimiter::new(None);

        for minute in 0..5 {
            limiter
                .check(
                    Bucket::QrIssue,
                    &format!("user-{minute}"),
                    10,
                    at(1_000_000 + minute * 60),
                )
                .await
                .expect("within the limit");
        }

        let counters = limiter.local.lock().expect("counters");
        assert_eq!(
            counters.len(),
            1,
            "only the live window's counter is kept: {counters:?}"
        );
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
