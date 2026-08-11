//! §18.4's alert inputs, as numbers.
//!
//! §18.4 lists what is worth paging about; this turns each of those into something a
//! monitor can read. Two kinds of signal live here and they are gathered differently:
//!
//! * **Process counters** — request volume, error rate, latency percentiles, invariant
//!   violations. These are in-memory, updated by the request middleware, and reset when the
//!   process does. That is correct for a rate: an aggregator scrapes several instances and
//!   adds them up.
//! * **Database gauges** — queue backlog, unpublished outbox age, dead letters, provider
//!   failure rate. These are facts about the *system*, not about this process, so every
//!   instance reports the same value and a scrape from any one of them is the truth.
//!
//! The latency figures are a coarse bucketed histogram rather than a reservoir. §18.1's SLO
//! is stated as p95 thresholds — 500ms for the wallet, 800ms for accrual and issuance — and
//! a bucket boundary either side of each threshold answers "are we over?" exactly, which is
//! the question being asked. Interpolating a percentile from a sample would be more precise
//! about a number nobody acts on.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::ApiResult;
use crate::state::AppState;

/// Upper bounds in milliseconds. Chosen around §18.1's thresholds: 500 and 800 are SLO
/// boundaries, the rest give the curve enough shape to see a regression coming.
const LATENCY_BUCKETS_MS: [u64; 8] = [50, 100, 250, 500, 800, 1_500, 5_000, u64::MAX];

