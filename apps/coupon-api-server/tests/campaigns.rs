//! Phase 3 end-to-end and concurrency tests over a real PostgreSQL (§19.2).
//!
//! These are the assertions only a real database can make. §19.2 names them one by one —
//! the last coupon under a hundred simultaneous claims, a hundred reservations of one
//! coupon, an expiry racing a confirmation, a pause racing a batch, a cancellation racing
//! a use, and a worker crash releasing its advisory lock — and is explicit that an
//! in-memory substitute settles none of them.
//!
//! ```sh
//! ./scripts/coupon/db-up.sh
//! cd apps/coupon-api-server
//! DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon sqlx migrate run
//! COUPON_TEST_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon \
//!   cargo test --workspace
//! ```
//!
//! Without `COUPON_TEST_DATABASE_URL` every test here skips with a visible note rather
//! than passing silently.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::jobs::transport::RegistryOnlyTransport;
use coupon_api_server::jobs::worker::JobRuntime;
use coupon_api_server::jobs::{AdvisoryLock, JobKey, JobSpec, JobStatus};
use coupon_api_server::state::AppState;
use coupon_api_server::{db, http};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    app: Router,
    pool: PgPool,
    state: AppState,
}

impl Harness {
    /// A worker driving the registry directly. §19.2's job tests are about the registry,
    /// the advisory lock and the checkpoint; Redis is transport only (§14.2), so running
    /// without it exercises exactly the layer under test.
    fn runtime(&self) -> JobRuntime {
        JobRuntime::new(self.state.clone(), Arc::new(RegistryOnlyTransport))
    }
}

async fn harness() -> Option<Harness> {
    // The concurrency tests fire a hundred requests at once; §16.4's limits have their
    // own tests and would otherwise refuse the hundredth claim for being the hundredth
    // rather than for the stock being gone.
    harness_with(json!({
        "rate_limit_stamp_approval_per_min": 10000,
        "rate_limit_qr_issue_per_min": 10000,
        "rate_limit_qr_resolve_failure_per_min": 10000,
        "rate_limit_campaign_claim_per_min": 10000,
    }))
    .await
}

async fn harness_with(overrides: Value) -> Option<Harness> {
    let database_url = std::env::var("COUPON_TEST_DATABASE_URL").ok()?;

    let mut settings = json!({
        "env": "test",
        "database_url": database_url,
        "firebase_project_id": "ddadan-test",
        "auth_dev_bypass": true,
        "database_max_connections": 32,
    });
    for (key, value) in overrides.as_object().expect("overrides object") {
        settings[key] = value.clone();
    }

    let config: Config = serde_json::from_value(settings).expect("test configuration");

    let pool = db::connect(&config)
        .await
        .expect("connect to the test database");
    let sealer = Sealer::from_config(&config).expect("sealer");
    let lookup_hash = LookupHash::from_config(&config).expect("lookup hash");
    let state = AppState::new(Arc::new(config), pool.clone(), None, sealer, lookup_hash)
        .expect("build state");

    Some(Harness {
        app: http::router::build(state.clone()),
        pool,
        state,
    })
}

macro_rules! harness_or_skip {
    () => {
        match harness().await {
            Some(harness) => harness,
            None => {
                eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
                return;
            }
        }
    };
}

struct Response {
    status: StatusCode,
    json: Value,
}

impl Response {
    fn data(&self) -> &Value {
        &self.json["data"]
    }

    fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    fn expect_ok(&self, context: &str) -> &Value {
        assert!(
            self.status.is_success(),
            "{context} failed with {}: {}",
            self.status,
            self.json
        );
        self.data()
    }
}

async fn send(app: &Router, method: &str, path: &str, uid: &str, body: Option<Value>) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-dev-firebase-uid", uid)
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::new_v4().to_string());

    let request = builder
        .body(match &body {
            Some(value) => Body::from(value.to_string()),
            None => Body::empty(),
        })
        .expect("valid request");

    let response = app.clone().oneshot(request).await.expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("response body");

    // Silences the unused-mut warning the builder chain would otherwise produce.
    builder = Request::builder();
    let _ = &builder;

    Response {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

fn uid(label: &str) -> String {
    format!("t3-{label}-{}", Uuid::new_v4().simple())
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

struct Store {
    owner_uid: String,
    store_id: Uuid,
}

async fn bootstrap(app: &Router, user_uid: &str, name: &str) -> Uuid {
    let response = send(
        app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        user_uid,
        Some(json!({ "display_name": name })),
    )
    .await;

    Uuid::parse_str(
        response.expect_ok("bootstrap")["id"]
            .as_str()
            .expect("user id"),
    )
    .expect("uuid")
}

async fn activate_store(pool: &PgPool, store_id: Uuid) {
    sqlx::query(
        "UPDATE coupon.stores SET status = 'ACTIVE', activated_at = clock_timestamp() WHERE id = $1",
    )
    .bind(store_id)
    .execute(pool)
    .await
    .expect("activate the store");
}

async fn store(harness: &Harness, label: &str) -> Store {
    let owner_uid = uid(&format!("{label}-owner"));
    bootstrap(&harness.app, &owner_uid, "점주").await;

    let created = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({
            "name": "테스트 카페",
            "slug": format!("t3-{}", Uuid::new_v4().simple()),
        })),
    )
    .await;
    let store_id = Uuid::parse_str(
        created.expect_ok("create store")["id"]
            .as_str()
            .expect("id"),
    )
    .expect("uuid");
    activate_store(&harness.pool, store_id).await;

    Store {
        owner_uid,
        store_id,
    }
}

