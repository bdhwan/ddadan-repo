//! Phase 4 notification tests over a real PostgreSQL and a contract-mock provider
//! (§19.2, §19.3).
//!
//! §19.3 asks for the provider matrix — 2xx, 4xx, 429, 5xx and a duplicated callback — plus
//! webhook signature failure and replay prevention. Those need a real HTTP peer, so these
//! tests stand a tiny axum server up on a loopback port and point the configured FCM
//! endpoint at it. That is deliberately not a mocked `WebPushProvider`: the thing under
//! test is how an HTTP answer becomes a `notification_deliveries.status`, and stubbing the
//! provider would skip exactly that translation.
//!
//! ```sh
//! ./scripts/coupon/db-up.sh
//! cd apps/coupon-api-server
//! DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon sqlx migrate run
//! COUPON_TEST_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon \
//!   cargo test --test notifications
//! ```
//!
//! Without `COUPON_TEST_DATABASE_URL` every test here skips with a visible note rather
//! than passing silently.

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::jobs::transport::RegistryOnlyTransport;
use coupon_api_server::notifications::delivery::{self, DispatchOutcome};
use coupon_api_server::jobs::worker::JobRuntime;
use coupon_api_server::state::AppState;
use coupon_api_server::{db, http};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const CALLBACK_SECRET: &str = "test-callback-secret";

// ---------------------------------------------------------------------------
// Contract mock provider (§19.3)
// ---------------------------------------------------------------------------

/// A provider that answers with whatever status the test last set.
struct MockProvider {
    base_url: String,
    status: Arc<AtomicU16>,
    calls: Arc<AtomicU64>,
    /// Kept so the server task lives as long as the test does.
    _task: tokio::task::JoinHandle<()>,
}

impl MockProvider {
    async fn start() -> Self {
        let status = Arc::new(AtomicU16::new(200));
        let calls = Arc::new(AtomicU64::new(0));

        let handler_status = status.clone();
        let handler_calls = calls.clone();
        let app = Router::new().route(
            "/send",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                let status = handler_status.clone();
                let calls = handler_calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let code = status.load(Ordering::SeqCst);
                    let reference = body["delivery_id"].as_str().unwrap_or("unknown").to_owned();

                    let payload = match code {
                        200..=299 => json!({ "provider_reference": format!("prov-{reference}") }),
                        429 => json!({ "code": "RATE_LIMITED" }),
                        400..=499 => json!({ "code": "INVALID_ARGUMENT", "message": "거절" }),
                        _ => json!({ "code": "UPSTREAM" }),
                    };

                    (
                        StatusCode::from_u16(code).unwrap_or(StatusCode::OK),
                        [(axum::http::header::RETRY_AFTER, "45")],
                        axum::Json(payload),
                    )
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the mock provider");
        let addr = listener.local_addr().expect("mock provider address");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self {
            base_url: format!("http://{addr}/send"),
            status,
            calls,
            _task: task,
        }
    }

    fn answer_with(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }

    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    app: Router,
    pool: PgPool,
    state: AppState,
    runtime: JobRuntime,
}

impl Harness {
    /// Everything the worker does between a committed accrual and a provider call.
    ///
    /// Three separate steps because they are three separate loops in `coupon-worker`, and
    /// running them individually is what lets a test stop between two of them — which is
    /// exactly what NOTIFY-001 needs.
    /// Turn every committed domain event into notifications and queued deliveries.
    ///
    /// Looped rather than run once: the relay takes the oldest rows first and these tests
    /// share a database with every other suite, so a single pass can consume somebody
    /// else's backlog and never reach this test's own event.
    ///
    /// Deliberately *not* followed by a job poll. `poll` claims whatever is due across the
    /// whole database, so two tests running in parallel would each dispatch the other's
    /// deliveries through their own mock provider. Each test dispatches its own delivery
    /// instead — the same [`delivery::dispatch`] the `notify_event` handler calls, with
    /// nothing about the send skipped.
    async fn relay(&self) {
        for _ in 0..40 {
            let relayed = self
                .runtime
                .relay_notifications()
                .await
                .expect("notification relay");
            self.runtime.relay().await.expect("job relay");
            if relayed == 0 {
                break;
            }
        }
    }

