//! §18.1 SLO 를 상시 측정 가능한 형태로 붙인다.
//!
//! This is not a benchmark. It answers one question — *is an SLO obviously violated?* — and
//! it answers it every time the suite runs, which is the property a benchmark nobody runs
//! does not have.
//!
//! **What the numbers are and are not.** Requests go through the real router, the real
//! handlers and a real PostgreSQL, but `tower::ServiceExt::oneshot` on a local machine:
//! there is no network hop, no TLS, no load balancer, no concurrent tenant, and the
//! database is on the same host with a warm cache. So a passing run is a **lower bound** —
//! it proves the code path itself is nowhere near the budget, and it cannot prove the
//! deployed system meets it. §18.1's numbers still need a staging measurement over the wire
//! before they can be claimed (§20.3). What this catches is the other direction: an N+1
//! query or a missing index that eats the budget before the network even gets a turn.
//!
//! Run with the numbers visible:
//!
//! ```sh
//! ./scripts/coupon/test.sh --test load -- --nocapture
//! ```

mod common;

use std::time::{Duration, Instant};

use common::*;
use serde_json::json;

/// Samples per measurement. Enough for a p95 to mean something, small enough that the
//  suite stays a suite.
const SAMPLES: usize = 100;

/// Requests discarded before measuring: the first calls through a path pay for connection
/// acquisition and statement preparation, which is a startup cost and not a latency.
const WARMUP: usize = 10;

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Latencies {
    label: &'static str,
    samples: Vec<Duration>,
}

impl Latencies {
    fn percentile(&self, fraction: f64) -> Duration {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        // Nearest-rank: the smallest value at or above `fraction` of the sample.
        let rank = ((fraction * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
        sorted[rank - 1]
    }

    fn report(&self, budget: Duration) {
        let p50 = self.percentile(0.50);
        let p95 = self.percentile(0.95);
        let max = self.samples.iter().max().copied().unwrap_or_default();

        eprintln!(
            "§18.1 {:<28} n={:<4} p50={:>7.1}ms p95={:>7.1}ms max={:>7.1}ms  budget p95 ≤ {}ms",
            self.label,
            self.samples.len(),
            p50.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            max.as_secs_f64() * 1000.0,
            budget.as_millis(),
        );

        assert!(
            p95 <= budget,
            "{}: p95 {:.1}ms is over the §18.1 budget of {}ms — and this is measured \
             in-process, so the deployed number can only be worse",
            self.label,
            p95.as_secs_f64() * 1000.0,
            budget.as_millis(),
        );
    }
}

/// Run `once` `WARMUP + SAMPLES` times, timing the samples after the warm-up.
async fn measure<F, Fut>(label: &'static str, mut once: F) -> Latencies
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut samples = Vec::with_capacity(SAMPLES);

    for index in 0..(WARMUP + SAMPLES) {
        let started = Instant::now();
        once(index).await;
        if index >= WARMUP {
            samples.push(started.elapsed());
        }
    }

    Latencies { label, samples }
}

// ---------------------------------------------------------------------------
// 지갑 조회 p95 500ms 이하
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_wallet_read_stays_inside_its_budget() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "load-wallet").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 1_000 })),
    )
    .await;

    // A wallet with something in it. An empty wallet measures the router, not the query.
    let customer = consumer(&harness.app, "load-wallet-customer").await;
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &customer.uid,
        None,
    )
    .await
    .expect_ok("claim a coupon");
    earn_a_stamp(&harness, &shop, &customer.uid).await;

    let coupons = measure("wallet: coupons", |_| {
        let app = harness.app.clone();
        let uid = customer.uid.clone();
        async move {
            send(&app, "GET", "/api/coupon/v1/me/wallet/coupons", &uid, None)
                .await
                .expect_ok("wallet coupons");
        }
    })
    .await;
    coupons.report(Duration::from_millis(500));

    let stamps = measure("wallet: stamps", |_| {
        let app = harness.app.clone();
        let uid = customer.uid.clone();
        async move {
            send(&app, "GET", "/api/coupon/v1/me/wallet/stamps", &uid, None)
                .await
                .expect_ok("wallet stamps");
        }
    })
    .await;
    stamps.report(Duration::from_millis(500));
}

// ---------------------------------------------------------------------------
// 적립/사용 승인 p95 800ms 이하 (외부 알림 제외)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_accrual_approval_stays_inside_its_budget() {
    // "외부 알림 제외" is structural rather than a stopwatch trick: §14.2 commits an outbox
    // row and returns, and the notification happens in the relay afterwards. So what is
    // timed here is already the figure §18.1 asks for.
    let harness = harness_or_skip!();
    let shop = store(&harness, "load-accrual").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;

    // One customer per accrual: §8.6's near-duplicate check would otherwise refuse the
    // second identical order and we would be timing a rejection.
    let mut customers = Vec::with_capacity(WARMUP + SAMPLES);
    for index in 0..(WARMUP + SAMPLES) {
        customers.push(consumer(&harness.app, &format!("load-accrual-{index}")).await);
    }

    // The QR is issued outside the measurement: §18.1's budget is for the approval, and
    // the customer's phone made the QR before the owner reached for the scanner.
    let mut tokens = Vec::with_capacity(customers.len());
    for customer in &customers {
        tokens.push(issue_qr(&harness.app, &customer.uid).await.0);
    }

    let accruals = measure("accrual: approve", |index| {
        let app = harness.app.clone();
        let owner_uid = shop.owner_uid.clone();
        let token = tokens[index].clone();
        async move {
            send(
                &app,
                "POST",
                "/api/coupon/v1/owner/stamp-transactions",
                &owner_uid,
                Some(json!({ "qr_token": token, "order": order(12_000) })),
            )
            .await
            .expect_ok("approve an accrual");
        }
    })
    .await;
    accruals.report(Duration::from_millis(800));
}