/// A minimal loyalty policy. Only needed where a test exercises the stamp side.
async fn publish_policy(harness: &Harness, owner_uid: &str) {
    let policy = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        owner_uid,
        Some(json!({
            "name": "기본 도장 정책",
            "rules": {
                "target_stamp_count": 3,
                "stamps_per_order": 1,
                "minimum_order_amount": 0,
                "daily_earning_limit": null,
                "duplicate_warning_minutes": 1,
                "stamp_validity_days": 180,
                "eligible_item_ids": [],
                "eligible_category_ids": [],
                "excluded_item_ids": [],
            },
            "reward": {
                "benefit_type": "FIXED_AMOUNT",
                "fixed_amount": 3000,
                "free_item_ids": [],
                "minimum_order_amount": 0,
                "validity_days": 30,
                "title": "3,000원 할인 쿠폰",
                "description": "사용 조건",
                "customer_notice": "중복 사용 불가",
            },
        })),
    )
    .await;
    let policy_id = Uuid::parse_str(
        policy.expect_ok("draft policy")["id"]
            .as_str()
            .expect("id"),
    )
    .expect("uuid");

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish"),
        owner_uid,
        Some(json!({})),
    )
    .await
    .expect_ok("publish policy");
}

fn draft(issue_mode: &str, total: Value) -> Value {
    json!({
        "name": "여름 할인",
        "customer_description": "시원한 한 잔 2,000원 할인",
        "benefit": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 2000 },
        "minimum_order_amount": 0,
        "issue_mode": issue_mode,
        "audience_type": "ALL_CUSTOMERS",
        "total_quantity": total,
        "per_user_quantity": 1,
        "issue_starts_at": "2020-01-01T00:00:00Z",
        "issue_ends_at": "2099-01-01T00:00:00Z",
        "usable_until": "2099-06-01T00:00:00Z",
    })
}

async fn create_campaign(harness: &Harness, store: &Store, body: Value) -> Uuid {
    let created = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/campaigns",
        &store.owner_uid,
        Some(body),
    )
    .await;

    Uuid::parse_str(
        created.expect_ok("draft campaign")["id"]
            .as_str()
            .expect("id"),
    )
    .expect("uuid")
}

async fn publish_campaign(harness: &Harness, store: &Store, campaign_id: Uuid) -> Value {
    let published = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/publish"),
        &store.owner_uid,
        None,
    )
    .await;

    published.expect_ok("publish campaign").clone()
}

/// Run this campaign's jobs to completion.
///
/// Scoped to one campaign on purpose: the tests in this binary share a database and run
/// in parallel, so a global poll would be running another test's jobs. The relay is still
/// the real one — publishing is harmless to anybody else — and each job is then taken
/// through `run_once`, which is exactly what the Redis consumer does with a delivered id.
async fn drive_jobs(harness: &Harness, campaign_id: Uuid, rounds: usize) {
    let runtime = harness.runtime();

    for _ in 0..rounds {
        relay_until_dispatched(harness, &runtime, campaign_id).await;

        let due: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM coupon.job_registry
             WHERE resource_id = $1 AND status IN ('QUEUED', 'RETRY_WAIT')
             ORDER BY created_at",
        )
        .bind(campaign_id)
        .fetch_all(&harness.pool)
        .await
        .expect("due jobs");

        if due.is_empty() {
            continue;
        }
        for job_id in due {
            runtime.run_once(job_id).await.expect("run the job");
        }
    }
}

/// Wait until `id`'s jobs have left the outbox — `id` being either a job id or the
/// campaign whose jobs these are.
///
/// One relay pass publishes at most `POLL_BATCH` rows, oldest first, from an
/// `outbox_events` table every test in this binary shares. Under `cargo test`'s default
/// parallelism a single pass can therefore spend its whole batch on another test's
/// backlog and leave this test's row `PENDING_OUTBOX` — where `claim` correctly refuses
/// it, and the test reads a job that silently never ran. That is a test-harness artefact,
/// not a defect: in production the relay is a loop and the next tick takes the remainder.
/// This spells that loop out, and waits for the *effect* rather than for its own pass to
/// have caused it, because a parallel test's relay may well publish the row first.
async fn relay_until_dispatched(harness: &Harness, runtime: &JobRuntime, id: Uuid) {
    for _ in 0..60 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM coupon.job_registry
             WHERE status = 'PENDING_OUTBOX' AND (id = $1 OR resource_id = $1)",
        )
        .bind(id)
        .fetch_one(&harness.pool)
        .await
        .expect("jobs still in the outbox");

        if waiting == 0 {
            return;
        }
        runtime.relay().await.expect("relay the outbox");
    }

    panic!("{id}: 아웃박스에 남은 job 이 릴레이되지 않았다");
}

async fn consumer(app: &Router, label: &str) -> String {
    let consumer_uid = uid(label);
    bootstrap(app, &consumer_uid, "김손님").await;
    consumer_uid
}

async fn issue_qr(app: &Router, consumer_uid: &str) -> String {
    let response = send(app, "POST", "/api/coupon/v1/me/qr-tokens", consumer_uid, None).await;
    response.expect_ok("issue qr")["token"]
        .as_str()
        .expect("token")
        .to_owned()
}

fn order(amount: i64, reference: Option<&str>) -> Value {
    json!({
        "gross_amount": amount,
        "currency": "KRW",
        "items": [],
        "external_order_ref": reference,
    })
}