    /// Relay until this user's notification exists.
    ///
    /// Every harness in this file relays the same global outbox, so a row can be picked up
    /// and published by a *different* test's relay while this one is between passes. The
    /// loop waits for the effect rather than for its own pass to have caused it.
    async fn relay_until_notified(&self, user_id: Uuid) {
        for _ in 0..60 {
            self.relay().await;

            let seen: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM coupon.notifications WHERE user_id = $1",
            )
            .bind(user_id)
            .fetch_one(&self.pool)
            .await
            .expect("count notifications");

            if seen > 0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        panic!("the relay never produced a notification for {user_id}");
    }

    /// Send one delivery, exactly as the worker's `notify_event` handler does.
    async fn dispatch(&self, delivery_id: Uuid) -> DispatchOutcome {
        delivery::dispatch(&self.state, delivery_id)
            .await
            .expect("dispatch")
    }
}

async fn harness_with(overrides: Value) -> Option<Harness> {
    let database_url = std::env::var("COUPON_TEST_DATABASE_URL").ok()?;

    let mut settings = json!({
        "env": "test",
        "database_url": database_url,
        "firebase_project_id": "ddadan-test",
        "auth_dev_bypass": true,
        "database_max_connections": 16,
        "notification_callback_secret": CALLBACK_SECRET,
        "rate_limit_stamp_approval_per_min": 1000,
        "rate_limit_qr_issue_per_min": 1000,
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
        runtime: JobRuntime::new(state.clone(), Arc::new(RegistryOnlyTransport)),
        state,
    })
}

macro_rules! harness_or_skip {
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
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("x-dev-firebase-uid", uid)
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::new_v4().to_string())
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

/// A signed provider callback, posted the way a provider would.
async fn post_callback(
    app: &Router,
    provider: &str,
    body: Value,
    timestamp: &str,
    secret: &str,
) -> StatusCode {
    let raw = body.to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(raw.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/coupon/v1/notifications/callbacks/{provider}"
        ))
        .header("content-type", "application/json")
        .header("x-signature", signature)
        .header("x-signature-timestamp", timestamp)
        .body(Body::from(raw))
        .expect("valid request");

    app.clone()
        .oneshot(request)
        .await
        .expect("router responds")
        .status()
}

fn uid(label: &str) -> String {
    format!("t4-{label}-{}", Uuid::new_v4().simple())
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// An active store with a published policy, a consumer who has consented to push, and a
/// registered browser.
struct Scenario {
    owner_uid: String,
    customer_uid: String,
    customer_user_id: Uuid,
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

    Uuid::parse_str(response.expect_ok("bootstrap")["id"].as_str().expect("id")).expect("uuid")
}

async fn scenario(harness: &Harness, label: &str) -> Scenario {
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
            "name": "알림 베이커리",
            "slug": format!("t4-{}", Uuid::new_v4().simple()),
        })),
    )
    .await;
    let store_id =
        Uuid::parse_str(store.expect_ok("create store")["id"].as_str().expect("id")).expect("uuid");

    sqlx::query(
        "UPDATE coupon.stores SET status = 'ACTIVE', activated_at = clock_timestamp() WHERE id = $1",
    )
    .bind(store_id)
    .execute(&harness.pool)
    .await
    .expect("activate the store");

    let policy = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/loyalty-policies",
        &owner_uid,
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
                "description": "1만원 이상 주문 시 사용 가능",
                "customer_notice": "다른 할인과 중복 사용 불가",
            },
        })),
    )
    .await;
    let policy_id =
        Uuid::parse_str(policy.expect_ok("draft policy")["id"].as_str().expect("id")).expect("uuid");

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish"),
        &owner_uid,
        Some(json!({})),
    )
    .await
    .expect_ok("publish policy");

    Scenario {
        owner_uid,
        customer_uid,
        customer_user_id,
    }
}