#[tokio::test]
async fn a_redemption_approval_stays_inside_its_budget() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "load-redeem").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 1_000 })),
    )
    .await;

    // Each approval needs its own coupon, its own holder and its own live reservation:
    // §12.6-6 allows exactly one of each, so they cannot be shared across samples.
    let mut reservations = Vec::with_capacity(WARMUP + SAMPLES);
    for index in 0..(WARMUP + SAMPLES) {
        let holder = consumer(&harness.app, &format!("load-redeem-{index}")).await;
        let claim = send(
            &harness.app,
            "POST",
            &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
            &holder.uid,
            None,
        )
        .await;
        let coupon_id = claim.expect_ok("claim")["coupon_id"].clone();

        let (token, _) = issue_qr(&harness.app, &holder.uid).await;
        let session = format!("load-till-{index}");
        let reserved = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/redemptions/preview",
            &shop.owner_uid,
            Some(json!({
                "qr_token": token,
                "coupon_id": coupon_id,
                "owner_session_id": session,
                "order": order(12_000),
            })),
        )
        .await;
        let reservation_id = reserved.expect_ok("reserve")["reservation_id"]
            .as_str()
            .expect("reservation id")
            .to_owned();
        reservations.push((reservation_id, session));
    }

    let approvals = measure("redemption: approve", |index| {
        let app = harness.app.clone();
        let owner_uid = shop.owner_uid.clone();
        let (reservation_id, session) = reservations[index].clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/confirm"),
                &owner_uid,
                Some(json!({
                    "owner_session_id": session,
                    "order": order(12_000),
                })),
            )
            .await
            .expect_ok("approve a use");
        }
    })
    .await;
    approvals.report(Duration::from_millis(800));
}

// ---------------------------------------------------------------------------
// 선착순 발급 p95 800ms 이하
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_first_come_claim_stays_inside_its_budget() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "load-claim").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 1_000 })),
    )
    .await;

    // A fresh customer per sample: a repeat claim short-circuits to the coupon they
    // already hold, which is a different and much cheaper path.
    let mut claimers = Vec::with_capacity(WARMUP + SAMPLES);
    for index in 0..(WARMUP + SAMPLES) {
        claimers.push(consumer(&harness.app, &format!("load-claim-{index}")).await);
    }

    let claims = measure("campaign: first-come claim", |index| {
        let app = harness.app.clone();
        let uid = claimers[index].uid.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
                &uid,
                None,
            )
            .await
            .expect_ok("claim");
        }
    })
    .await;
    claims.report(Duration::from_millis(800));
}

#[tokio::test]
async fn the_last_coupon_under_contention_still_answers_inside_the_budget() {
    // The p95 above is a quiet queue. §18.1's budget has to survive the moment the campaign
    // is actually interesting: everyone pressing at once on the last few coupons, where
    // every request queues behind the same counter row lock (§13.2).
    let harness = harness_or_skip!();
    let shop = store(&harness, "load-contended").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;

    // `LOAD_CONTENDERS=128 ./scripts/coupon/test.sh --test load -- --nocapture` re-runs this
    // at another width, which is how the shape of the curve gets checked rather than a
    // single point on it. Measured locally: 16 → p95 117ms, 64 → 99ms, 128 → 141ms, so the
    // cost is not growing with the queue and there is no convoy on the counter row.
    let contenders: usize = std::env::var("LOAD_CONTENDERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(64);
    let mut claimers = Vec::with_capacity(contenders);
    for index in 0..contenders {
        claimers.push(consumer(&harness.app, &format!("load-contended-{index}")).await);
    }

    // Warm the pool to the width of the burst first.
    //
    // `sqlx` grows a pool lazily, so the first N *simultaneous* requests each pay for a
    // fresh PostgreSQL connection — measured here at roughly 450ms for the batch, which
    // swamps the thing under test and would be read as a lock convoy that is not there. A
    // deployed instance has an established pool by the time a campaign opens; a burst of
    // cheap reads gives this one the same footing.
    futures_join(claimers.iter().map(|claimer| {
        let app = harness.app.clone();
        let uid = claimer.uid.clone();
        async move {
            send(&app, "GET", "/api/coupon/v1/me/wallet/coupons", &uid, None)
                .await
                .expect_ok("warm the pool");
        }
    }))
    .await;

    let started = Instant::now();
    let outcomes = futures_join(claimers.iter().map(|claimer| {
        let app = harness.app.clone();
        let uid = claimer.uid.clone();
        async move {
            let at = Instant::now();
            let response = send(
                &app,
                "POST",
                &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
                &uid,
                None,
            )
            .await;
            (at.elapsed(), response.status.is_success())
        }
    }))
    .await;
    let wall_clock = started.elapsed();

    let winners = outcomes.iter().filter(|(_, won)| *won).count();
    assert_eq!(winners, 5, "the stock is the stock, even under contention");

    let contended = Latencies {
        label: "campaign: N at once on 5",
        samples: outcomes.iter().map(|(elapsed, _)| *elapsed).collect(),
    };
    eprintln!(
        "§18.1 {:<28} wall clock for all {contenders}: {:.1}ms",
        "",
        wall_clock.as_secs_f64() * 1000.0,
    );
    contended.report(Duration::from_millis(800));
}