/// Process-wide counters, updated on the request path.
///
/// Relaxed ordering throughout: these are statistics, and paying for coherence between two
/// counters that are read seconds apart by a scraper would be paying for nothing.
#[derive(Debug, Default)]
pub struct MetricsRegistry {
    requests: AtomicU64,
    client_errors: AtomicU64,
    server_errors: AtomicU64,
    /// §18.4: 중복 불변식 위반 또는 unique conflict 급증. Incremented wherever a domain
    /// uniqueness constraint refuses a write that the application believed was new.
    invariant_violations: AtomicU64,
    latency_buckets: [AtomicU64; LATENCY_BUCKETS_MS.len()],
    latency_total_ms: AtomicU64,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one finished request.
    pub fn observe_request(&self, status: u16, elapsed: Duration) {
        self.requests.fetch_add(1, Ordering::Relaxed);

        match status {
            400..=499 => {
                self.client_errors.fetch_add(1, Ordering::Relaxed);
            }
            500..=599 => {
                self.server_errors.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        let millis = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        self.latency_total_ms.fetch_add(millis, Ordering::Relaxed);

        let index = LATENCY_BUCKETS_MS
            .iter()
            .position(|bound| millis <= *bound)
            .unwrap_or(LATENCY_BUCKETS_MS.len() - 1);
        self.latency_buckets[index].fetch_add(1, Ordering::Relaxed);
    }

    /// §18.4: a uniqueness constraint caught something the application thought was new.
    ///
    /// This is not the same as "a user retried". It is counted where a *duplicate logical
    /// transaction* was prevented by the database rather than by the code above it, which
    /// §18.1 wants to see at zero.
    pub fn record_invariant_violation(&self) {
        self.invariant_violations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn error_rate(&self) -> f64 {
        let total = self.requests() as f64;
        if total == 0.0 {
            return 0.0;
        }
        self.server_errors.load(Ordering::Relaxed) as f64 / total
    }

    /// The bucket boundary at or above which 95% of requests fell.
    ///
    /// Reported as the *bucket bound*, not an interpolation: "p95 is under 800ms" is the
    /// claim §18.1 makes, and a bucket answers it without pretending to a precision the
    /// data does not carry. Returns `None` before any request has been served.
    pub fn latency_p95_ms(&self) -> Option<u64> {
        let counts: Vec<u64> = self
            .latency_buckets
            .iter()
            .map(|bucket| bucket.load(Ordering::Relaxed))
            .collect();
        percentile_bucket(&counts, 95)
    }

    pub fn snapshot(&self) -> ProcessMetrics {
        ProcessMetrics {
            requests: self.requests(),
            client_errors: self.client_errors.load(Ordering::Relaxed),
            server_errors: self.server_errors.load(Ordering::Relaxed),
            error_rate: self.error_rate(),
            latency_p95_ms: self.latency_p95_ms(),
            invariant_violations: self.invariant_violations.load(Ordering::Relaxed),
        }
    }
}

/// The percentile bucket bound, given per-bucket counts.
///
/// Split out from the registry so the arithmetic is testable without atomics.
pub fn percentile_bucket(counts: &[u64], percentile: u64) -> Option<u64> {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return None;
    }

    // Ceiling division: with 20 requests the 95th percentile is the 19th, not the 19.0th.
    let target = total.saturating_mul(percentile).div_ceil(100).max(1);
    let mut seen = 0u64;

    for (index, count) in counts.iter().enumerate() {
        seen += count;
        if seen >= target {
            return Some(LATENCY_BUCKETS_MS[index.min(LATENCY_BUCKETS_MS.len() - 1)]);
        }
    }

    LATENCY_BUCKETS_MS.last().copied()
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProcessMetrics {
    pub requests: u64,
    pub client_errors: u64,
    pub server_errors: u64,
    /// 5xx as a fraction of all requests (§18.4 오류율).
    pub error_rate: f64,
    /// `null` until the process has served a request.
    pub latency_p95_ms: Option<u64>,
    pub invariant_violations: u64,
}

/// Everything §18.4 names, in one response.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OperationalMetrics {
    pub process: ProcessMetrics,
    pub queues: QueueMetrics,
    pub notifications: NotificationMetrics,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QueueMetrics {
    /// Jobs waiting or running, by §14.4 status.
    pub campaign_backlog: i64,
    /// §18.4: outbox unpublished age. Seconds since the oldest unpublished row was written;
    /// zero when the outbox is empty. A rising value is JOB-005 in progress.
    pub outbox_unpublished_age_secs: i64,
    pub outbox_unpublished_count: i64,
    /// §18.4: dead-letter 신규 발생, over the last hour.
    pub dead_letters_last_hour: i64,
    pub dead_letters_total: i64,
    /// Jobs that have been `RUNNING` past their visibility timeout — a worker died, or is
    /// wedged.
    pub stalled_jobs: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationMetrics {
    /// §18.4: notification backlog.
    pub pending_deliveries: i64,
    pub retrying_deliveries: i64,
    /// Seconds the oldest due delivery has been waiting.
    pub oldest_pending_age_secs: i64,
    /// §18.4: FCM/알림톡 provider 실패율, over the last hour.
    pub provider_failure_rate_1h: f64,
    pub permanent_failures_1h: i64,
    pub delivered_1h: i64,
    /// Suppressions are not failures — they are the consent machinery working — but a
    /// sudden rise means a consent projection broke, so they are reported separately rather
    /// than folded into either number.
    pub suppressed_1h: i64,
}

/// Read every §18.4 signal.
pub async fn collect(state: &AppState) -> ApiResult<OperationalMetrics> {
    let queues = sqlx::query!(
        r#"
        SELECT
            (SELECT COUNT(*) FROM coupon.job_registry
             WHERE job_type IN ('issue_campaign', 'build_campaign_audience', 'revoke_campaign')
               AND status IN ('PENDING_OUTBOX', 'QUEUED', 'RUNNING', 'RETRY_WAIT'))
                AS "campaign_backlog!",
            COALESCE((
                SELECT EXTRACT(EPOCH FROM (clock_timestamp() - MIN(created_at)))::bigint
                FROM coupon.outbox_events WHERE status IN ('PENDING', 'FAILED')
            ), 0) AS "outbox_age!",
            (SELECT COUNT(*) FROM coupon.outbox_events WHERE status IN ('PENDING', 'FAILED'))
                AS "outbox_count!",
            (SELECT COUNT(*) FROM coupon.job_registry
             WHERE status = 'DEAD_LETTER'
               AND dead_lettered_at > clock_timestamp() - interval '1 hour')
                AS "dead_letters_hour!",
            (SELECT COUNT(*) FROM coupon.job_registry WHERE status = 'DEAD_LETTER')
                AS "dead_letters_total!",
            (SELECT COUNT(*) FROM coupon.job_registry
             WHERE status = 'RUNNING'
               AND heartbeat_at < clock_timestamp() - (visibility_timeout_secs || ' seconds')::interval)
                AS "stalled!"
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    let notifications = sqlx::query!(
        r#"
        SELECT
            (SELECT COUNT(*) FROM coupon.notification_deliveries WHERE status = 'PENDING')
                AS "pending!",
            (SELECT COUNT(*) FROM coupon.notification_deliveries
             WHERE status = 'FAILED_RETRYABLE') AS "retrying!",
            COALESCE((
                SELECT EXTRACT(EPOCH FROM (
                    clock_timestamp() - MIN(COALESCE(next_attempt_at, scheduled_at))
                ))::bigint
                FROM coupon.notification_deliveries
                WHERE status IN ('PENDING', 'FAILED_RETRYABLE')
                  AND COALESCE(next_attempt_at, scheduled_at) <= clock_timestamp()
            ), 0) AS "oldest_pending_age!",
            (SELECT COUNT(*) FROM coupon.notification_deliveries
             WHERE status = 'FAILED_PERMANENT' AND updated_at > clock_timestamp() - interval '1 hour')
                AS "failed_hour!",
            (SELECT COUNT(*) FROM coupon.notification_deliveries
             WHERE status = 'DELIVERED' AND channel <> 'IN_APP'
               AND updated_at > clock_timestamp() - interval '1 hour') AS "delivered_hour!",
            (SELECT COUNT(*) FROM coupon.notification_deliveries
             WHERE status = 'SUPPRESSED' AND updated_at > clock_timestamp() - interval '1 hour')
                AS "suppressed_hour!"
        "#,
    )
    .fetch_one(&state.pool)
    .await?;

    let attempted = notifications.failed_hour + notifications.delivered_hour;

    Ok(OperationalMetrics {
        process: state.metrics.snapshot(),
        queues: QueueMetrics {
            campaign_backlog: queues.campaign_backlog,
            outbox_unpublished_age_secs: queues.outbox_age,
            outbox_unpublished_count: queues.outbox_count,
            dead_letters_last_hour: queues.dead_letters_hour,
            dead_letters_total: queues.dead_letters_total,
            stalled_jobs: queues.stalled,
        },
        notifications: NotificationMetrics {
            pending_deliveries: notifications.pending,
            retrying_deliveries: notifications.retrying,
            oldest_pending_age_secs: notifications.oldest_pending_age,
            // Suppressions are excluded from the denominator on purpose: a message we chose
            // not to send is not a provider that failed us, and folding them in would make
            // a consent change look like an outage.
            provider_failure_rate_1h: if attempted == 0 {
                0.0
            } else {
                notifications.failed_hour as f64 / attempted as f64
            },
            permanent_failures_1h: notifications.failed_hour,
            delivered_1h: notifications.delivered_hour,
            suppressed_1h: notifications.suppressed_hour,
        },
    })
}

/// Render the process counters in Prometheus text exposition format.
///
/// Only the process half: the database gauges need a query, and a scrape endpoint that
/// opens a transaction is a scrape endpoint that fails when the database is struggling —
/// exactly when monitoring matters most. `GET /admin/metrics` serves those to an operator
/// who is already authenticated.
pub fn prometheus_text(metrics: &ProcessMetrics) -> String {
    let mut out = String::new();

    out.push_str("# HELP coupon_requests_total Requests served by this process\n");
    out.push_str("# TYPE coupon_requests_total counter\n");
    out.push_str(&format!("coupon_requests_total {}\n", metrics.requests));

    out.push_str("# HELP coupon_request_errors_total Responses by error class\n");
    out.push_str("# TYPE coupon_request_errors_total counter\n");
    out.push_str(&format!(
        "coupon_request_errors_total{{class=\"client\"}} {}\n",
        metrics.client_errors
    ));
    out.push_str(&format!(
        "coupon_request_errors_total{{class=\"server\"}} {}\n",
        metrics.server_errors
    ));

    out.push_str("# HELP coupon_request_latency_p95_ms 95th percentile bucket bound\n");
    out.push_str("# TYPE coupon_request_latency_p95_ms gauge\n");
    out.push_str(&format!(
        "coupon_request_latency_p95_ms {}\n",
        metrics.latency_p95_ms.unwrap_or(0)
    ));

    out.push_str("# HELP coupon_invariant_violations_total Duplicate logical transactions refused by a constraint\n");
    out.push_str("# TYPE coupon_invariant_violations_total counter\n");
    out.push_str(&format!(
        "coupon_invariant_violations_total {}\n",
        metrics.invariant_violations
    ));

    out
}

/// Times every request and records its class (§18.4 오류율·p95).
///
/// Mounted outermost so it measures what the client experiences — including the time spent
/// in authentication, idempotency replay and the timeout layer — rather than only the
/// handler's own work.
pub async fn layer(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    state
        .metrics
        .observe_request(response.status().as_u16(), started.elapsed());
    response
}

/// `GET /metrics` — the process counters, in Prometheus text format.
///
/// Unauthenticated and beside the health probes, for the same reason those are: a scraper
/// is infrastructure, not a user. It exposes counts and latencies only — no identifiers, no
/// per-store figures — so §17.2's rule about not handing out customer data is not in play.
/// The database gauges live behind `GET /admin/metrics` instead.
pub fn metrics_router() -> Router<AppState> {
    Router::new().route("/metrics", get(scrape))
}

async fn scrape(State(state): State<AppState>) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        prometheus_text(&state.metrics.snapshot()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_rate_counts_only_our_own_failures() {
        // §18.4 alerts on 오류율. A client sending malformed JSON is not an outage, and
        // counting it as one would mean an attacker could page the on-call engineer.
        let registry = MetricsRegistry::new();
        registry.observe_request(200, Duration::from_millis(10));
        registry.observe_request(422, Duration::from_millis(10));
        registry.observe_request(500, Duration::from_millis(10));
        registry.observe_request(200, Duration::from_millis(10));

        assert_eq!(registry.requests(), 4);
        assert!((registry.error_rate() - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn the_p95_lands_in_the_bucket_the_slo_is_written_against() {
        // §18.1: 지갑 조회 p95 500ms 이하.
        let registry = MetricsRegistry::new();
        for _ in 0..99 {
            registry.observe_request(200, Duration::from_millis(30));
        }
        assert_eq!(registry.latency_p95_ms(), Some(50));

        // Exactly one slow request in twenty does *not* move it — that request is the
        // 100th percentile, not the 95th — but two do.
        let registry = MetricsRegistry::new();
        for _ in 0..19 {
            registry.observe_request(200, Duration::from_millis(30));
        }
        registry.observe_request(200, Duration::from_millis(1_200));
        assert_eq!(registry.latency_p95_ms(), Some(50));

        let registry = MetricsRegistry::new();
        for _ in 0..18 {
            registry.observe_request(200, Duration::from_millis(30));
        }
        for _ in 0..2 {
            registry.observe_request(200, Duration::from_millis(1_200));
        }
        assert_eq!(registry.latency_p95_ms(), Some(1_500));
    }

    #[test]
    fn a_process_that_has_served_nothing_reports_no_percentile() {
        // Zero would read as "we are very fast" on a dashboard.
        let registry = MetricsRegistry::new();
        assert_eq!(registry.latency_p95_ms(), None);
        assert_eq!(registry.error_rate(), 0.0);
    }

    #[test]
    fn the_percentile_bucket_arithmetic_rounds_up() {
        // With 20 samples the 95th is the 19th: 19 fast and 1 slow still reports fast,
        // and 18 fast with 2 slow does not.
        assert_eq!(percentile_bucket(&[19, 0, 0, 0, 0, 0, 1, 0], 95), Some(50));
        assert_eq!(percentile_bucket(&[18, 0, 0, 0, 0, 0, 2, 0], 95), Some(5_000));
        assert_eq!(percentile_bucket(&[20, 0, 0, 0, 0, 0, 0, 0], 95), Some(50));
        assert_eq!(percentile_bucket(&[0; 8], 95), None);

        // 21 samples: ceil(21 * 0.95) = 20, so one slow request out of 21 is still not the
        // 95th. Rounding *down* here would report the slow bucket and page needlessly.
        assert_eq!(percentile_bucket(&[20, 0, 0, 0, 0, 0, 1, 0], 95), Some(50));
    }

    #[test]
    fn invariant_violations_are_counted_separately_from_errors() {
        // §18.1 wants 중복 논리 거래 at zero, and §18.4 alerts on it. It is not an error
        // rate: the constraint doing its job usually produces a *successful* response.
        let registry = MetricsRegistry::new();
        registry.record_invariant_violation();
        registry.record_invariant_violation();

        let snapshot = registry.snapshot();
        assert_eq!(snapshot.invariant_violations, 2);
        assert_eq!(snapshot.server_errors, 0);
    }

    #[test]
    fn the_exposition_names_every_18_4_process_signal() {
        let registry = MetricsRegistry::new();
        registry.observe_request(200, Duration::from_millis(10));
        let text = prometheus_text(&registry.snapshot());

        for metric in [
            "coupon_requests_total",
            "coupon_request_errors_total",
            "coupon_request_latency_p95_ms",
            "coupon_invariant_violations_total",
        ] {
            assert!(text.contains(metric), "{metric} missing from:\n{text}");
        }
    }
}