/// Grant 서비스 거래 Web Push 동의 and register a browser.
async fn opt_in_to_push(harness: &Harness, customer_uid: &str) {
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/me/consents",
        customer_uid,
        Some(json!({
            "consents": [{
                "scope": "TRANSACTIONAL_WEB_PUSH",
                "action": "GRANTED",
                "source": "settings/alerts",
            }],
        })),
    )
    .await
    .expect_ok("grant push consent");

    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/me/push-subscriptions",
        customer_uid,
        Some(json!({
            "token": format!("fcm-token-{}", Uuid::new_v4().simple()),
            "browser_family": "chrome",
        })),
    )
    .await
    .expect_ok("register the browser");
}

async fn revoke_push_consent(harness: &Harness, customer_uid: &str) {
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/me/consents",
        customer_uid,
        Some(json!({
            "consents": [{
                "scope": "TRANSACTIONAL_WEB_PUSH",
                "action": "REVOKED",
                "source": "settings/alerts",
            }],
        })),
    )
    .await
    .expect_ok("revoke push consent");
}

/// One accrual: issue a QR, resolve it, confirm it.
async fn earn_a_stamp(
    harness: &Harness,
    scenario: &Scenario,
    order_ref: Option<&str>,
) -> Uuid {
    let qr = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/me/qr-tokens",
        &scenario.customer_uid,
        None,
    )
    .await;
    let token = qr.expect_ok("issue qr")["token"]
        .as_str()
        .expect("token")
        .to_owned();

    let confirmed = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &scenario.owner_uid,
        Some(json!({
            "qr_token": token,
            "order": {
                "gross_amount": 12_000,
                "currency": "KRW",
                "items": [],
                // STAMP-003 refuses a near-duplicate unless the owner names a new order
                // *and* acknowledges it.
                "external_order_ref": order_ref,
            },
            "acknowledge_duplicate": order_ref.is_some(),
        })),
    )
    .await;

    Uuid::parse_str(
        confirmed.expect_ok("confirm accrual")["transaction_id"]
            .as_str()
            .expect("transaction id"),
    )
    .expect("uuid")
}

/// The delivery row for one channel of one notification.
async fn delivery_for(pool: &PgPool, user_id: Uuid, channel: &str) -> Option<DeliveryRow> {
    sqlx::query_as::<_, DeliveryRow>(
        "SELECT d.id, d.status::text AS status, d.suppression_reason, d.attempt_count,
                d.provider_reference, d.last_error_code, d.correlation_id
         FROM coupon.notification_deliveries d
         JOIN coupon.notifications n ON n.id = d.notification_id
         WHERE n.user_id = $1 AND d.channel = $2::coupon.notification_channel
         ORDER BY d.created_at DESC
         LIMIT 1",
    )
    .bind(user_id)
    .bind(channel)
    .fetch_optional(pool)
    .await
    .expect("read the delivery")
}

#[derive(Debug, sqlx::FromRow)]
struct DeliveryRow {
    id: Uuid,
    status: String,
    suppression_reason: Option<String>,
    attempt_count: i32,
    provider_reference: Option<String>,
    last_error_code: Option<String>,
    correlation_id: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// End to end (§15.1, §18.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_accrual_becomes_an_in_app_record_and_a_queued_push() {
    // The whole Phase 4 pipeline: 적립 → outbox → STAMP_EARNED 앱 내 알림 → notify job →
    // FCM 발송 → 상태 전이.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "e2e").await;
    opt_in_to_push(&harness, &scenario.customer_uid).await;

    earn_a_stamp(&harness, &scenario, None).await;
    harness.relay_until_notified(scenario.customer_user_id).await;