// ---------------------------------------------------------------------------
// §19.2: 마지막 1장에 100개 동시 claim, 성공 수 정확히 1
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_hundred_simultaneous_claims_on_the_last_coupon_produce_exactly_one() {
    let harness = harness_or_skip!();
    let store = store(&harness, "soldout").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 1 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    // A hundred distinct consumers, so nothing is deduplicated by the per-person limit.
    let mut consumers = Vec::new();
    for index in 0..100 {
        consumers.push(consumer(&harness.app, &format!("claimer-{index}")).await);
    }

    let claims = futures_join(consumers.iter().map(|consumer_uid| {
        let app = harness.app.clone();
        let consumer_uid = consumer_uid.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
                &consumer_uid,
                None,
            )
            .await
        }
    }))
    .await;

    let winners: Vec<&Response> = claims
        .iter()
        .filter(|response| response.status.is_success())
        .collect();
    let sold_out = claims
        .iter()
        .filter(|response| response.error_code() == "CAMPAIGN_SOLD_OUT")
        .count();

    assert_eq!(
        winners.len(),
        1,
        "§19.2: 마지막 1장에 100개 동시 claim, 성공 수 정확히 1 — got {:?}",
        claims
            .iter()
            .map(|r| (r.status.as_u16(), r.error_code().to_owned()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        sold_out,
        99,
        "every loser must get CAMPAIGN_SOLD_OUT, not a generic conflict"
    );

    // §12.6-4, checked in the database rather than in the responses.
    let issued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(issued, 1);

    let counters: (i64, i64) = sqlx::query_as(
        "SELECT global_issued_count, COALESCE((SELECT SUM(issued_count) FROM coupon.campaign_counters
         WHERE campaign_id = $1), 0)::bigint FROM coupon.campaigns WHERE id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("counters");
    assert_eq!(counters, (1, 1), "both counters agree with the instance count");
}

#[tokio::test]
async fn a_repeat_claim_returns_the_coupon_the_customer_already_has() {
    // §11.3: 중복 요청이면 기존 쿠폰 ID를 반환한다.
    let harness = harness_or_skip!();
    let store = store(&harness, "repeat").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 10 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    let consumer_uid = consumer(&harness.app, "repeat").await;
    let path = format!("/api/coupon/v1/campaigns/{campaign_id}/claims");

    let first = send(&harness.app, "POST", &path, &consumer_uid, None).await;
    let coupon_id = first.expect_ok("first claim")["coupon_id"].clone();
    assert_eq!(first.data()["already_claimed"], false);

    let second = send(&harness.app, "POST", &path, &consumer_uid, None).await;
    assert!(second.status.is_success(), "{}", second.json);
    assert_eq!(second.data()["coupon_id"], coupon_id);
    assert_eq!(second.data()["already_claimed"], true);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(count, 1, "the per-person limit is one coupon, not one per press");
}

#[tokio::test]
async fn a_paused_campaign_refuses_a_claim_with_its_own_reason() {
    // CAMPAIGN-006: 중지 중 선착순 요청은 CAMPAIGN_PAUSED 를 반환한다.
    let harness = harness_or_skip!();
    let store = store(&harness, "paused").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 10 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/pause"),
        &store.owner_uid,
        Some(json!({ "reason": "재고 확인" })),
    )
    .await
    .expect_ok("pause");

    let consumer_uid = consumer(&harness.app, "paused").await;
    let refused = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await;
    assert_eq!(refused.error_code(), "CAMPAIGN_PAUSED");

    // And resuming lets the same customer through, with no coupon lost in between.
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/resume"),
        &store.owner_uid,
        None,
    )
    .await
    .expect_ok("resume");

    let allowed = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await;
    assert!(allowed.status.is_success(), "{}", allowed.json);
}

// ---------------------------------------------------------------------------
// §19.2: 같은 쿠폰 100개 동시 예약, 활성 예약 1건
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_hundred_simultaneous_reservations_of_one_coupon_leave_one_active() {
    let harness = harness_or_skip!();
    let store = store(&harness, "reserve").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    let consumer_uid = consumer(&harness.app, "reserve").await;
    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await;
    let coupon_id = claim.expect_ok("claim")["coupon_id"]
        .as_str()
        .expect("coupon id")
        .to_owned();

    let token = issue_qr(&harness.app, &consumer_uid).await;

    // Distinct till sessions, so what is under test is the coupon's own uniqueness rather
    // than REDEEM-002's one-reservation-per-session rule.
    let attempts = futures_join((0..100).map(|index| {
        let app = harness.app.clone();
        let owner_uid = store.owner_uid.clone();
        let token = token.clone();
        let coupon_id = coupon_id.clone();
        async move {
            send(
                &app,
                "POST",
                "/api/coupon/v1/owner/redemptions/preview",
                &owner_uid,
                Some(json!({
                    "qr_token": token,
                    "coupon_id": coupon_id,
                    "owner_session_id": format!("till-{index}"),
                    "order": order(12_000, None),
                })),
            )
            .await
        }
    }))
    .await;

    let winners = attempts.iter().filter(|r| r.status.is_success()).count();
    assert_eq!(winners, 1, "§12.6-6: 쿠폰당 활성 예약은 최대 1개");

    for loser in attempts.iter().filter(|r| !r.status.is_success()) {
        assert_eq!(
            loser.error_code(),
            "COUPON_NOT_AVAILABLE",
            "§15 gives the losing side of a reservation race its own code: {}",
            loser.json
        );
    }

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.redemption_reservations
         WHERE coupon_id = $1 AND status = 'ACTIVE'",
    )
    .bind(Uuid::parse_str(&coupon_id).expect("uuid"))
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(active, 1);

    let status: String = sqlx::query_scalar(
        "SELECT status::text FROM coupon.coupon_instances WHERE id = $1",
    )
    .bind(Uuid::parse_str(&coupon_id).expect("uuid"))
    .fetch_one(&harness.pool)
    .await
    .expect("status");
    assert_eq!(status, "RESERVED");
}

