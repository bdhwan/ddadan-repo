//! Shared harness for the Phase 5 release-verification suites (§19.2).
//!
//! Phase 1–4 each grew their own copy of this scaffolding inside a single test binary.
//! Phase 5 asks the same questions across four of them — security, resilience, the §12.6
//! invariants and the §18.1 budgets — so the scaffolding moves here rather than being
//! pasted four more times. The Phase 1–4 binaries are left exactly as they are: they pass,
//! and rewriting a passing suite to share a module buys nothing.
//!
//! ```sh
//! ./scripts/coupon/db-up.sh
//! ./scripts/coupon/test.sh
//! ```
//!
//! Without `COUPON_TEST_DATABASE_URL` every test here skips with a visible note rather
//! than passing silently.

// Each test binary uses a different slice of this module, and the compiler only sees the
// slice it was compiled into.
#![allow(dead_code)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::jobs::transport::RegistryOnlyTransport;
use coupon_api_server::jobs::worker::JobRuntime;
use coupon_api_server::state::{AppState, RedisHandle};
use coupon_api_server::{db, http};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

pub struct Harness {
    pub app: Router,
    pub pool: PgPool,
    pub state: AppState,
}

impl Harness {
    /// A worker driving the registry directly, as `campaigns.rs` does: §14.2 makes Redis
    /// transport only, so running without it exercises exactly the durable layer.
    pub fn runtime(&self) -> JobRuntime {
        JobRuntime::new(self.state.clone(), Arc::new(RegistryOnlyTransport))
    }
}

/// Build a harness against `COUPON_TEST_DATABASE_URL`, or `None` when it is unset.
///
/// The §16.4 rate limits are lifted by default. Where a test is *about* a limit it
/// overrides the one it cares about, and the rest stay out of the way — otherwise an
/// unrelated loop of a hundred requests would be refused for being the hundredth.
pub async fn harness_with(overrides: Value) -> Option<Harness> {
    harness_with_redis(overrides, None).await
}

