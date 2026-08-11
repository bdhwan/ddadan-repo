//! §17 장애·복구 시나리오와 §18.5 복구 순서.
//!
//! Two claims are under test here, and they are the ones a release either has or does not:
//!
//! 1. **Redis is not the system.** §13.2 says a counter cache is never the source of truth
//!    for a quantity judgement and §18.2 says a Redis outage is a *degraded feature*, not a
//!    failed readiness probe. Both are asserted against a handle pointed at a port nothing
//!    listens on — the same failure a stopped container produces, without stopping one.
//! 2. **The §18.5 recovery order is executable.** 복원 후 outbox 재발행 → 만료 따라잡기 →
//!    deletion ledger 재적용. A runbook step nobody has run is a guess, so the whole
//!    sequence runs here against a state deliberately rolled backwards.

mod common;

use axum::http::StatusCode;
use chrono::Utc;
use common::*;
use serde_json::json;
use uuid::Uuid;

/// A port nothing listens on. `redis://` against it parses, connects lazily, and fails on
/// first use — which is exactly what a Redis outage looks like from inside the process.
const DEAD_REDIS: &str = "redis://127.0.0.1:6399";

// ---------------------------------------------------------------------------
// §18.2: Redis 장애는 degraded 이지 not_ready 가 아니다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_dead_redis_degrades_a_feature_and_not_the_readiness_probe() {
    // §18.2: Redis 장애는 API 전체 readiness 실패가 아니라 기능별 degraded 상태로 노출한다.
    let Some(harness) = harness_with_redis(json!({}), Some(DEAD_REDIS)).await else {
        eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
        return;
    };

    let ready = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/health/ready",
        "unauthenticated",
        None,
    )
    .await;

    assert_eq!(
        ready.status,
        StatusCode::OK,
        "a Redis outage must not take the instance out of the load balancer: {}",
        ready.raw,
    );
    let body: serde_json::Value = serde_json::from_str(&ready.raw).expect("a health body");
    assert_eq!(body["status"], "ready");
    assert_eq!(body["postgres"]["status"], "ok");
    assert_eq!(
        body["redis"]["status"], "degraded",
        "the outage has to be *visible*, not swallowed: {body}",
    );
    assert!(
        !body["redis"]["detail"].is_null(),
        "…and it has to say what went wrong, so the alert is actionable",
    );

    // /health/live touches nothing external on purpose (§18.2): an outage in a dependency
    // must not make the orchestrator restart a healthy process.
    let live = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/health/live",
        "unauthenticated",
        None,
    )
    .await;
    assert_eq!(live.status, StatusCode::OK);
}

#[tokio::test]
async fn an_unconfigured_redis_reads_as_disabled_rather_than_broken() {
    // The third state §18.2 needs: a deployment that simply has no Redis is not in trouble.
    let harness = harness_or_skip!();

    let ready = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/health/ready",
        "unauthenticated",
        None,
    )
    .await;
    let body: serde_json::Value = serde_json::from_str(&ready.raw).expect("a health body");

    assert_eq!(ready.status, StatusCode::OK);
    assert_eq!(body["redis"]["status"], "disabled");
    assert_eq!(
        body["migration_version"], body["expected_migration_version"],
        "readiness includes the migration version the binary expects (§18.2)",
    );
}

#[tokio::test]
async fn losing_postgres_does_fail_readiness() {
    // The contrast that makes the Redis assertions mean something: readiness is not a
    // rubber stamp. §17 — PostgreSQL 쓰기 장애 — must take the instance out of rotation.
    let harness = harness_or_skip!();

    // This harness owns its own pool, so closing it takes down nothing else.
    harness.pool.close().await;

    let ready = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/health/ready",
        "unauthenticated",
        None,
    )
    .await;
    let body: serde_json::Value = serde_json::from_str(&ready.raw).expect("a health body");

    assert_eq!(ready.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["status"], "not_ready");
    assert_eq!(body["postgres"]["status"], "down");
}