// ---------------------------------------------------------------------------
// §19.2: 예약 만료와 승인 동시 실행, 종결 상태 하나
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_expiring_reservation_and_a_confirmation_settle_on_exactly_one_outcome() {
    let harness = harness_or_skip!();
    let store = store(&harness, "race").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    let consumer_uid = consumer(&harness.app, "race").await;
    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await;
    let coupon_id = Uuid::parse_str(
        claim.expect_ok("claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    let token = issue_qr(&harness.app, &consumer_uid).await;
    let reserved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &store.owner_uid,
        Some(json!({
            "qr_token": token,
            "coupon_id": coupon_id,
            "owner_session_id": "till-race",
            "order": order(12_000, None),
        })),
    )
    .await;
    let reservation_id = Uuid::parse_str(
        reserved.expect_ok("reserve")["reservation_id"]
            .as_str()
            .expect("reservation id"),
    )
    .expect("uuid");

    // Put the deadline exactly at the present instant, which is the moment the two
    // decisions disagree about. §15 says the row lock and the server clock settle it.
    sqlx::query("UPDATE coupon.redemption_reservations SET expires_at = clock_timestamp() WHERE id = $1")
        .bind(reservation_id)
        .execute(&harness.pool)
        .await
        .expect("age the reservation");

    let confirm = {
        let app = harness.app.clone();
        let owner_uid = store.owner_uid.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/confirm"),
                &owner_uid,
                Some(json!({
                    "owner_session_id": "till-race",
                    "order": order(12_000, None),
                })),
            )
            .await
        }
    };

    let sweep = async {
        harness
            .state
            .redemptions
            .expire_due_reservations(&harness.pool, chrono::Utc::now(), 100)
            .await
            .expect("sweep")
    };

    let (confirmed, _) = tokio::join!(confirm, sweep);

    let reservation_status: String = sqlx::query_scalar(
        "SELECT status::text FROM coupon.redemption_reservations WHERE id = $1",
    )
    .bind(reservation_id)
    .fetch_one(&harness.pool)
    .await
    .expect("reservation status");

    let coupon_status: String =
        sqlx::query_scalar("SELECT status::text FROM coupon.coupon_instances WHERE id = $1")
            .bind(coupon_id)
            .fetch_one(&harness.pool)
            .await
            .expect("coupon status");

    let redemptions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.redemption_transactions
         WHERE reservation_id = $1 AND status = 'CONFIRMED'",
    )
    .bind(reservation_id)
    .fetch_one(&harness.pool)
    .await
    .expect("redemption count");

    // §15: 사용·복구 중 하나만 커밋. Whichever won, the three facts must agree.
    if confirmed.status.is_success() {
        assert_eq!(reservation_status, "CONFIRMED");
        assert_eq!(coupon_status, "USED");
        assert_eq!(redemptions, 1);
    } else {
        assert_eq!(
            confirmed.error_code(),
            "RESERVATION_EXPIRED",
            "{}",
            confirmed.json
        );
        assert_eq!(reservation_status, "EXPIRED");
        assert_eq!(
            coupon_status, "AVAILABLE",
            "an expired hold returns the coupon rather than consuming it"
        );
        assert_eq!(redemptions, 0);
    }
}

// ---------------------------------------------------------------------------
// §19.2: 취소와 사용 경합
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_revocation_and_a_use_of_the_same_coupon_cannot_both_win() {
    let harness = harness_or_skip!();
    let store = store(&harness, "revoke").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    let consumer_uid = consumer(&harness.app, "revoke").await;
    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await;
    let coupon_id = Uuid::parse_str(
        claim.expect_ok("claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    let token = issue_qr(&harness.app, &consumer_uid).await;
    let reserved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &store.owner_uid,
        Some(json!({
            "qr_token": token,
            "coupon_id": coupon_id,
            "owner_session_id": "till-revoke",
            "order": order(12_000, None),
        })),
    )
    .await;
    let reservation_id = Uuid::parse_str(
        reserved.expect_ok("reserve")["reservation_id"]
            .as_str()
            .expect("id"),
    )
    .expect("uuid");

    // The owner cancels the whole campaign with 전부 회수 while the till confirms.
    let cancel = {
        let app = harness.app.clone();
        let owner_uid = store.owner_uid.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/cancel"),
                &owner_uid,
                Some(json!({ "revoke_policy": "REVOKE_UNUSED", "reason": "가격 오기재" })),
            )
            .await
        }
    };
    let confirm = {
        let app = harness.app.clone();
        let owner_uid = store.owner_uid.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/confirm"),
                &owner_uid,
                Some(json!({
                    "owner_session_id": "till-revoke",
                    "order": order(12_000, None),
                })),
            )
            .await
        }
    };

    let (_, confirmed) = tokio::join!(cancel, confirm);

    // The revocation job runs afterwards, exactly as it would in production.
    drive_jobs(&harness, campaign_id, 3).await;

    let coupon_status: String =
        sqlx::query_scalar("SELECT status::text FROM coupon.coupon_instances WHERE id = $1")
            .bind(coupon_id)
            .fetch_one(&harness.pool)
            .await
            .expect("coupon status");

    let confirmed_uses: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.redemption_transactions
         WHERE coupon_id = $1 AND status = 'CONFIRMED'",
    )
    .bind(coupon_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");

    // §15: 사용과 회수 동시 불가. A coupon that was spent stays spent and is left out of
    // the revocation (ADMIN-005: 이미 사용된 쿠폰은 상태를 바꾸지 않는다).
    if confirmed.status.is_success() {
        assert_eq!(coupon_status, "USED");
        assert_eq!(confirmed_uses, 1);
    } else {
        assert_eq!(coupon_status, "REVOKED", "{}", confirmed.json);
        assert_eq!(confirmed_uses, 0);
    }
}

