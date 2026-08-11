//! Phase 2 end-to-end and concurrency tests over a real PostgreSQL (§19.2).
//!
//! These are the assertions that only a real database can make: the ledger invariants,
//! the single-use QR under contention, and the idempotency contract. §19.2 is explicit
//! that an in-memory substitute does not settle these questions.
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
    /// The same service graph the router uses, so a test can drive a background sweep
    /// directly instead of rebuilding one.
    state: AppState,
}

async fn harness() -> Option<Harness> {
    // §19.2's concurrency tests are about the *ledger* holding under contention, so the
    // §16.4 rate limits are lifted here — otherwise the hundredth scan would be refused
    // for being the hundredth rather than for the QR being spent. The limits have their
    // own test below, and their own unit tests.
    harness_with(json!({
        "rate_limit_stamp_approval_per_min": 1000,
        "rate_limit_qr_issue_per_min": 1000,
        "rate_limit_qr_resolve_failure_per_min": 1000,
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
        // The concurrency tests fire a hundred requests at once and each one holds a
        // connection while it waits on the store row lock.
        "database_max_connections": 32,
    });
    for (key, value) in overrides.as_object().expect("overrides object") {
        settings[key] = value.clone();
    }

    let config: Config = serde_json::from_value(settings).expect("test configuration");

    let pool = db::connect(&config).await.expect("connect to the test database");
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
    ($overrides:expr) => {
        match harness_with($overrides).await {
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

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    uid: &str,
    body: Option<Value>,
) -> Response {
    send_with_key(app, method, path, uid, Some(Uuid::new_v4()), body).await
}

async fn send_with_key(
    app: &Router,
    method: &str,
    path: &str,
    uid: &str,
    key: Option<Uuid>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-dev-firebase-uid", uid)
        .header("content-type", "application/json");

    if let Some(key) = key {
        builder = builder.header("idempotency-key", key.to_string());
    }

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

    Response {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

fn uid(label: &str) -> String {
    format!("t2-{label}-{}", Uuid::new_v4().simple())
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// An active store with a published policy, plus a consumer.
struct Scenario {
    owner_uid: String,
    customer_uid: String,
    store_id: Uuid,
    customer_user_id: Uuid,
    policy_id: Uuid,
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
        response
            .expect_ok("bootstrap")["id"]
            .as_str()
            .expect("user id"),
    )
    .expect("uuid")
}

/// Approving a store is Phase 3's `admin` endpoint, so the tests move it to ACTIVE
/// directly. Everything under test still goes through the API.
async fn activate_store(pool: &PgPool, store_id: Uuid) {
    sqlx::query(
        "UPDATE coupon.stores SET status = 'ACTIVE', activated_at = clock_timestamp() WHERE id = $1",
    )
    .bind(store_id)
    .execute(pool)
    .await
    .expect("activate the store");
}

async fn grant_role(pool: &PgPool, user_id: Uuid, role: &str) {
    sqlx::query(
        "INSERT INTO coupon.user_roles (user_id, role) VALUES ($1, $2::text::coupon.account_role)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await
    .expect("grant the role");
}

async fn scenario(harness: &Harness, label: &str, rules: Value) -> Scenario {
    let owner_uid = uid(&format!("{label}-owner"));
    let customer_uid = uid(&format!("{label}-customer"));

    bootstrap(&harness.app, &owner_uid, "점주").await;
    let customer_user_id = bootstrap(&harness.app, &customer_uid, "김손님").await;

    let store = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({
            "name": "테스트 베이커리",
            "slug": format!("t2-{}", Uuid::new_v4().simple()),
        })),
    )
    .await;
    let store_id =
        Uuid::parse_str(store.expect_ok("create store")["id"].as_str().expect("id")).expect("uuid");
    activate_store(&harness.pool, store_id).await;

    let policy = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        &owner_uid,
        Some(json!({
            "name": "기본 도장 정책",
            "rules": rules,
            "reward": {
                "benefit_type": "FIXED_AMOUNT",
                "fixed_amount": 3000,
                "free_item_ids": [],
                "minimum_order_amount": 0,
                "validity_days": 30,
                "title": "3,000원 할인 쿠폰",
                "description": "1만원 이상 주문 시 사용 가능",
                "customer_notice": "다른 할인과 중복 사용 불가",
            },
        })),
    )
    .await;
    let policy_id =
        Uuid::parse_str(policy.expect_ok("draft policy")["id"].as_str().expect("id")).expect("uuid");

    let published = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish"),
        &owner_uid,
        Some(json!({})),
    )
    .await;
    assert_eq!(published.expect_ok("publish")["status"], "ACTIVE");

    Scenario {
        owner_uid,
        customer_uid,
        store_id,
        customer_user_id,
        policy_id,
    }
}

fn default_rules() -> Value {
    json!({
        "target_stamp_count": 3,
        "stamps_per_order": 1,
        "minimum_order_amount": 0,
        "daily_earning_limit": null,
        "duplicate_warning_minutes": 1,
        "stamp_validity_days": 180,
        "eligible_item_ids": [],
        "eligible_category_ids": [],
        "excluded_item_ids": [],
    })
}

/// Issue a QR for the customer and return the signed token.
async fn issue_qr(app: &Router, customer_uid: &str) -> (String, String) {
    let response = send(app, "POST", "/api/coupon/v1/me/qr-tokens", customer_uid, None).await;
    let data = response.expect_ok("issue qr");

    (
        data["token"].as_str().expect("token").to_owned(),
        data["fallback_code"].as_str().expect("code").to_owned(),
    )
}

fn order(amount: i64) -> Value {
    json!({ "gross_amount": amount, "currency": "KRW", "items": [] })
}

// ---------------------------------------------------------------------------
// STAMP-002 / STAMP-004: the happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_scan_resolves_previews_and_confirms_into_one_ledger_entry() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "happy", default_rules()).await;
    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;

    // Resolve does not consume the QR — the owner may still walk away.
    let resolved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/scan/resolve",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token })),
    )
    .await;
    let customer = &resolved.expect_ok("resolve")["customer"];
    assert_eq!(customer["masked_name"], "김**", "WALLET-005");
    assert_eq!(customer["is_new_customer"], true);
    assert_eq!(resolved.data()["stamp_board"]["available"], 0);
    assert_eq!(resolved.data()["policy"]["target_stamp_count"], 3);

    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions/preview",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    let preview_data = preview.expect_ok("preview");
    assert_eq!(preview_data["approvable"], true);
    assert_eq!(preview_data["expected_stamps"], 1);
    assert_eq!(preview_data["rewards_to_issue"], 0);

    let confirmed = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": token,
            "preview_id": preview_data["preview_id"],
            "order": order(12_000),
        })),
    )
    .await;
    assert_eq!(confirmed.status, StatusCode::CREATED, "{}", confirmed.json);
    let transaction_id = Uuid::parse_str(
        confirmed.data()["transaction_id"]
            .as_str()
            .expect("transaction id"),
    )
    .expect("uuid");
    assert_eq!(confirmed.data()["stamp_board"]["available"], 1);
    assert_eq!(confirmed.data()["stamp_board"]["remaining_to_goal"], 2);

    // Exactly one EARN row, and the QR is now spent.
    let earned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(quantity_delta), 0)::bigint FROM coupon.stamp_ledger
         WHERE source_stamp_transaction_id = $1 AND event_type = 'EARN'",
    )
    .bind(transaction_id)
    .fetch_one(&harness.pool)
    .await
    .expect("ledger sum");
    assert_eq!(earned, 1);

    let replay = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    assert_eq!(replay.status, StatusCode::CONFLICT);
    assert_eq!(replay.error_code(), "QR_ALREADY_USED");
}