    // §15.1: the in-app notification is the base record, and it exists whatever the
    // provider does next.
    let inbox = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/notifications",
        &scenario.customer_uid,
        None,
    )
    .await;
    let items = inbox.expect_ok("inbox")["items"]
        .as_array()
        .expect("items")
        .clone();

    assert_eq!(items.len(), 1, "one accrual, one notification: {items:?}");
    assert_eq!(items[0]["type"], "STAMP_EARNED");
    assert_eq!(items[0]["purpose"], "TRANSACTIONAL");
    assert!(
        items[0]["body"]
            .as_str()
            .expect("body")
            .contains("알림 베이커리"),
        "the store name is rendered into the body: {}",
        items[0]["body"]
    );
    assert!(items[0]["read_at"].is_null(), "a new notification is unread");

    // The in-app channel has its own delivery row, already settled: there is no provider
    // between us and the record (§15.4).
    let in_app = delivery_for(&harness.pool, scenario.customer_user_id, "IN_APP")
        .await
        .expect("an in-app delivery exists");
    assert_eq!(in_app.status, "DELIVERED");

    // §14.3: the send is queued as its own job, keyed on event + channel + recipient.
    let queued_job: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.job_registry
         WHERE job_type = 'notify_event' AND resource_id = $1",
    )
    .bind(
        delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
            .await
            .expect("a push delivery exists")
            .id,
    )
    .fetch_one(&harness.pool)
    .await
    .expect("count jobs");
    assert_eq!(queued_job, 1, "the push has its own notify_event job");

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    assert_eq!(push.status, "PENDING");

    harness.dispatch(push.id).await;

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    assert_eq!(push.status, "SENDING", "the provider accepted it");
    assert_eq!(push.attempt_count, 1);
    assert!(
        push.provider_reference.is_some(),
        "the provider's reference is recorded so a callback can be matched"
    );
    assert_eq!(provider.calls(), 1);

    // §18.3: API → outbox → job → provider delivery carry the same correlation id.
    let outbox_correlation: Uuid = sqlx::query_scalar(
        "SELECT correlation_id FROM coupon.outbox_events
         WHERE event_type = 'STAMP_EARNED' AND payload ->> 'user_id' = $1::text
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("outbox row");
    assert_eq!(
        push.correlation_id,
        Some(outbox_correlation),
        "the delivery inherits the domain event's correlation id"
    );
}

#[tokio::test]
async fn clearing_the_inbox_leaves_the_ledger_alone() {
    // §15.1: 알림을 지워도 거래·쿠폰은 지워지지 않는다.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "inbox").await;

    let transaction_id = earn_a_stamp(&harness, &scenario, None).await;
    // `relay` alone stops as soon as *a* pass relays nothing, which a parallel test's
    // relay can arrange before this one's event is reached; the inbox would then be empty
    // for a reason that has nothing to do with what this test is about.
    harness.relay_until_notified(scenario.customer_user_id).await;

    let updated = send(
        &harness.app,
        "PATCH",
        "/api/coupon/v1/me/notifications",
        &scenario.customer_uid,
        Some(json!({ "all": true, "action": "DISMISS" })),
    )
    .await;
    assert_eq!(updated.expect_ok("dismiss")["updated"], 1);

    let inbox = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/notifications",
        &scenario.customer_uid,
        None,
    )
    .await;
    assert!(
        inbox.expect_ok("inbox")["items"]
            .as_array()
            .expect("items")
            .is_empty(),
        "a dismissed notification leaves the inbox"
    );

    let ledger_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_ledger WHERE source_stamp_transaction_id = $1",
    )
    .bind(transaction_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count the ledger");
    assert!(
        ledger_rows > 0,
        "clearing the inbox must not touch the accrual"
    );
}