// ---------------------------------------------------------------------------
// §19.2: 캠페인 중지와 발급 batch 경합
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pausing_a_campaign_stops_the_issuing_batch_and_resuming_continues_it() {
    let harness = harness_or_skip!();
    let store = store(&harness, "batch").await;
    publish_policy(&harness, &store.owner_uid).await;

    // Three customers of this store, so a DIRECT campaign has an audience to snapshot.
    let mut consumers = Vec::new();
    for index in 0..3 {
        let consumer_uid = consumer(&harness.app, &format!("batch-{index}")).await;
        let user_id: Uuid = sqlx::query_scalar("SELECT id FROM coupon.users WHERE firebase_uid = $1")
            .bind(&consumer_uid)
            .fetch_one(&harness.pool)
            .await
            .expect("user id");
        sqlx::query(
            "INSERT INTO coupon.store_customers (store_id, user_id, alias)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(store.store_id)
        .bind(user_id)
        .bind(format!("손님-{index}"))
        .execute(&harness.pool)
        .await
        .expect("store customer");
        consumers.push((consumer_uid, user_id));
    }

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("DIRECT", json!({ "mode": "LIMITED", "quantity": 100 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    // The audience job runs and hands over to the issuing job.
    drive_jobs(&harness, campaign_id, 1).await;
    let snapshotted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.campaign_audience_members WHERE campaign_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(snapshotted, 3, "CAMPAIGN-003: 게시 시점의 대상 스냅샷");

    // Pause before the issuing job gets to run.
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/pause"),
        &store.owner_uid,
        Some(json!({ "reason": "확인 필요" })),
    )
    .await
    .expect_ok("pause");

    drive_jobs(&harness, campaign_id, 2).await;

    let issued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(
        issued, 0,
        "§15: 캠페인 중지와 발급 batch — 중지 확인 뒤 신규 배치 없음"
    );

    // Resuming continues from the same job and the same checkpoint.
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/resume"),
        &store.owner_uid,
        None,
    )
    .await
    .expect_ok("resume");

    drive_jobs(&harness, campaign_id, 4).await;

    let issued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(issued, 3, "CAMPAIGN-006: 미처리 대상부터 계속한다");

    // Re-running the finished job issues nothing more: §14.2's domain uniqueness, not the
    // lock, is what guarantees that.
    drive_jobs(&harness, campaign_id, 2).await;
    let issued_again: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(issued_again, 3);
}

// ---------------------------------------------------------------------------
// §19.2 / §12.6-10: 동일 job unique key 의 활성 작업은 최대 1개
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registering_the_same_job_twice_returns_the_first_job() {
    // JOB-001: 활성 작업 고유 제약으로 하나만 생성되고 나머지는 기존 job_id 를 반환한다.
    let harness = harness_or_skip!();
    let store = store(&harness, "jobkey").await;

    let key = JobKey::issue_campaign(store.store_id, Uuid::new_v4(), 1);
    let spec = JobSpec::new(key.clone(), json!({ "campaign_id": Uuid::new_v4() }))
        .store(store.store_id);

    let mut tx = harness.pool.begin().await.expect("tx");
    let first = harness.state.jobs.enqueue(&mut tx, &spec).await.expect("first");
    tx.commit().await.expect("commit");

    let mut tx = harness.pool.begin().await.expect("tx");
    let second = harness
        .state
        .jobs
        .enqueue(&mut tx, &spec)
        .await
        .expect("second");
    tx.commit().await.expect("commit");

    assert_eq!(second.job_id, first.job_id);
    assert!(second.deduplicated, "the second registration must not create a job");

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.job_registry
         WHERE unique_key = $1
           AND status IN ('PENDING_OUTBOX','QUEUED','RUNNING','RETRY_WAIT','PAUSE_REQUESTED','PAUSED')",
    )
    .bind(key.to_string())
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(active, 1, "§12.6-10");
}

#[tokio::test]
async fn concurrent_registrations_of_one_key_still_produce_one_job() {
    let harness = harness_or_skip!();
    let store = store(&harness, "jobrace").await;

    let key = JobKey::issue_campaign(store.store_id, Uuid::new_v4(), 1);

    let results = futures_join((0..16).map(|_| {
        let pool = harness.pool.clone();
        let jobs = harness.state.jobs.clone();
        let spec = JobSpec::new(key.clone(), json!({})).store(store.store_id);
        async move {
            let mut tx = pool.begin().await.expect("tx");
            let enqueued = jobs.enqueue(&mut tx, &spec).await;
            match enqueued {
                Ok(job) => {
                    tx.commit().await.expect("commit");
                    Some(job.job_id)
                }
                Err(_) => None,
            }
        }
    }))
    .await;

    let mut ids: Vec<Uuid> = results.into_iter().flatten().collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 1, "sixteen registrations, one logical job");

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.job_registry
         WHERE unique_key = $1
           AND status IN ('PENDING_OUTBOX','QUEUED','RUNNING','RETRY_WAIT','PAUSE_REQUESTED','PAUSED')",
    )
    .bind(key.to_string())
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(active, 1);
}

// ---------------------------------------------------------------------------
// §19.2: worker crash 후 advisory lock 해제와 체크포인트 재개
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_advisory_lock_blocks_a_second_worker_and_is_released_when_the_first_goes_away() {
    // §14.5-5/6/9. The lock lives on a dedicated connection, so a worker that dies has it
    // released by PostgreSQL closing that connection — no lease, nothing to expire.
    let harness = harness_or_skip!();
    let key = JobKey::issue_campaign(Uuid::new_v4(), Uuid::new_v4(), 1);

    let held = AdvisoryLock::try_acquire(&harness.pool, key.advisory_lock_key())
        .await
        .expect("acquire")
        .expect("the first worker takes the lock");

    let contended = AdvisoryLock::try_acquire(&harness.pool, key.advisory_lock_key())
        .await
        .expect("try");
    assert!(
        contended.is_none(),
        "§14.5-6: a second worker must not take a lock somebody else holds"
    );

    // The worker "crashes": its guard is dropped, which closes the connection.
    drop(held);
    // The connection close is asynchronous; give the server a moment to notice.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let recovered = AdvisoryLock::try_acquire(&harness.pool, key.advisory_lock_key())
        .await
        .expect("try");
    assert!(
        recovered.is_some(),
        "§14.5-9: a crashed worker's lock must free itself"
    );
    if let Some(lock) = recovered {
        lock.release().await;
    }
}