#[tokio::test]
async fn reaching_the_goal_issues_a_reward_in_the_same_transaction() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "goal", default_rules()).await;

    let mut last = Value::Null;
    for round in 1..=3 {
        let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/stamp-transactions",
            &scenario.owner_uid,
            // A distinct amount each round keeps the STAMP-003 duplicate guard out of it.
            Some(json!({ "qr_token": token, "order": order(10_000 + round * 100) })),
        )
        .await;
        last = response.expect_ok("confirm").clone();
    }

    // STAMP-004: the third stamp completes the board and pays out immediately.
    assert_eq!(last["issued_rewards"].as_array().expect("rewards").len(), 1);
    assert_eq!(last["stamp_board"]["available"], 0, "the board was spent");

    let coupon_id = Uuid::parse_str(last["issued_rewards"][0]["coupon_id"].as_str().expect("id"))
        .expect("uuid");

    // The consumption is recorded against the coupon it paid for, and the lot balances
    // sum to zero rather than the stamps simply disappearing (§12.6-3).
    let consumed: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(-quantity_delta), 0)::bigint FROM coupon.stamp_ledger
         WHERE reward_coupon_id = $1 AND event_type = 'CONSUME_FOR_REWARD'",
    )
    .bind(coupon_id)
    .fetch_one(&harness.pool)
    .await
    .expect("consumption");
    assert_eq!(consumed, 3);

    let wallet = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/coupons?status=AVAILABLE",
        &scenario.customer_uid,
        None,
    )
    .await;
    let items = wallet.expect_ok("wallet")["items"]
        .as_array()
        .expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["effective_status"], "AVAILABLE");
    assert_eq!(items[0]["title"], "3,000원 할인 쿠폰");

    let detail = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/me/wallet/coupons/{coupon_id}"),
        &scenario.customer_uid,
        None,
    )
    .await;
    let detail = detail.expect_ok("coupon detail");
    assert_eq!(detail["condition_snapshot"]["policy"]["id"], scenario.policy_id.to_string());
    assert_eq!(detail["history"][0]["to_status"], "AVAILABLE");
}