/// As [`harness_with`], but wiring a Redis handle built from `redis_url`.
///
/// `redis::Client::open` only parses; it connects lazily. That is what makes a *broken*
/// Redis testable without stopping a container: point the handle at a port nothing listens
/// on and every use of it fails the way a Redis outage fails (§18.2, JOB-005).
pub async fn harness_with_redis(overrides: Value, redis_url: Option<&str>) -> Option<Harness> {
    let database_url = std::env::var("COUPON_TEST_DATABASE_URL").ok()?;

    let mut settings = json!({
        "env": "test",
        "database_url": database_url,
        "firebase_project_id": "ddadan-test",
        "auth_dev_bypass": true,
        "database_max_connections": 32,
        "rate_limit_qr_issue_per_min": 100_000,
        "rate_limit_qr_resolve_failure_per_min": 100_000,
        "rate_limit_stamp_approval_per_min": 100_000,
        "rate_limit_campaign_claim_per_min": 100_000,
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
    let redis = redis_url.map(|url| RedisHandle {
        client: redis::Client::open(url).expect("a well-formed redis url"),
    });
    let state = AppState::new(Arc::new(config), pool.clone(), redis, sealer, lookup_hash)
        .expect("build state");

    Some(Harness {
        app: http::router::build(state.clone()),
        pool,
        state,
    })
}

macro_rules! harness_or_skip {
    () => {
        harness_or_skip!(serde_json::json!({}))
    };
    ($overrides:expr) => {
        match $crate::common::harness_with($overrides).await {
            Some(harness) => harness,
            None => {
                eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
                return;
            }
        }
    };
}

pub(crate) use harness_or_skip;

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

pub struct Response {
    pub status: StatusCode,
    pub json: Value,
    /// The raw body, so a rejection that never produced our error envelope — an extractor
    /// refusing a request before any handler ran — is still readable in a failure message.
    pub raw: String,
}

impl Response {
    pub fn data(&self) -> &Value {
        &self.json["data"]
    }

    pub fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    pub fn message(&self) -> &str {
        self.json["error"]["message"].as_str().unwrap_or_default()
    }

    pub fn expect_ok(&self, context: &str) -> &Value {
        let detail = if self.json.is_null() {
            self.raw.clone()
        } else {
            self.json.to_string()
        };
        assert!(
            self.status.is_success(),
            "{context} failed with {}: {detail}",
            self.status,
        );
        self.data()
    }

    /// Assert a specific refusal, reporting what actually came back when it does not match.
    pub fn expect_error(&self, status: StatusCode, code: &str, context: &str) {
        assert_eq!(
            (self.status, self.error_code()),
            (status, code),
            "{context}: {}",
            self.json
        );
    }

    pub fn id(&self, context: &str) -> Uuid {
        Uuid::parse_str(self.expect_ok(context)["id"].as_str().expect("id")).expect("uuid")
    }
}

pub async fn send(
    app: &Router,
    method: &str,
    path: &str,
    uid: &str,
    body: Option<Value>,
) -> Response {
    send_with_key(app, method, path, uid, Some(Uuid::new_v4()), body).await
}

pub async fn send_with_key(
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
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

/// Follow `next_cursor` through a list endpoint until `matches` hits.
///
/// The test database is shared, kept between runs and never truncated, so "is my row in
/// this list" cannot be asked of the first page alone: on an oldest-first list the answer
/// is yes until the table outgrows one page and no forever after. That failure looks like
/// flakiness and is not — see `operations.rs`, where it cost a day.
pub async fn find_across_pages(
    app: &Router,
    path: &str,
    uid: &str,
    matches: impl Fn(&Value) -> bool,
) -> Option<Value> {
    let separator = if path.contains('?') { '&' } else { '?' };
    let mut cursor: Option<String> = None;

    for _ in 0..10_000 {
        let url = match &cursor {
            Some(cursor) => format!("{path}{separator}cursor={cursor}"),
            None => path.to_owned(),
        };
        let response = send(app, "GET", &url, uid, None).await;
        let page = response.expect_ok("list page");

        if let Some(found) = page["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|item| matches(item))
        {
            return Some(found.clone());
        }

        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return None,
        }
    }

    panic!("{path}: 커서가 끝에 닿지 않았다");
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

pub fn uid(label: &str) -> String {
    format!("p5-{label}-{}", Uuid::new_v4().simple())
}

pub async fn bootstrap(app: &Router, user_uid: &str, name: &str) -> Uuid {
    send(
        app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        user_uid,
        Some(json!({ "display_name": name })),
    )
    .await
    .id("bootstrap")
}

/// A consumer with nothing but an account.
pub struct Consumer {
    pub uid: String,
    pub user_id: Uuid,
}

pub async fn consumer(app: &Router, label: &str) -> Consumer {
    let consumer_uid = uid(label);
    let user_id = bootstrap(app, &consumer_uid, "김손님").await;
    Consumer {
        uid: consumer_uid,
        user_id,
    }
}

pub async fn grant_role(pool: &PgPool, user_id: Uuid, role: &str) {
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

/// An administrator holding the roles §3.3 gives the security desk.
pub async fn admin(harness: &Harness, label: &str) -> Consumer {
    let account = consumer(&harness.app, label).await;
    grant_role(&harness.pool, account.user_id, "SECURITY").await;
    grant_role(&harness.pool, account.user_id, "OPERATIONS").await;
    account
}

/// Approving a store is `POST /admin/store-reviews/:id/decision`, which has its own test
/// (§11.5, STORE-002). Everywhere else a store simply needs to *be* active, so these
/// fixtures move it there directly rather than re-driving the review each time.
pub async fn activate_store(pool: &PgPool, store_id: Uuid) {
    sqlx::query(
        "UPDATE coupon.stores SET status = 'ACTIVE', activated_at = clock_timestamp() WHERE id = $1",
    )
    .bind(store_id)
    .execute(pool)
    .await
    .expect("activate the store");
}

/// An owner with an active store.
pub struct Store {
    pub owner_uid: String,
    pub owner_user_id: Uuid,
    pub id: Uuid,
}

pub async fn store(harness: &Harness, label: &str) -> Store {
    let owner_uid = uid(&format!("{label}-owner"));
    let owner_user_id = bootstrap(&harness.app, &owner_uid, "점주").await;

    let store_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({
            "name": "테스트 카페",
            "slug": format!("p5-{}", Uuid::new_v4().simple()),
        })),
    )
    .await
    .id("create store");
    activate_store(&harness.pool, store_id).await;

    Store {
        owner_uid,
        owner_user_id,
        id: store_id,
    }
}

pub fn default_rules() -> Value {
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

/// Draft and publish a loyalty policy for this store, returning the policy id.
pub async fn publish_policy(harness: &Harness, owner_uid: &str, rules: Value) -> Uuid {
    let policy_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        owner_uid,
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
                "description": "사용 조건",
                "customer_notice": "중복 사용 불가",
            },
        })),
    )
    .await
    .id("draft policy");

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish"),
        owner_uid,
        Some(json!({})),
    )
    .await
    .expect_ok("publish policy");

    policy_id
}

pub fn campaign_draft(issue_mode: &str, total: Value) -> Value {
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

/// Draft and publish a campaign, returning its id.
pub async fn publish_campaign(harness: &Harness, store: &Store, draft: Value) -> Uuid {
    let campaign_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/campaigns",
        &store.owner_uid,
        Some(draft),
    )
    .await
    .id("draft campaign");

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/publish"),
        &store.owner_uid,
        None,
    )
    .await
    .expect_ok("publish campaign");

    campaign_id
}

/// Issue a rotating QR for a consumer: `(token, fallback_code)`.
pub async fn issue_qr(app: &Router, customer_uid: &str) -> (String, String) {
    let response = send(app, "POST", "/api/coupon/v1/me/qr-tokens", customer_uid, None).await;
    let data = response.expect_ok("issue qr");
    (
        data["token"].as_str().expect("token").to_owned(),
        data["fallback_code"].as_str().expect("code").to_owned(),
    )
}

pub fn order(amount: i64) -> Value {
    json!({ "gross_amount": amount, "currency": "KRW", "items": [] })
}

/// Scan a consumer's QR and confirm one accrual, returning the transaction body.
pub async fn earn_a_stamp(harness: &Harness, store: &Store, customer_uid: &str) -> Value {
    let (token, _) = issue_qr(&harness.app, customer_uid).await;
    let response = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &store.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    response.expect_ok("accrue a stamp").clone()
}

/// Run `futures` concurrently and collect the results in order.
///
/// `tokio::spawn` rather than `join_all`: the concurrency tests are about *simultaneous*
/// requests, and futures polled on one task would interleave at await points rather than
/// genuinely contend.
pub async fn futures_join<I, F, T>(futures: I) -> Vec<T>
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