#[tokio::test]
async fn a_job_interrupted_mid_run_resumes_from_its_checkpoint() {
    let harness = harness_or_skip!();
    let store = store(&harness, "resume").await;
    publish_policy(&harness, &store.owner_uid).await;

    let mut user_ids = Vec::new();
    for index in 0..4 {
        let consumer_uid = consumer(&harness.app, &format!("resume-{index}")).await;
        let user_id: Uuid = sqlx::query_scalar("SELECT id FROM coupon.users WHERE firebase_uid = $1")
            .bind(&consumer_uid)
            .fetch_one(&harness.pool)
            .await
            .expect("user id");
        sqlx::query(
            "INSERT INTO coupon.store_customers (store_id, user_id, alias)
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(store.store_id)
        .bind(user_id)
        .bind(format!("손님-{index}"))
        .execute(&harness.pool)
        .await
        .expect("store customer");
        user_ids.push(user_id);
    }
    user_ids.sort_unstable();

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("DIRECT", json!({ "mode": "LIMITED", "quantity": 100 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    drive_jobs(&harness, campaign_id, 1).await;

    // Simulate a worker that issued the first two and then died: two members are marked
    // issued by hand, the checkpoint names the second, and the job is put back on the
    // queue exactly as `reclaim_stalled` would leave it.
    let job_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM coupon.job_registry
         WHERE resource_id = $1 AND job_type = 'issue_campaign'",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("issuing job");

    sqlx::query(
        "UPDATE coupon.job_registry
         SET status = 'QUEUED', checkpoint = $2
         WHERE id = $1",
    )
    .bind(job_id)
    .bind(json!({ "after_id": user_ids[1], "batches": 1 }))
    .execute(&harness.pool)
    .await
    .expect("checkpoint");

    for user_id in &user_ids[..2] {
        sqlx::query(
            "UPDATE coupon.campaign_audience_members
             SET status = 'ISSUED', processed_at = clock_timestamp()
             WHERE campaign_id = $1 AND user_id = $2",
        )
        .bind(campaign_id)
        .bind(user_id)
        .execute(&harness.pool)
        .await
        .expect("mark issued");
    }

    drive_jobs(&harness, campaign_id, 3).await;

    // Only the two the checkpoint had not reached were issued: a resume continues, it
    // does not start the campaign over.
    let issued: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM coupon.coupon_instances WHERE campaign_id = $1 ORDER BY user_id",
    )
    .bind(campaign_id)
    .fetch_all(&harness.pool)
    .await
    .expect("issued");

    assert_eq!(
        issued,
        user_ids[2..].to_vec(),
        "JOB-003: 성공 항목은 반복하지 않고 실패 체크포인트부터 계속한다"
    );

    let job_status: String =
        sqlx::query_scalar("SELECT status::text FROM coupon.job_registry WHERE id = $1")
            .bind(job_id)
            .fetch_one(&harness.pool)
            .await
            .expect("job status");
    assert_eq!(job_status, JobStatus::Succeeded.as_db());
}

// ---------------------------------------------------------------------------
// End to end: 게시 → 대량 발급 → 지갑 → 예약 → 승인
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_direct_campaign_runs_from_publication_through_to_a_confirmed_use() {
    let harness = harness_or_skip!();
    let store = store(&harness, "e2e").await;
    publish_policy(&harness, &store.owner_uid).await;

    let consumer_uid = consumer(&harness.app, "e2e").await;
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM coupon.users WHERE firebase_uid = $1")
        .bind(&consumer_uid)
        .fetch_one(&harness.pool)
        .await
        .expect("user id");
    sqlx::query(
        "INSERT INTO coupon.store_customers (store_id, user_id, alias)
         VALUES ($1, $2, '손님-E2E') ON CONFLICT DO NOTHING",
    )
    .bind(store.store_id)
    .bind(user_id)
    .execute(&harness.pool)
    .await
    .expect("store customer");

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("DIRECT", json!({ "mode": "UNLIMITED", "operational_cap": 500 })),
    )
    .await;

    // CAMPAIGN-002 shows the cost before the confirmation modal.
    let estimate = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/estimate"),
        &store.owner_uid,
        None,
    )
    .await;
    assert_eq!(estimate.expect_ok("estimate")["audience_size"], 1);

    let published = publish_campaign(&harness, &store, campaign_id).await;
    assert_eq!(published["status"], "ISSUING");
    assert!(published["job_id"].is_string(), "the issuing job is registered");

    // The worker builds the audience, hands over, and issues.
    drive_jobs(&harness, campaign_id, 4).await;

    // §11.6: 발급 DB 커밋 후 소비자 지갑 조회에 반영된다.
    let wallet = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/coupons?status=AVAILABLE",
        &consumer_uid,
        None,
    )
    .await;
    let coupons = wallet.expect_ok("wallet")["items"]
        .as_array()
        .expect("items")
        .clone();
    assert_eq!(coupons.len(), 1, "{:?}", coupons);
    let coupon_id = coupons[0]["id"].as_str().expect("coupon id").to_owned();
    assert_eq!(coupons[0]["effective_status"], "AVAILABLE");

    // Both jobs are recorded, and neither is still active.
    let jobs: Vec<(String, String)> = sqlx::query_as(
        "SELECT job_type, status::text FROM coupon.job_registry
         WHERE resource_id = $1 ORDER BY created_at",
    )
    .bind(campaign_id)
    .fetch_all(&harness.pool)
    .await
    .expect("jobs");
    assert_eq!(
        jobs,
        vec![
            ("build_campaign_audience".to_owned(), "SUCCEEDED".to_owned()),
            ("issue_campaign".to_owned(), "SUCCEEDED".to_owned()),
        ]
    );

    // REDEEM-001: reserve, then approve.
    let token = issue_qr(&harness.app, &consumer_uid).await;
    let reserved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &store.owner_uid,
        Some(json!({
            "qr_token": token,
            "coupon_id": coupon_id,
            "owner_session_id": "till-e2e",
            "order": order(12_000, Some("POS-E2E-1")),
        })),
    )
    .await;
    let reservation = reserved.expect_ok("reserve");
    assert_eq!(reservation["expected_discount_amount"], 2_000);
    assert_eq!(reservation["payable_amount"], 10_000);
    let reservation_id = reservation["reservation_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let confirmed = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/confirm"),
        &store.owner_uid,
        Some(json!({
            "owner_session_id": "till-e2e",
            "order": order(12_000, Some("POS-E2E-1")),
        })),
    )
    .await;
    let redemption = confirmed.expect_ok("confirm");
    assert_eq!(redemption["discount_amount"], 2_000);
    assert_eq!(redemption["coupon_status"], "USED");

    // The ledger row, seen directly.
    let row: (i64, i64, String) = sqlx::query_as(
        "SELECT gross_amount, discount_amount, status::text FROM coupon.redemption_transactions
         WHERE coupon_id = $1",
    )
    .bind(Uuid::parse_str(&coupon_id).expect("uuid"))
    .fetch_one(&harness.pool)
    .await
    .expect("redemption row");
    assert_eq!(row, (12_000, 2_000, "CONFIRMED".to_owned()));

    // §5.4: 주문 1건에 혜택 1개. A second benefit against the same POS order is refused.
    let second_claim = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &store.owner_uid,
        Some(json!({
            "qr_token": issue_qr(&harness.app, &consumer_uid).await,
            "coupon_id": coupon_id,
            "owner_session_id": "till-e2e-2",
            "order": order(12_000, Some("POS-E2E-1")),
        })),
    )
    .await;
    assert!(!second_claim.status.is_success());
}