// ---------------------------------------------------------------------------
// Consent (§15.3, NOTIFY-001)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_consent_withdrawn_after_enqueue_stops_the_send() {
    // NOTIFY-001: 철회가 enqueue 이후에 발생해도 실제 provider 호출 직전에 동의를 다시
    // 확인한다. The delivery is created while consent holds and dispatched after it is gone.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "withdraw").await;
    opt_in_to_push(&harness, &scenario.customer_uid).await;

    earn_a_stamp(&harness, &scenario, None).await;
    harness.relay_until_notified(scenario.customer_user_id).await;

    let queued = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery was created while consent held");
    assert_eq!(
        queued.status, "PENDING",
        "eligibility passed when the row was written"
    );

    revoke_push_consent(&harness, &scenario.customer_uid).await;

    let outcome = harness.dispatch(queued.id).await;
    assert!(
        matches!(outcome, DispatchOutcome::Suppressed { .. }),
        "{outcome:?}"
    );

    let after = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("the delivery is still there");
    assert_eq!(after.status, "SUPPRESSED");
    assert_eq!(after.suppression_reason.as_deref(), Some("CONSENT_MISSING"));
    assert_eq!(
        provider.calls(),
        0,
        "the provider must never have been called"
    );

    // §15.1: the in-app record is unaffected by an external channel being refused.
    let inbox = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/notifications",
        &scenario.customer_uid,
        None,
    )
    .await;
    assert_eq!(
        inbox.expect_ok("inbox")["items"]
            .as_array()
            .expect("items")
            .len(),
        1
    );
}

#[tokio::test]
async fn without_consent_the_external_channels_are_suppressed_with_reasons() {
    // §15.3: absent consent is a no, and the refusal is recorded rather than silent.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "noconsent").await;

    earn_a_stamp(&harness, &scenario, None).await;
    harness.relay_until_notified(scenario.customer_user_id).await;

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a suppression is still a row");
    assert_eq!(push.status, "SUPPRESSED");
    assert_eq!(
        push.suppression_reason.as_deref(),
        Some("NO_ACTIVE_SUBSCRIPTION"),
        "no browser has been registered, which is checked before the consent flag"
    );
    assert_eq!(provider.calls(), 0);

    // 알림톡 is suppressed too, for its own reason.
    let alimtalk = delivery_for(&harness.pool, scenario.customer_user_id, "KAKAO_ALIMTALK")
        .await
        .expect("an alimtalk row exists");
    assert_eq!(alimtalk.status, "SUPPRESSED");
    assert_eq!(
        alimtalk.suppression_reason.as_deref(),
        Some("TEMPLATE_UNAVAILABLE"),
        "§15.1 keeps 알림톡 to approved templates, and STAMP_EARNED has none yet — that is \
         checked before consent because an unapproved template cannot be sent to anyone"
    );
}

// ---------------------------------------------------------------------------
// Provider contract (§19.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_provider_status_maps_onto_the_15_4_vocabulary() {
    // §19.3: FCM/알림톡 provider contract mock 의 2xx, 4xx, 429, 5xx.
    for (status, expected, expected_error) in [
        (200u16, "SENDING", None),
        (429u16, "FAILED_RETRYABLE", Some("RATE_LIMITED")),
        (400u16, "FAILED_PERMANENT", Some("INVALID_ARGUMENT")),
        (503u16, "FAILED_RETRYABLE", Some("UPSTREAM")),
    ] {
        let provider = MockProvider::start().await;
        provider.answer_with(status);

        let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
        let scenario = scenario(&harness, &format!("status{status}")).await;
        opt_in_to_push(&harness, &scenario.customer_uid).await;

        earn_a_stamp(&harness, &scenario, None).await;
        harness.relay_until_notified(scenario.customer_user_id).await;

        let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
            .await
            .expect("a push delivery exists");
        harness.dispatch(push.id).await;

        let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
            .await
            .expect("a push delivery exists");

        assert_eq!(push.status, expected, "provider answered {status}");
        assert_eq!(
            push.last_error_code.as_deref(),
            expected_error,
            "provider answered {status}"
        );
        assert_eq!(provider.calls(), 1, "provider answered {status}");

        // §14.7: provider 429/Retry-After 는 제공자 값을 우선한다. The mock answers 45.
        if status == 429 {
            let next: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
                "SELECT next_attempt_at FROM coupon.notification_deliveries WHERE id = $1",
            )
            .bind(push.id)
            .fetch_one(&harness.pool)
            .await
            .expect("read next_attempt_at");
            let delay = next.expect("a retry is scheduled") - chrono::Utc::now();
            assert!(
                delay.num_seconds() > 30 && delay.num_seconds() <= 46,
                "the provider's own Retry-After wins: {delay}"
            );
        }
    }
}