// ---------------------------------------------------------------------------
// §19.2: concurrency
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_qr_scanned_a_hundred_times_at_once_produces_one_ledger_entry() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "race-qr", default_rules()).await;
    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;

    let attempts = (0..100).map(|_| {
        let app = harness.app.clone();
        let owner_uid = scenario.owner_uid.clone();
        let token = token.clone();
        async move {
            send(
                &app,
                "POST",
                "/api/coupon/v1/owner/stamp-transactions",
                &owner_uid,
                // Distinct keys on purpose: idempotency must not be what saves us here.
                // The nonce has to.
                Some(json!({ "qr_token": token, "order": order(12_000) })),
            )
            .await
        }
    });

    let results = futures_join(attempts).await;

    let created = results
        .iter()
        .filter(|response| response.status == StatusCode::CREATED)
        .count();
    assert_eq!(created, 1, "§12.6-7: a nonce backs at most one transaction");

    for response in results.iter().filter(|r| r.status != StatusCode::CREATED) {
        assert_eq!(
            response.error_code(),
            "QR_ALREADY_USED",
            "the losers must be told the QR is spent, not something vague: {}",
            response.json
        );
    }

    let ledger: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_ledger e
         JOIN coupon.stamp_lots l ON l.id = e.lot_id
         WHERE l.store_id = $1 AND l.user_id = $2",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("ledger count");
    assert_eq!(ledger, 1, "exactly one ledger row exists");

    let transactions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_transactions WHERE store_id = $1 AND user_id = $2",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("transaction count");
    assert_eq!(transactions, 1);
}

#[tokio::test]
async fn the_same_key_replays_and_a_different_body_conflicts() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "idem", default_rules()).await;
    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;

    let key = Uuid::new_v4();
    let body = json!({ "qr_token": token, "order": order(12_000) });

    let first = send_with_key(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(key),
        Some(body.clone()),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.json);

    let replay = send_with_key(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(key),
        Some(body),
    )
    .await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(
        replay.data()["transaction_id"],
        first.data()["transaction_id"],
        "an identical retry returns the original transaction, not a new one"
    );

    // §12.6-9: same key, different body.
    let reused = send_with_key(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(key),
        Some(json!({ "qr_token": token, "order": order(99_000) })),
    )
    .await;
    assert_eq!(reused.status, StatusCode::CONFLICT);
    assert_eq!(reused.error_code(), "IDEMPOTENCY_KEY_REUSED");

    let ledger: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_ledger e
         JOIN coupon.stamp_lots l ON l.id = e.lot_id
         WHERE l.store_id = $1 AND l.user_id = $2",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("ledger count");
    assert_eq!(ledger, 1, "three requests, one accrual");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_void_racing_a_new_accrual_leaves_a_consistent_ledger() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "race-void", default_rules()).await;

    // One accrual to reverse.
    let (first_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let first = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": first_token, "order": order(11_000) })),
    )
    .await;
    let transaction_id = first.expect_ok("first accrual")["transaction_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // A second QR ready to be accrued at the same moment the first is reversed.
    let (second_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;

    let void_app = harness.app.clone();
    let void_owner = scenario.owner_uid.clone();
    let void_path = format!("/api/coupon/v1/owner/stamp-transactions/{transaction_id}/void");
    let void = async move {
        send(
            &void_app,
            "POST",
            &void_path,
            &void_owner,
            Some(json!({ "reason": "손님 요청" })),
        )
        .await
    };

    let earn_app = harness.app.clone();
    let earn_owner = scenario.owner_uid.clone();
    let earn = async move {
        send(
            &earn_app,
            "POST",
            "/api/coupon/v1/owner/stamp-transactions",
            &earn_owner,
            Some(json!({ "qr_token": second_token, "order": order(13_000) })),
        )
        .await
    };

    let (void_result, earn_result) = tokio::join!(void, earn);
    assert!(
        void_result.status.is_success(),
        "the reversal must succeed: {}",
        void_result.json
    );
    assert!(
        earn_result.status.is_success(),
        "the second accrual must succeed: {}",
        earn_result.json
    );

    // One reversed accrual and one live one: net one stamp, whichever order they ran in.
    let available: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(balance), 0)::bigint FROM coupon.stamp_lot_balances
         WHERE store_id = $1 AND user_id = $2 AND expires_at > clock_timestamp()",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("balance");
    assert_eq!(available, 1);
}