#[tokio::test]
async fn an_owner_may_undo_a_use_inside_the_ten_minute_window() {
    // REDEEM-004.
    let harness = harness_or_skip!();
    let store = store(&harness, "undo").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    let consumer_uid = consumer(&harness.app, "undo").await;
    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await;
    let coupon_id = claim.expect_ok("claim")["coupon_id"]
        .as_str()
        .expect("id")
        .to_owned();

    let token = issue_qr(&harness.app, &consumer_uid).await;
    let reserved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &store.owner_uid,
        Some(json!({
            "qr_token": token,
            "coupon_id": coupon_id,
            "owner_session_id": "till-undo",
            "order": order(9_000, None),
        })),
    )
    .await;
    let reservation_id = reserved.expect_ok("reserve")["reservation_id"]
        .as_str()
        .expect("id")
        .to_owned();

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/confirm"),
        &store.owner_uid,
        Some(json!({
            "owner_session_id": "till-undo",
            "order": order(9_000, None),
        })),
    )
    .await
    .expect_ok("confirm");

    let cancelled = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/cancel"),
        &store.owner_uid,
        Some(json!({ "reason": "주문 취소", "restore_coupon": true })),
    )
    .await;
    let voided = cancelled.expect_ok("cancel");
    assert_eq!(voided["coupon_restored"], true);
    assert_eq!(voided["coupon_status"], "AVAILABLE");

    // The use is voided rather than deleted, and the coupon is spendable again.
    let (status, restored): (String, bool) = sqlx::query_as(
        "SELECT status::text, coupon_restored FROM coupon.redemption_transactions
         WHERE coupon_id = $1",
    )
    .bind(Uuid::parse_str(&coupon_id).expect("uuid"))
    .fetch_one(&harness.pool)
    .await
    .expect("redemption row");
    assert_eq!(status, "VOIDED");
    assert!(restored);

    let events: Vec<String> = sqlx::query_scalar(
        "SELECT to_status::text FROM coupon.coupon_status_events
         WHERE coupon_id = $1 ORDER BY occurred_at, id",
    )
    .bind(Uuid::parse_str(&coupon_id).expect("uuid"))
    .fetch_all(&harness.pool)
    .await
    .expect("events");
    assert_eq!(
        events,
        vec!["AVAILABLE", "RESERVED", "USED", "AVAILABLE"],
        "§12.6-8: every transition is recorded, none is rewritten"
    );
}

#[tokio::test]
async fn a_published_campaign_refuses_an_edit_that_would_reach_issued_coupons() {
    // §8.5 / CAMPAIGN-008.
    let harness = harness_or_skip!();
    let store = store(&harness, "edit").await;

    let campaign_id = create_campaign(
        &harness,
        &store,
        draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 10 })),
    )
    .await;
    publish_campaign(&harness, &store, campaign_id).await;

    let consumer_uid = consumer(&harness.app, "edit").await;
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &consumer_uid,
        None,
    )
    .await
    .expect_ok("claim");

    let refused = send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}"),
        &store.owner_uid,
        Some(json!({ "benefit": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 9000 } })),
    )
    .await;
    assert_eq!(refused.error_code(), "CAMPAIGN_NOT_EDITABLE");

    // CAMPAIGN-008: 시작 후 증량은 가능하다.
    let increased = send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}"),
        &store.owner_uid,
        Some(json!({ "total_quantity": { "mode": "LIMITED", "quantity": 50 } })),
    )
    .await;
    assert_eq!(
        increased.expect_ok("increase")["total_quantity"]["quantity"],
        50
    );

    // 이미 발급·예약된 수량 미만으로는 낮출 수 없다.
    let reduced = send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}"),
        &store.owner_uid,
        Some(json!({ "total_quantity": { "mode": "LIMITED", "quantity": 0 } })),
    )
    .await;
    assert!(!reduced.status.is_success());
}