#[tokio::test]
async fn a_permanent_provider_failure_leaves_the_reward_untouched() {
    // NOTIFY-003 / §15.4: 영구 실패가 지갑 혜택 상태에 영향을 주지 않는다.
    let provider = MockProvider::start().await;
    provider.answer_with(400);

    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "failsafe").await;
    opt_in_to_push(&harness, &scenario.customer_uid).await;

    // Three stamps reaches the goal and issues a reward coupon. Each accrual carries its
    // own order reference so STAMP-003's near-duplicate guard does not refuse the second.
    for round in 1..=3 {
        earn_a_stamp(&harness, &scenario, Some(&format!("order-{round}"))).await;
        harness.relay_until_notified(scenario.customer_user_id).await;
    }

    let reward = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    harness.dispatch(reward.id).await;

    let after = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    assert_eq!(after.status, "FAILED_PERMANENT");

    let wallet = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/coupons",
        &scenario.customer_uid,
        None,
    )
    .await;
    let coupons = wallet.expect_ok("wallet")["items"]
        .as_array()
        .cloned()
        .or_else(|| wallet.data()["coupons"].as_array().cloned())
        .expect("coupons");

    assert_eq!(coupons.len(), 1, "the reward exists: {coupons:?}");
    assert_eq!(
        coupons[0]["status"], "AVAILABLE",
        "a failed notification must not change the coupon"
    );
}

// ---------------------------------------------------------------------------
// Duplication (NOTIFY-004, §14.6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn one_event_produces_one_send_however_often_it_is_relayed() {
    // NOTIFY-004: event_id + channel + template_version + recipient 를 발송 고유키로 쓴다.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "dedupe").await;
    opt_in_to_push(&harness, &scenario.customer_uid).await;

    earn_a_stamp(&harness, &scenario, None).await;

    // Put the outbox row back to PENDING between relays, which is what an at-least-once
    // relay looks like from the database's point of view.
    for _ in 0..3 {
        sqlx::query(
            "UPDATE coupon.outbox_events SET status = 'PENDING', published_at = NULL
             WHERE event_type = 'STAMP_EARNED' AND payload ->> 'user_id' = $1::text",
        )
        .bind(scenario.customer_user_id)
        .execute(&harness.pool)
        .await
        .expect("replay the outbox");
        harness.relay_until_notified(scenario.customer_user_id).await;
    }

    let notifications: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.notifications
         WHERE user_id = $1 AND notification_type = 'STAMP_EARNED'",
    )
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count notifications");
    assert_eq!(notifications, 1, "one event, one notification");

    let deliveries: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.notification_deliveries d
         JOIN coupon.notifications n ON n.id = d.notification_id
         WHERE n.user_id = $1 AND d.channel = 'FCM_WEB_PUSH'",
    )
    .bind(scenario.customer_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count deliveries");
    assert_eq!(deliveries, 1, "one event and one channel, one delivery");

    // And dispatching the same delivery twice does not send twice: the second attempt
    // finds a settled row and stops (§14.5-4).
    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    harness.dispatch(push.id).await;
    let repeat = harness.dispatch(push.id).await;

    assert!(
        matches!(repeat, DispatchOutcome::Sent { .. } | DispatchOutcome::AlreadySettled { .. }),
        "{repeat:?}"
    );
    assert_eq!(
        provider.calls(),
        1,
        "the provider is called once however many times the relay and the job run"
    );
}