#[tokio::test]
async fn a_lot_can_never_be_consumed_past_what_it_earned() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "invariant", default_rules()).await;

    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let confirmed = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    let transaction_id =
        Uuid::parse_str(confirmed.expect_ok("confirm")["transaction_id"].as_str().expect("id"))
            .expect("uuid");

    let lot_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM coupon.stamp_lots WHERE source_transaction_id = $1",
    )
    .bind(transaction_id)
    .fetch_one(&harness.pool)
    .await
    .expect("lot");

    // §12.6-3 is a database invariant, not an application convention: writing straight to
    // the ledger must still be refused.
    let over_consumed = sqlx::query(
        "INSERT INTO coupon.stamp_ledger
            (lot_id, event_type, quantity_delta, actor_type, reason_code)
         VALUES ($1, 'EXPIRE', -5, 'SYSTEM', 'TEST_OVER_CONSUMPTION')",
    )
    .bind(lot_id)
    .execute(&harness.pool)
    .await;

    let error = over_consumed.expect_err("the balance trigger must reject this");
    assert!(
        error.to_string().contains("outside 0"),
        "unexpected failure: {error}"
    );

    let balance: i64 = sqlx::query_scalar(
        "SELECT balance::bigint FROM coupon.stamp_lot_balances WHERE lot_id = $1",
    )
    .bind(lot_id)
    .fetch_one(&harness.pool)
    .await
    .expect("balance");
    assert_eq!(balance, 1, "the rejected write left nothing behind");
}

// ---------------------------------------------------------------------------
// STAMP-005 / STAMP-003 / STAMP-007
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_daily_limit_blocks_the_second_accrual_and_names_the_next_business_day() {
    let harness = harness_or_skip!();
    let rules = json!({
        "target_stamp_count": 10,
        "stamps_per_order": 1,
        "minimum_order_amount": 0,
        "daily_earning_limit": 1,
        "duplicate_warning_minutes": 1,
        "stamp_validity_days": 180,
        "eligible_item_ids": [],
        "eligible_category_ids": [],
        "excluded_item_ids": [],
    });
    let scenario = scenario(&harness, "daily", rules).await;

    let (first_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": first_token, "order": order(10_000) })),
    )
    .await
    .expect_ok("first accrual");

    let (second_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions/preview",
        &scenario.owner_uid,
        Some(json!({ "qr_token": second_token, "order": order(20_000) })),
    )
    .await;
    let preview = preview.expect_ok("preview");

    assert_eq!(preview["approvable"], false);
    assert_eq!(preview["daily_used"], 1);
    assert_eq!(preview["blockers"][0]["code"], "DAILY_LIMIT_EXCEEDED");
    assert!(
        preview["next_business_day_start"].is_string(),
        "STAMP-005 tells the owner when the customer may come back"
    );

    let refused = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": second_token, "order": order(20_000) })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.error_code(), "DAILY_LIMIT_EXCEEDED");
}

#[tokio::test]
async fn a_minimum_order_below_the_threshold_is_refused_with_the_shortfall() {
    let harness = harness_or_skip!();
    let rules = json!({
        "target_stamp_count": 10,
        "stamps_per_order": 1,
        "minimum_order_amount": 10_000,
        "daily_earning_limit": null,
        "duplicate_warning_minutes": 1,
        "stamp_validity_days": 180,
        "eligible_item_ids": [],
        "eligible_category_ids": [],
        "excluded_item_ids": [],
    });
    let scenario = scenario(&harness, "minimum", rules).await;
    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;

    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions/preview",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token, "order": order(7_500) })),
    )
    .await;
    let preview = preview.expect_ok("preview");

    assert_eq!(preview["approvable"], false);
    assert_eq!(preview["blockers"][0]["code"], "MINIMUM_ORDER_NOT_MET");
    assert!(
        preview["blockers"][0]["message"]
            .as_str()
            .expect("message")
            .contains("2500"),
        "STAMP-005 shows the shortfall: {}",
        preview["blockers"][0]["message"]
    );
}

#[tokio::test]
async fn a_near_duplicate_needs_a_distinct_order_reference_to_go_through() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "dup", default_rules()).await;

    let (first_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": first_token,
            "order": { "gross_amount": 12_000, "currency": "KRW", "items": [],
                       "external_order_ref": "POS-1" },
        })),
    )
    .await
    .expect_ok("first accrual");

    // STAMP-003: same customer, same amount, inside the window.
    let (second_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let refused = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": second_token,
            "order": { "gross_amount": 12_000, "currency": "KRW", "items": [],
                       "external_order_ref": "POS-1" },
        })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.error_code(), "DUPLICATE_TRANSACTION_SUSPECTED");

    // The owner confirms it really is a separate order, with its own reference.
    let accepted = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": second_token,
            "acknowledge_duplicate": true,
            "order": { "gross_amount": 12_000, "currency": "KRW", "items": [],
                       "external_order_ref": "POS-2" },
        })),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::CREATED, "{}", accepted.json);
    assert_eq!(accepted.data()["stamp_board"]["available"], 2);
}