// ---------------------------------------------------------------------------
// A tiny join helper, so the tests do not need the `futures` crate.
// ---------------------------------------------------------------------------

/// Spawn every future and collect the results in order.
///
/// `tokio::spawn` rather than `join_all`: §19.2's tests are about *simultaneous*
/// requests, and futures polled on one task would interleave at await points rather than
/// genuinely contend.
async fn futures_join<I, F, T>(futures: I) -> Vec<T>
where
    I: IntoIterator<Item = F>,
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = futures.into_iter().map(tokio::spawn).collect();

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(handle.await.expect("task did not panic"));
    }
    results
}

// ---------------------------------------------------------------------------
// §3.3 / ADMIN-003: 원장 보정은 요청자와 승인자가 달라야 한다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_ledger_correction_needs_a_second_administrator_and_runs_as_a_job() {
    let harness = harness_or_skip!();
    let store = store(&harness, "adjust").await;
    publish_policy(&harness, &store.owner_uid).await;

    let consumer_uid = consumer(&harness.app, "adjust").await;
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM coupon.users WHERE firebase_uid = $1")
        .bind(&consumer_uid)
        .fetch_one(&harness.pool)
        .await
        .expect("user id");

    let requester_uid = uid("adjust-requester");
    let requester_id = bootstrap(&harness.app, &requester_uid, "운영자 A").await;
    grant_admin(&harness.pool, requester_id).await;

    let approver_uid = uid("adjust-approver");
    let approver_id = bootstrap(&harness.app, &approver_uid, "운영자 B").await;
    grant_admin(&harness.pool, approver_id).await;

    let case_id: Uuid = sqlx::query_scalar(
        "INSERT INTO coupon.admin_cases (case_type, title, description, subject_user_id,
                                         subject_store_id, opened_by_user_id)
         VALUES ('WRONG_STAMP', '도장 미적립 민원', '고객이 도장을 못 받았다고 신고', $1, $2, $3)
         RETURNING id",
    )
    .bind(user_id)
    .bind(store.store_id)
    .bind(requester_id)
    .fetch_one(&harness.pool)
    .await
    .expect("case");

    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/adjustments/preview",
        &requester_uid,
        Some(json!({
            "case_id": case_id,
            "adjustment_type": "STAMP_GRANT",
            "store_id": store.store_id,
            "user_id": user_id,
            "quantity": 2,
            "reason": "민원 확인 후 도장 2개 보정",
        })),
    )
    .await;
    let adjustment_id = preview.expect_ok("preview")["adjustment_id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(preview.data()["executable"], true);

    // §3.3: the person who asked may not be the person who approves.
    let self_approved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/adjustments",
        &requester_uid,
        Some(json!({ "adjustment_id": adjustment_id, "approval_reason": "내가 승인" })),
    )
    .await;
    assert_eq!(self_approved.error_code(), "APPROVAL_SEPARATION_REQUIRED");
    assert_eq!(self_approved.status, StatusCode::FORBIDDEN);

    let approved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/adjustments",
        &approver_uid,
        Some(json!({ "adjustment_id": adjustment_id, "approval_reason": "증빙 확인함" })),
    )
    .await;
    let job_id = Uuid::parse_str(
        approved.expect_ok("approve")["execution_job_id"]
            .as_str()
            .expect("job id"),
    )
    .expect("uuid");

    // ADMIN-003: 대량 보정은 동기 API 가 아니라 검토 가능한 큐 작업으로 실행한다.
    let runtime = harness.runtime();
    relay_until_dispatched(&harness, &runtime, job_id).await;
    runtime.run_once(job_id).await.expect("run");

    let granted: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity_delta), 0)::bigint FROM coupon.stamp_ledger
         WHERE reason_code = 'ADMIN_STAMP_GRANT'
           AND metadata ->> 'adjustment_id' = $1",
    )
    .bind(&adjustment_id)
    .fetch_one(&harness.pool)
    .await
    .expect("ledger");
    assert_eq!(granted, 2);

    let status: String =
        sqlx::query_scalar("SELECT status::text FROM coupon.admin_adjustments WHERE id = $1")
            .bind(Uuid::parse_str(&adjustment_id).expect("uuid"))
            .fetch_one(&harness.pool)
            .await
            .expect("status");
    assert_eq!(status, "SUCCEEDED");

    // Re-running the job must not grant a second time: the ledger event carries the
    // adjustment id, so "already applied" is a question the ledger itself answers.
    sqlx::query("UPDATE coupon.job_registry SET status = 'QUEUED' WHERE id = $1")
        .bind(job_id)
        .execute(&harness.pool)
        .await
        .expect("requeue");
    runtime.run_once(job_id).await.expect("re-run");

    let granted_again: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity_delta), 0)::bigint FROM coupon.stamp_ledger
         WHERE reason_code = 'ADMIN_STAMP_GRANT'
           AND metadata ->> 'adjustment_id' = $1",
    )
    .bind(&adjustment_id)
    .fetch_one(&harness.pool)
    .await
    .expect("ledger");
    assert_eq!(granted_again, 2, "§14.2: a job that runs twice must land once");
}

async fn grant_admin(pool: &PgPool, user_id: Uuid) {
    sqlx::query(
        "INSERT INTO coupon.user_roles (user_id, role) VALUES ($1, 'OPERATIONS')
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .expect("grant the role");
}