// ---------------------------------------------------------------------------
// Callbacks (§15.4, §19.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_duplicate_callback_confirms_the_result_only_once() {
    // §15.4: 같은 사건 콜백이 여러 번 와도 발송 결과를 한 번만 확정한다.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "callback").await;
    opt_in_to_push(&harness, &scenario.customer_uid).await;

    earn_a_stamp(&harness, &scenario, None).await;
    harness.relay_until_notified(scenario.customer_user_id).await;

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    harness.dispatch(push.id).await;

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    let reference = push.provider_reference.clone().expect("a provider reference");
    let event_id = format!("cb-{}", Uuid::new_v4().simple());

    let body = json!({
        "event_id": event_id,
        "provider_reference": reference,
        "status": "DELIVERED",
    });
    let timestamp = chrono::Utc::now().to_rfc3339();

    let first = post_callback(&harness.app, "fcm", body.clone(), &timestamp, CALLBACK_SECRET).await;
    assert_eq!(first, StatusCode::OK);

    let second = post_callback(&harness.app, "fcm", body, &timestamp, CALLBACK_SECRET).await;
    assert_eq!(
        second,
        StatusCode::OK,
        "a duplicate is accepted, not an error"
    );

    let after = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("the delivery is still there");
    assert_eq!(after.status, "DELIVERED");

    let recorded: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.notification_delivery_callbacks WHERE delivery_id = $1",
    )
    .bind(after.id)
    .fetch_one(&harness.pool)
    .await
    .expect("count callbacks");
    assert_eq!(
        recorded, 1,
        "the duplicate is recognised by its provider event id rather than stored twice"
    );

    let applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.notification_delivery_callbacks
         WHERE delivery_id = $1 AND applied",
    )
    .bind(after.id)
    .fetch_one(&harness.pool)
    .await
    .expect("count applied callbacks");
    assert_eq!(applied, 1, "the result is confirmed exactly once");
}

#[tokio::test]
async fn a_forged_or_replayed_callback_changes_nothing() {
    // §19.3: webhook 서명 실패와 replay 방지.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let scenario = scenario(&harness, "forged").await;
    opt_in_to_push(&harness, &scenario.customer_uid).await;

    earn_a_stamp(&harness, &scenario, None).await;
    harness.relay_until_notified(scenario.customer_user_id).await;

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    harness.dispatch(push.id).await;

    let push = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("a push delivery exists");
    let reference = push.provider_reference.clone().expect("a provider reference");

    let body = json!({
        "event_id": format!("cb-{}", Uuid::new_v4().simple()),
        "provider_reference": reference,
        "status": "DELIVERED",
    });

    // Signed with the wrong secret.
    let forged = post_callback(
        &harness.app,
        "fcm",
        body.clone(),
        &chrono::Utc::now().to_rfc3339(),
        "not-the-secret",
    )
    .await;
    assert_eq!(forged, StatusCode::UNAUTHORIZED);

    // Correctly signed, but an hour old: the signature is valid and the request is still
    // refused, which is the whole point of the freshness window.
    let stale_timestamp = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let replayed = post_callback(&harness.app, "fcm", body, &stale_timestamp, CALLBACK_SECRET).await;
    assert_eq!(replayed, StatusCode::UNAUTHORIZED);

    let after = delivery_for(&harness.pool, scenario.customer_user_id, "FCM_WEB_PUSH")
        .await
        .expect("the delivery is still there");
    assert_eq!(
        after.status, "SENDING",
        "neither rejected callback moved the delivery"
    );
}

#[tokio::test]
async fn a_callback_naming_an_unknown_reference_is_accepted_and_ignored() {
    // The provider did nothing wrong and retrying will not help it, so this is a 200 with
    // `IGNORED` rather than an error the provider will keep re-sending.
    let provider = MockProvider::start().await;
    let harness = harness_or_skip!(json!({ "fcm_endpoint": provider.base_url }));
    let event_id = format!("cb-unknown-{}", Uuid::new_v4().simple());

    let status = post_callback(
        &harness.app,
        "fcm",
        json!({
            "event_id": event_id,
            "provider_reference": "nobody-sent-this",
            "status": "DELIVERED",
        }),
        &chrono::Utc::now().to_rfc3339(),
        CALLBACK_SECRET,
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    let recorded: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.notification_delivery_callbacks
         WHERE provider_event_id = $1 AND delivery_id IS NULL AND NOT applied",
    )
    .bind(&event_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count callbacks");
    assert_eq!(
        recorded, 1,
        "an unmatched callback is still evidence and is kept"
    );
}