#[tokio::test]
async fn reversing_an_accrual_takes_back_the_reward_and_restores_the_stamps() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "void", default_rules()).await;

    let mut transactions = Vec::new();
    let mut last = Value::Null;
    for round in 1..=3 {
        let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/stamp-transactions",
            &scenario.owner_uid,
            Some(json!({ "qr_token": token, "order": order(10_000 + round * 100) })),
        )
        .await;
        let data = response.expect_ok("confirm").clone();
        transactions.push(data["transaction_id"].as_str().expect("id").to_owned());
        last = data;
    }

    let coupon_id = last["issued_rewards"][0]["coupon_id"]
        .as_str()
        .expect("reward")
        .to_owned();

    // Reverse the accrual that completed the board.
    let voided = send(
        &harness.app,
        "POST",
        &format!(
            "/api/coupon/v1/owner/stamp-transactions/{}/void",
            transactions[2]
        ),
        &scenario.owner_uid,
        Some(json!({ "reason": "점주 실수" })),
    )
    .await;
    let voided = voided.expect_ok("void");

    assert_eq!(voided["status"], "VOIDED");
    assert_eq!(voided["revoked_reward_ids"][0], coupon_id);
    // STAMP-007: the two stamps the reward consumed come back; the one this transaction
    // granted does not.
    assert_eq!(voided["stamp_board"]["available"], 2);

    let coupon: String = sqlx::query_scalar(
        "SELECT status::text FROM coupon.coupon_instances WHERE id = $1",
    )
    .bind(Uuid::parse_str(&coupon_id).expect("uuid"))
    .fetch_one(&harness.pool)
    .await
    .expect("coupon status");
    assert_eq!(coupon, "REVOKED");

    // Nothing was deleted: the original EARN row is still there next to its reversal.
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_ledger WHERE source_stamp_transaction_id = $1",
    )
    .bind(Uuid::parse_str(&transactions[2]).expect("uuid"))
    .fetch_one(&harness.pool)
    .await
    .expect("ledger rows");
    assert!(rows >= 4, "EARN + 3 consumptions + reversals, got {rows}");

    // And a second reversal of the same transaction is refused.
    let again = send(
        &harness.app,
        "POST",
        &format!(
            "/api/coupon/v1/owner/stamp-transactions/{}/void",
            transactions[2]
        ),
        &scenario.owner_uid,
        Some(json!({ "reason": "again" })),
    )
    .await;
    assert_eq!(again.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(again.error_code(), "INVALID_STATE_TRANSITION");
}

#[tokio::test]
async fn approvals_are_rate_limited_per_store_and_owner() {
    // §16.4: 적립/사용 승인 30회/분, keyed by store+owner. Two per minute here so the
    // ceiling is reached without firing thirty requests.
    let harness = harness_or_skip!(json!({ "rate_limit_stamp_approval_per_min": 2 }));
    let scenario = scenario(&harness, "ratelimit", default_rules()).await;

    let mut outcomes = Vec::new();
    for round in 0..4 {
        let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/stamp-transactions",
            &scenario.owner_uid,
            Some(json!({ "qr_token": token, "order": order(10_000 + round * 500) })),
        )
        .await;
        outcomes.push(response);
    }

    assert_eq!(outcomes[0].status, StatusCode::CREATED, "{}", outcomes[0].json);
    assert_eq!(outcomes[1].status, StatusCode::CREATED, "{}", outcomes[1].json);
    assert_eq!(outcomes[2].status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(outcomes[2].error_code(), "RATE_LIMITED");
    assert_eq!(
        outcomes[2].json["error"]["retryable"], true,
        "the owner should try again shortly, not give up"
    );

    // The refused approvals wrote nothing.
    let ledger: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_ledger e
         JOIN coupon.stamp_lots l ON l.id = e.lot_id
         WHERE l.store_id = $1 AND l.user_id = $2",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("ledger count");
    assert_eq!(ledger, 2);
}

// ---------------------------------------------------------------------------
// STORE-005 / SEC-002 / WALLET-004
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_manual_code_works_when_the_camera_does_not() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "manual", default_rules()).await;
    let (_, code) = issue_qr(&harness.app, &scenario.customer_uid).await;

    assert_eq!(code.len(), 8);

    let confirmed = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "fallback_code": code, "order": order(12_000) })),
    )
    .await;
    assert_eq!(confirmed.status, StatusCode::CREATED, "{}", confirmed.json);

    // STORE-005: the code carries the same nonce, so it is spent too.
    let reused = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "fallback_code": code, "order": order(12_000) })),
    )
    .await;
    assert_eq!(reused.error_code(), "QR_ALREADY_USED");
}

#[tokio::test]
async fn forged_and_unknown_tokens_are_refused_without_explaining_why() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "forged", default_rules()).await;

    for forged in [
        "not-a-token",
        "aaaa.bbbb.cccc",
        // A structurally valid token signed by nobody.
        "eyJhbGciOiJFZERTQSIsInR5cCI6IkRRUiIsImtpZCI6ImRlYWRiZWVmIn0.e30.AAAA",
    ] {
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/scan/resolve",
            &scenario.owner_uid,
            Some(json!({ "qr_token": forged })),
        )
        .await;

        assert_eq!(
            response.error_code(),
            "QR_TOKEN_INVALID",
            "every forgery must look the same from outside (SEC-002): {}",
            response.json
        );
        let message = response.json["error"]["message"].as_str().unwrap_or_default();
        assert!(
            !message.contains("서명") && !message.contains("kid"),
            "the message must not describe the failure: {message}"
        );
    }
}