#[tokio::test]
async fn the_last_coupon_is_still_the_last_coupon_with_redis_down() {
    // §13.2: 카운터 캐시나 Redis 값은 수량 판정의 source of truth 로 사용하지 않는다.
    // `campaigns.rs` proves the invariant with no Redis configured at all; this proves it
    // with Redis configured and *failing*, which is the case where a cache-first
    // implementation would quietly hand out a second coupon.
    let Some(harness) = harness_with_redis(json!({}), Some(DEAD_REDIS)).await else {
        eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
        return;
    };

    let shop = store(&harness, "redis-down-stock").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 1 })),
    )
    .await;

    let mut claimers = Vec::new();
    for index in 0..40 {
        claimers.push(consumer(&harness.app, &format!("redis-down-{index}")).await);
    }

    let claims = futures_join(claimers.iter().map(|claimer| {
        let app = harness.app.clone();
        let claimer_uid = claimer.uid.clone();
        async move {
            let response = send(
                &app,
                "POST",
                &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
                &claimer_uid,
                None,
            )
            .await;
            (response.status, response.error_code().to_owned())
        }
    }))
    .await;

    let winners = claims
        .iter()
        .filter(|(status, _)| status.is_success())
        .count();
    assert_eq!(
        winners, 1,
        "§12.6-4 does not depend on Redis being up: {claims:?}",
    );
    assert!(
        claims
            .iter()
            .all(|(status, code)| status.is_success() || code == "CAMPAIGN_SOLD_OUT"),
        "and the losers get the reason, not a 5xx from the cache: {claims:?}",
    );

    let issued: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1")
            .bind(campaign_id)
            .fetch_one(&harness.pool)
            .await
            .expect("count");
    assert_eq!(issued, 1);
}

#[tokio::test]
async fn a_rate_limit_survives_its_own_counter_going_away() {
    // JOB-005 / §16.4: 제한기 장애가 API 장애가 되어서는 안 된다. With Redis gone the limiter
    // falls back to a per-process counter — weaker than cluster-wide, and still a wall.
    let Some(harness) = harness_with_redis(
        json!({ "rate_limit_qr_issue_per_min": 3 }),
        Some(DEAD_REDIS),
    )
    .await
    else {
        eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
        return;
    };

    let customer = consumer(&harness.app, "limiter-fallback").await;

    let mut statuses = Vec::new();
    for _ in 0..5 {
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/me/qr-tokens",
            &customer.uid,
            None,
        )
        .await;
        statuses.push(response.status);
    }

    assert!(
        statuses.iter().all(|status| status.is_success()
            || *status == StatusCode::TOO_MANY_REQUESTS),
        "a limiter outage must never surface as a 5xx: {statuses:?}",
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::TOO_MANY_REQUESTS)
            .count(),
        2,
        "the local counter still enforces the ceiling: {statuses:?}",
    );
}

// ---------------------------------------------------------------------------
// §18.5 복구 순서 1: outbox 재발행
// ---------------------------------------------------------------------------

#[tokio::test]
async fn republishing_the_outbox_after_a_restore_does_not_double_up() {
    // §18.5 step 1. A restore rewinds `outbox_events` to a point where rows that had
    // already been delivered are `PENDING` again, so the relay *will* publish them a second
    // time. The notification uniqueness is what makes that safe (NOTIFY-004), and it is
    // asserted here rather than assumed.
    let harness = harness_or_skip!();
    let runtime = harness.runtime();
    let shop = store(&harness, "outbox-replay").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "outbox-replay-customer").await;

    earn_a_stamp(&harness, &shop, &customer.uid).await;

    // Drain the outbox as the running system would.
    for _ in 0..10 {
        runtime.relay().await.expect("relay jobs");
        runtime
            .relay_notifications()
            .await
            .expect("relay notifications");
        if pending_outbox_for(&harness, customer.user_id).await == 0 {
            break;
        }
    }

    let after_first = notification_count(&harness, customer.user_id).await;
    assert!(
        after_first > 0,
        "the accrual produced an in-app notification to begin with",
    );
    let deliveries_after_first = delivery_count(&harness, customer.user_id).await;

    // The restore: every event this customer's accrual produced is pending again.
    let rewound = sqlx::query(
        "UPDATE coupon.outbox_events
         SET status = 'PENDING', published_at = NULL, available_at = clock_timestamp()
         WHERE payload->>'user_id' = $1::text",
    )
    .bind(customer.user_id.to_string())
    .execute(&harness.pool)
    .await
    .expect("rewind the outbox");
    assert!(
        rewound.rows_affected() > 0,
        "the accrual has to have written an outbox row for this test to mean anything",
    );

    for _ in 0..10 {
        runtime
            .relay_notifications()
            .await
            .expect("relay notifications again");
        if pending_outbox_for(&harness, customer.user_id).await == 0 {
            break;
        }
    }

    assert_eq!(
        notification_count(&harness, customer.user_id).await,
        after_first,
        "§18.5: 재발행이 중복 반영되어서는 안 된다",
    );
    assert_eq!(
        delivery_count(&harness, customer.user_id).await,
        deliveries_after_first,
        "…and no second send goes out either",
    );

    // Every rewound row ends up published rather than stuck: a row left PENDING forever
    // makes §18.4's outbox-age alert fire on a healthy system.
    assert_eq!(pending_outbox_for(&harness, customer.user_id).await, 0);
}