#[tokio::test]
async fn two_devices_hold_independent_nonces() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "devices", default_rules()).await;

    // WALLET-004: the same member makes a QR on two devices.
    let (phone_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let (tablet_token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    assert_ne!(phone_token, tablet_token);

    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": phone_token, "order": order(12_000) })),
    )
    .await
    .expect_ok("first device");

    // The other nonce is still alive — issuing a new QR does not kill the old one.
    let second = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/scan/resolve",
        &scenario.owner_uid,
        Some(json!({ "qr_token": tablet_token })),
    )
    .await;
    assert!(
        second.status.is_success(),
        "the second nonce must survive the first being spent: {}",
        second.json
    );
}

// ---------------------------------------------------------------------------
// Policy versioning (STAMP-008) and the catalogue (§8.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_active_policy_is_replaced_by_a_new_version_not_edited() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "versions", default_rules()).await;

    // STAMP-008: 활성 정책의 목표 수는 직접 변경하지 않는다.
    let refused = send(
        &harness.app,
        "PATCH",
        &format!(
            "/api/coupon/v1/owner/loyalty-policies/{}",
            scenario.policy_id
        ),
        &scenario.owner_uid,
        Some(json!({ "name": "몰래 수정" })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.error_code(), "POLICY_NOT_EDITABLE");

    let next = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        &scenario.owner_uid,
        Some(json!({
            "name": "가을 정책",
            "rules": { "target_stamp_count": 5, "stamps_per_order": 1,
                       "minimum_order_amount": 0, "daily_earning_limit": null,
                       "duplicate_warning_minutes": 5, "stamp_validity_days": 180,
                       "eligible_item_ids": [], "eligible_category_ids": [],
                       "excluded_item_ids": [] },
            "reward": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 5000,
                        "free_item_ids": [], "minimum_order_amount": 0, "validity_days": 30,
                        "title": "5,000원 할인", "description": "조건", "customer_notice": "고지" },
        })),
    )
    .await;
    let next_id = next.expect_ok("second draft")["id"].as_str().expect("id").to_owned();
    assert_eq!(next.data()["version_no"], 2);

    let published = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{next_id}/publish"),
        &scenario.owner_uid,
        Some(json!({})),
    )
    .await;
    assert_eq!(published.expect_ok("publish")["status"], "ACTIVE");

    // §12.6-2: exactly one active version, and the old one is retired rather than deleted.
    let listed = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/owner/loyalty-policies",
        &scenario.owner_uid,
        None,
    )
    .await;
    let listed = listed.expect_ok("list");
    assert_eq!(listed["active_policy_id"], next_id);

    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.loyalty_policies WHERE store_id = $1 AND status = 'ACTIVE'",
    )
    .bind(scenario.store_id)
    .fetch_one(&harness.pool)
    .await
    .expect("active count");
    assert_eq!(active, 1);

    let previous = listed["policies"]
        .as_array()
        .expect("policies")
        .iter()
        .find(|policy| policy["id"] == scenario.policy_id.to_string())
        .expect("the first version is still listed");
    assert_eq!(previous["status"], "ENDED");
}

#[tokio::test]
async fn a_policy_cannot_go_live_without_the_wording_a_customer_reads() {
    let harness = harness_or_skip!();
    let owner_uid = uid("wording-owner");
    bootstrap(&harness.app, &owner_uid, "점주").await;

    let store = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({ "name": "카페", "slug": format!("t2-{}", Uuid::new_v4().simple()) })),
    )
    .await;
    let store_id =
        Uuid::parse_str(store.expect_ok("store")["id"].as_str().expect("id")).expect("uuid");
    activate_store(&harness.pool, store_id).await;

    let draft = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        &owner_uid,
        Some(json!({
            "name": "미완성 정책",
            "reward": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 3000,
                        "free_item_ids": [], "minimum_order_amount": 0, "validity_days": 30,
                        "title": "", "description": "", "customer_notice": "" },
        })),
    )
    .await;
    // STAMP-001: a draft may be incomplete.
    let draft_id = draft.expect_ok("draft")["id"].as_str().expect("id").to_owned();

    let refused = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{draft_id}/publish"),
        &owner_uid,
        Some(json!({})),
    )
    .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{}", refused.json);
    let fields: Vec<&str> = refused.json["error"]["field_errors"]
        .as_array()
        .expect("field errors")
        .iter()
        .map(|error| error["field"].as_str().unwrap_or_default())
        .collect();
    assert!(fields.contains(&"reward.customer_notice"), "{fields:?}");
}