// ---------------------------------------------------------------------------
// §18.5 복구 순서 2: 만료 따라잡기
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_sweep_that_has_been_stopped_for_a_month_catches_up_without_overshooting() {
    // §18.5 step 2 / JOB-004: 만료 작업은 늦게 도는 것이 허용된 작업이다. What must hold when
    // it finally runs is that a coupon a month overdue reaches exactly one terminal state,
    // and that running the sweep again after it caught up changes nothing (§12.6-8: 상태
    // 이벤트의 이전 상태는 당시 인스턴스 상태와 같아야 한다).
    let harness = harness_or_skip!();
    let shop = store(&harness, "expiry-catchup").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    let holder = consumer(&harness.app, "expiry-holder").await;

    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &holder.uid,
        None,
    )
    .await;
    let coupon_id = Uuid::parse_str(
        claim.expect_ok("claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    // The world moved on while the worker was down. `ck_coupon_instance_period` keeps
    // `expires_at > usable_from`, so the whole window moves rather than only its end.
    sqlx::query(
        "UPDATE coupon.coupon_instances
         SET usable_from = clock_timestamp() - interval '60 days',
             expires_at = clock_timestamp() - interval '30 days'
         WHERE id = $1",
    )
    .bind(coupon_id)
    .execute(&harness.pool)
    .await
    .expect("backdate the coupon");

    // §18.1: 만료 상태 반영 지연 5분 이하, **온라인 판정은 즉시**. The stored `status` is the
    // record and still says AVAILABLE until the sweep rewrites it; `effective_status` is
    // the judgement, and it must not wait for a worker that has been down for a month.
    let seen_by_the_holder = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/me/wallet/coupons/{coupon_id}"),
        &holder.uid,
        None,
    )
    .await;
    let coupon = seen_by_the_holder.expect_ok("read the coupon");
    assert_eq!(coupon["status"], "AVAILABLE", "the record is untouched");
    assert_eq!(
        coupon["effective_status"], "EXPIRED",
        "an overdue coupon must never *read* as usable, sweep or no sweep: {coupon}",
    );

    // And the judgement is load-bearing, not decorative: the shop cannot take it either.
    let (token, _) = issue_qr(&harness.app, &holder.uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &shop.owner_uid,
        Some(json!({
            "qr_token": token,
            "coupon_id": coupon_id,
            "owner_session_id": "till-catchup",
            "order": order(12_000),
        })),
    )
    .await
    .expect_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "COUPON_EXPIRED",
        "a coupon the sweep has not reached yet is still expired at the till",
    );

    run_the_expiry_sweep(&harness).await;

    let (status, events) = coupon_state(&harness, coupon_id).await;
    assert_eq!(status, "EXPIRED");
    assert_eq!(
        events, 1,
        "one AVAILABLE→EXPIRED event, not one per missed day",
    );

    // Catching up twice is the same as catching up once.
    run_the_expiry_sweep(&harness).await;
    let (status, events) = coupon_state(&harness, coupon_id).await;
    assert_eq!(status, "EXPIRED");
    assert_eq!(events, 1, "§12.6-8: the second sweep has nothing to write");
}