#[tokio::test]
async fn an_item_restriction_is_judged_against_the_order_lines() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "items", default_rules()).await;

    let coffee = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/catalog/items",
        &scenario.owner_uid,
        Some(json!({ "name": "아메리카노", "reference_price": 4500 })),
    )
    .await;
    let coffee_id = coffee.expect_ok("item")["id"].as_str().expect("id").to_owned();

    let cake = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/catalog/items",
        &scenario.owner_uid,
        Some(json!({ "name": "케이크", "reference_price": 6000 })),
    )
    .await;
    let cake_id = cake.expect_ok("item")["id"].as_str().expect("id").to_owned();

    // A new version restricted to coffee.
    let draft = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        &scenario.owner_uid,
        Some(json!({
            "name": "커피 전용",
            "rules": { "target_stamp_count": 10, "stamps_per_order": 1,
                       "minimum_order_amount": 0, "daily_earning_limit": null,
                       "duplicate_warning_minutes": 5, "stamp_validity_days": 180,
                       "eligible_item_ids": [coffee_id], "eligible_category_ids": [],
                       "excluded_item_ids": [] },
            "reward": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 3000,
                        "free_item_ids": [], "minimum_order_amount": 0, "validity_days": 30,
                        "title": "커피 무료", "description": "조건", "customer_notice": "고지" },
        })),
    )
    .await;
    let draft_id = draft.expect_ok("draft")["id"].as_str().expect("id").to_owned();
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{draft_id}/publish"),
        &scenario.owner_uid,
        Some(json!({})),
    )
    .await
    .expect_ok("publish");

    // An order of cake alone does not qualify.
    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let refused = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": token,
            "order": { "gross_amount": 6_000, "currency": "KRW", "items": [
                { "catalog_item_id": cake_id, "quantity": 1, "unit_price": 6000 }
            ]},
        })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.error_code(), "ITEM_NOT_ELIGIBLE");

    // The same order with a coffee on it does.
    let accepted = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": token,
            "order": { "gross_amount": 10_500, "currency": "KRW", "items": [
                { "catalog_item_id": cake_id, "quantity": 1, "unit_price": 6000 },
                { "catalog_item_id": coffee_id, "quantity": 1, "unit_price": 4500 }
            ]},
        })),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::CREATED, "{}", accepted.json);

    // §8.3: a deactivated item stays in the snapshot but cannot be named by a new policy.
    send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/owner/catalog/items/{coffee_id}"),
        &scenario.owner_uid,
        Some(json!({ "status": "INACTIVE" })),
    )
    .await
    .expect_ok("deactivate");

    let rejected = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        &scenario.owner_uid,
        Some(json!({
            "name": "비활성 품목 정책",
            "rules": { "target_stamp_count": 10, "stamps_per_order": 1,
                       "minimum_order_amount": 0, "daily_earning_limit": null,
                       "duplicate_warning_minutes": 5, "stamp_validity_days": 180,
                       "eligible_item_ids": [coffee_id], "eligible_category_ids": [],
                       "excluded_item_ids": [] },
        })),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::NOT_FOUND);
    assert_eq!(rejected.error_code(), "CATALOG_ITEM_NOT_FOUND");
}

#[tokio::test]
async fn an_expired_stamp_stops_counting_the_moment_it_expires() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "expiry", default_rules()).await;

    // Two accruals, then push the first one's lot into the past.
    let mut lots = Vec::new();
    for round in 1..=2 {
        let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/stamp-transactions",
            &scenario.owner_uid,
            Some(json!({ "qr_token": token, "order": order(10_000 + round * 250) })),
        )
        .await;
        lots.push(
            Uuid::parse_str(response.expect_ok("confirm")["transaction_id"].as_str().expect("id"))
                .expect("uuid"),
        );
    }

    // Both timestamps move: `ck_stamp_lot_period` requires expiry to follow earning, so a
    // lot that expired an hour ago must have been earned before that.
    sqlx::query(
        "UPDATE coupon.stamp_lots
         SET earned_at = clock_timestamp() - interval '2 hours',
             expires_at = clock_timestamp() - interval '1 hour'
         WHERE source_transaction_id = $1",
    )
    .bind(lots[0])
    .execute(&harness.pool)
    .await
    .expect("backdate the lot");

    // STAMP-006 / §18.1: the online judgement is immediate, with no sweep having run.
    let wallet = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/stamps",
        &scenario.customer_uid,
        None,
    )
    .await;
    assert_eq!(
        wallet.expect_ok("wallet")["total_available"],
        1,
        "the expired stamp must not still be counted"
    );

    // The ledger has not been touched yet — the balance is still one per lot, and it is
    // the expiry comparison alone that excludes it.
    let raw_balance: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(balance), 0)::bigint FROM coupon.stamp_lot_balances
         WHERE store_id = $1 AND user_id = $2",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("raw balance");
    assert_eq!(raw_balance, 2);

    // The sweep then tidies the state up, and is idempotent when run again.
    let service = &harness.state.loyalty_stamps;

    let now = chrono::Utc::now();
    let expired = service
        .expire_due_lots(&harness.pool, now, 500)
        .await
        .expect("sweep");
    assert!(expired >= 1);

    let after: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(balance), 0)::bigint FROM coupon.stamp_lot_balances
         WHERE store_id = $1 AND user_id = $2",
    )
    .bind(scenario.store_id)
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("balance after sweep");
    assert_eq!(after, 1, "the sweep wrote the EXPIRE row the reads already assumed");

    let again = service
        .expire_due_lots(&harness.pool, now, 500)
        .await
        .expect("second sweep");
    assert_eq!(again, 0, "a repeated sweep must not double-expire anything");
}

// ---------------------------------------------------------------------------
// Wallet and admin
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_wallet_shows_the_board_and_hides_nothing_from_the_owner_of_it() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "wallet", default_rules()).await;

    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await
    .expect_ok("accrue");

    let stamps = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/stamps",
        &scenario.customer_uid,
        None,
    )
    .await;
    let stamps = stamps.expect_ok("wallet stamps");

    assert_eq!(stamps["total_available"], 1);
    let board = &stamps["boards"][0];
    assert_eq!(board["store_id"], scenario.store_id.to_string());
    assert_eq!(board["available"], 1);
    assert_eq!(board["target"], 3);
    assert_eq!(board["remaining_to_goal"], 2);
    assert!(board["earliest_expiry"].is_string());
    assert_eq!(board["reward_title"], "3,000원 할인 쿠폰");

    // SEC-001: another member cannot read this one's wallet.
    let stranger = uid("stranger");
    bootstrap(&harness.app, &stranger, "남").await;
    let empty = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/stamps",
        &stranger,
        None,
    )
    .await;
    assert_eq!(empty.expect_ok("stranger wallet")["total_available"], 0);
}

#[tokio::test]
async fn an_administrator_can_follow_one_transaction_end_to_end() {
    let harness = harness_or_skip!();
    let scenario = scenario(&harness, "admin", default_rules()).await;

    let (token, _) = issue_qr(&harness.app, &scenario.customer_uid).await;
    let confirmed = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    let transaction_id = confirmed.expect_ok("confirm")["transaction_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // An ordinary member is not an administrator.
    let forbidden = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/admin/transactions/{transaction_id}"),
        &scenario.owner_uid,
        None,
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.error_code(), "ROLE_REQUIRED");

    let admin_uid = uid("operator");
    let admin_user_id = bootstrap(&harness.app, &admin_uid, "운영자").await;
    grant_role(&harness.pool, admin_user_id, "OPERATIONS").await;

    let detail = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/admin/transactions/{transaction_id}"),
        &admin_uid,
        None,
    )
    .await;
    let detail = detail.expect_ok("admin detail");

    assert_eq!(detail["status"], "CONFIRMED");
    assert_eq!(detail["customer_masked_name"], "김**");
    assert_eq!(detail["ledger"].as_array().expect("ledger").len(), 1);
    let timeline = detail["timeline"].as_array().expect("timeline");
    assert!(
        timeline.iter().any(|event| event["source"] == "qr"),
        "the QR that authorised it is part of the story: {timeline:?}"
    );
    assert!(timeline.iter().any(|event| event["source"] == "ledger"));

    // §11.5: an administrative read is itself audited.
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.audit_logs
         WHERE resource_id = $1 AND action = 'stamp_transaction.viewed' AND actor_user_id = $2",
    )
    .bind(Uuid::parse_str(&transaction_id).expect("uuid"))
    .bind(admin_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("audit count");
    assert_eq!(audited, 1);

    // ADMIN-003: a correction is simulated against a repeatable-read snapshot first.
    let case_id: Uuid = sqlx::query_scalar(
        "INSERT INTO coupon.admin_cases (case_type, title, description)
         VALUES ('WRONG_STAMP', '오적립 문의', '고객 문의') RETURNING id",
    )
    .fetch_one(&harness.pool)
    .await
    .expect("case");

    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/adjustments/preview",
        &admin_uid,
        Some(json!({
            "case_id": case_id,
            "adjustment_type": "STAMP_TRANSACTION_VOID",
            "transaction_id": transaction_id,
            "reason": "점주 오적립 확인",
        })),
    )
    .await;
    let preview = preview.expect_ok("adjustment preview");

    assert_eq!(preview["executable"], true);
    assert_eq!(preview["stamps_before"], 1);
    assert_eq!(preview["stamps_after"], 0);
    assert!(
        preview["preview_expires_at"].is_string(),
        "§13.4 requires the preview to expire"
    );
    assert!(
        preview["observed_versions"]["stamp_transactions"]
            .as_object()
            .is_some_and(|versions| !versions.is_empty()),
        "execution must be able to detect that the row moved: {}",
        preview["observed_versions"]
    );
    assert!(
        !preview["ledger_entries_to_append"]
            .as_array()
            .expect("entries")
            .is_empty(),
        "a correction appends events; it never rewrites them"
    );
}

/// Run futures concurrently without pulling in an extra dependency.
async fn futures_join<F>(futures: impl IntoIterator<Item = F>) -> Vec<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handles: Vec<_> = futures.into_iter().map(tokio::spawn).collect();
    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(handle.await.expect("task did not panic"));
    }
    results
}