// ---------------------------------------------------------------------------
// §18.5 복구 순서 3: deletion ledger 재적용
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_full_18_5_recovery_sequence_leaves_an_erased_subject_erased() {
    // §18.5: 복원 후 outbox 재발행 → 만료 따라잡기 → deletion ledger 재적용 순서. Running the
    // three in order on one rolled-back state is the closest a test gets to the drill the
    // runbook asks for, and it catches the thing the steps-in-isolation tests cannot: an
    // earlier step writing the subject back after a later step erased them.
    let harness = harness_or_skip!(json!({ "privacy_deletion_grace_days": 0 }));
    let runtime = harness.runtime();
    let desk = admin(&harness, "recovery-admin").await;
    let shop = store(&harness, "recovery").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;

    let subject = consumer(&harness.app, "recovery-subject").await;
    earn_a_stamp(&harness, &shop, &subject.uid).await;

    let case_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/cases",
        &desk.uid,
        Some(json!({
            "case_type": "PRIVACY_REQUEST",
            "title": "파기 요청",
            "description": "본인 삭제 요청",
            "subject_user_id": subject.user_id,
        })),
    )
    .await
    .id("open a case");

    let erasure_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/privacy/erasures",
        &desk.uid,
        Some(json!({
            "subject_user_id": subject.user_id,
            "case_id": case_id,
            "reason": "본인 삭제 요청",
        })),
    )
    .await
    .id("request the erasure");

    harness
        .state
        .privacy
        .execute(&harness.pool, erasure_id)
        .await
        .expect("execute the erasure");

    // ---- The restore. The subject is alive again and the outbox is unpublished. ----
    sqlx::query(
        "UPDATE coupon.users
         SET display_name = '복원된 사람', status = 'ACTIVE', withdrawn_at = NULL,
             tombstoned_at = NULL, pseudonym_label = NULL, firebase_uid = $2
         WHERE id = $1",
    )
    .bind(subject.user_id)
    .bind(format!("restored-{}", Uuid::new_v4().simple()))
    .execute(&harness.pool)
    .await
    .expect("restore the subject");

    sqlx::query(
        "UPDATE coupon.outbox_events
         SET status = 'PENDING', published_at = NULL, available_at = clock_timestamp()
         WHERE payload->>'user_id' = $1::text",
    )
    .bind(subject.user_id.to_string())
    .execute(&harness.pool)
    .await
    .expect("rewind the outbox");

    // ---- Step 1: outbox 재발행 ----
    for _ in 0..10 {
        runtime.relay().await.expect("relay jobs");
        runtime
            .relay_notifications()
            .await
            .expect("relay notifications");
        if pending_outbox_for(&harness, subject.user_id).await == 0 {
            break;
        }
    }

    // ---- Step 2: 만료 따라잡기 ----
    run_the_expiry_sweep(&harness).await;

    // ---- Step 3: deletion ledger 재적용 ----
    let result = harness
        .state
        .privacy
        .reapply(&harness.pool)
        .await
        .expect("reapply the deletion ledger");
    assert!(
        result.reapplied >= 1,
        "the restore brought at least one erased subject back: {result:?}",
    );

    let (display_name, status, tombstoned): (String, String, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT display_name, status::text, tombstoned_at FROM coupon.users WHERE id = $1",
        )
        .bind(subject.user_id)
        .fetch_one(&harness.pool)
        .await
        .expect("read the subject");

    assert_ne!(
        display_name, "복원된 사람",
        "§18.5: 복원 시 삭제 tombstone 을 재적용한다",
    );
    assert_eq!(status, "WITHDRAWN");
    assert!(tombstoned.is_some());

    // §17.3: the ledger the erasure had to leave standing is still standing.
    let transactions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_transactions WHERE store_id = $1",
    )
    .bind(shop.id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(
        transactions, 1,
        "erasing a person must not erase the shop's books",
    );

    // Running the sequence a second time is a no-op, which is what makes it safe to put in
    // a runbook somebody will follow under pressure.
    let again = harness
        .state
        .privacy
        .reapply(&harness.pool)
        .await
        .expect("reapply again");
    assert!(again.examined >= result.examined);
    let still_tombstoned: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT tombstoned_at FROM coupon.users WHERE id = $1")
            .bind(subject.user_id)
            .fetch_one(&harness.pool)
            .await
            .expect("read the subject again");
    assert_eq!(still_tombstoned, tombstoned);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run the §18.5 catch-up sweep: the three expiries `JobType::ExpireCoupons` performs.
///
/// Called through the services rather than through the job registry on purpose. §14.6
/// gives `expire_coupons` an **hour shard** as its unique key, so on this shared database a
/// sweep another test already completed this hour deduplicates a fresh registration into
/// the finished job and nothing runs — the registry would be doing exactly what §12.6-10
/// asks of it while quietly making the test assert nothing. The wrapper (dedup, advisory
/// lock, checkpoint) has its own tests in `campaigns.rs`; what belongs here is the sweep.
async fn run_the_expiry_sweep(harness: &Harness) {
    let now = Utc::now();
    let batch = 1_000;

    harness
        .state
        .loyalty_stamps
        .expire_due_lots(&harness.pool, now, batch)
        .await
        .expect("expire due stamp lots");
    harness
        .state
        .wallet
        .expire_due_coupons(&harness.pool, now, batch)
        .await
        .expect("expire due coupons");
    harness
        .state
        .redemptions
        .expire_due_reservations(&harness.pool, now, batch)
        .await
        .expect("expire due reservations");
}

async fn coupon_state(harness: &Harness, coupon_id: Uuid) -> (String, i64) {
    let status: String =
        sqlx::query_scalar("SELECT status::text FROM coupon.coupon_instances WHERE id = $1")
            .bind(coupon_id)
            .fetch_one(&harness.pool)
            .await
            .expect("coupon status");

    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_status_events
         WHERE coupon_id = $1 AND to_status = 'EXPIRED'",
    )
    .bind(coupon_id)
    .fetch_one(&harness.pool)
    .await
    .expect("status events");

    (status, events)
}

async fn pending_outbox_for(harness: &Harness, user_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.outbox_events
         WHERE payload->>'user_id' = $1::text AND status IN ('PENDING', 'FAILED')",
    )
    .bind(user_id.to_string())
    .fetch_one(&harness.pool)
    .await
    .expect("pending outbox rows")
}

async fn notification_count(harness: &Harness, user_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM coupon.notifications WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&harness.pool)
        .await
        .expect("notifications")
}

async fn delivery_count(harness: &Harness, user_id: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.notification_deliveries d
         JOIN coupon.notifications n ON n.id = d.notification_id
         WHERE n.user_id = $1",
    )
    .bind(user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("deliveries")
}
