//! Phase 4 operations, analytics and privacy tests over a real PostgreSQL (§19.2).
//!
//! These are the assertions that only a real database settles: the separation-of-duties
//! CHECK on a permanent sanction, the audit trail's hash chain, the privacy floor over
//! aggregated counts, and an erasure that has to leave the transaction ledger standing
//! while removing the person from it.
//!
//! ```sh
//! ./scripts/coupon/db-up.sh
//! cd apps/coupon-api-server
//! DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon sqlx migrate run
//! COUPON_TEST_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon \
//!   cargo test --test operations
//! ```
//!
//! Without `COUPON_TEST_DATABASE_URL` every test here skips with a visible note rather
//! than passing silently.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration, NaiveDate, Utc};
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
    state: AppState,
}

async fn harness_with(overrides: Value) -> Option<Harness> {
    let database_url = std::env::var("COUPON_TEST_DATABASE_URL").ok()?;

    let mut settings = json!({
        "env": "test",
        "database_url": database_url,
        "firebase_project_id": "ddadan-test",
        "auth_dev_bypass": true,
        "database_max_connections": 16,
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
        state,
    })
}

macro_rules! harness_or_skip {
    () => {
        harness_or_skip!(json!({}))
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
    /// The raw body, so a rejection that never produced our error envelope — an extractor
    /// refusing a request before any handler ran — is still readable in a failure message.
    raw: String,
}

impl Response {
    fn data(&self) -> &Value {
        &self.json["data"]
    }

    fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    fn expect_ok(&self, context: &str) -> &Value {
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
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn uid(label: &str) -> String {
    format!("ops-{label}-{}", Uuid::new_v4().simple())
}

/// Follow `next_cursor` through a list endpoint until `matches` hits, and return that item.
///
/// Every list here is global and this database is shared, kept between runs and never
/// truncated, so "is my row in the list" cannot be asked of the first page alone: the
/// answer is yes until the table outgrows one page and no afterwards. Walking the cursor
/// asks the question the test actually means, and paying one extra request per hundred
/// accumulated rows is cheaper than a suite that decays into flakiness.
async fn find_across_pages(
    app: &Router,
    path: &str,
    uid: &str,
    matches: impl Fn(&Value) -> bool,
) -> Option<Value> {
    let separator = if path.contains('?') { '&' } else { '?' };
    let mut cursor: Option<String> = None;

    // A page holds at most 100 rows (§11.1); the bound only stops a runaway loop from
    // hanging the suite if `next_cursor` ever stopped advancing.
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

/// An administrator with the roles §3.3 gives the security desk.
async fn admin(harness: &Harness, label: &str) -> String {
    let admin_uid = uid(label);
    let admin_id = bootstrap(&harness.app, &admin_uid, "관리자").await;
    grant_role(&harness.pool, admin_id, "SECURITY").await;
    grant_role(&harness.pool, admin_id, "OPERATIONS").await;
    admin_uid
}

async fn open_case(harness: &Harness, admin_uid: &str, subject_user_id: Option<Uuid>) -> Uuid {
    let mut body = json!({
        "case_type": "QR_ABUSE",
        "title": "도용 신고",
        "description": "동일 QR 이 여러 매장에서 사용되었습니다.",
    });
    if let Some(subject) = subject_user_id {
        body["subject_user_id"] = json!(subject);
    }

    let case = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/cases",
        admin_uid,
        Some(body),
    )
    .await;

    Uuid::parse_str(case.expect_ok("open case")["id"].as_str().expect("id")).expect("uuid")
}

/// An active store owned by a fresh owner.
struct Store {
    owner_uid: String,
    owner_user_id: Uuid,
    id: Uuid,
    policy_id: Uuid,
}

async fn active_store(harness: &Harness, label: &str) -> Store {
    let owner_uid = uid(&format!("{label}-owner"));
    let owner_user_id = bootstrap(&harness.app, &owner_uid, "점주").await;

    let store = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({
            "name": "운영 베이커리",
            "slug": format!("ops-{}", Uuid::new_v4().simple()),
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
                "description": "사용 조건",
                "customer_notice": "중복 사용 불가",
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

    Store {
        owner_uid,
        owner_user_id,
        id: store_id,
        policy_id,
    }
}

/// Insert a confirmed accrual straight into the ledger tables.
///
/// The accrual path itself is Phase 2's and has its own tests; what these tests need is a
/// controlled population for a given business day, which the API cannot produce without
/// also controlling the clock.
async fn seed_transaction(
    pool: &PgPool,
    store_id: Uuid,
    owner_user_id: Uuid,
    policy_id: Uuid,
    user_id: Uuid,
    business_day: NaiveDate,
) {
    sqlx::query(
        "INSERT INTO coupon.stamp_transactions
             (store_id, user_id, policy_id, business_day, quantity, order_snapshot, status,
              approved_by_user_id, confirmed_at, idempotency_key)
         VALUES ($1, $2, $3, $4, 1, '{}'::jsonb, 'CONFIRMED', $5,
                 ($4::date + time '12:00') AT TIME ZONE 'Asia/Seoul', public.gen_random_uuid())",
    )
    .bind(store_id)
    .bind(user_id)
    .bind(policy_id)
    .bind(business_day)
    .bind(owner_user_id)
    .execute(pool)
    .await
    .expect("seed a transaction");
}

// ---------------------------------------------------------------------------
// Analytics (§6.3, §19)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_day_the_batch_has_not_reached_reports_pending_rather_than_zero() {
    // §19: 실시간 수치와 확정 배치 수치를 구분한다. A zero would be indistinguishable from a
    // quiet day, and an owner would read it as "nobody came in".
    let harness = harness_or_skip!();
    let store = active_store(&harness, "pending").await;

    let response = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/owner/analytics",
        &store.owner_uid,
        None,
    )
    .await;
    let data = response.expect_ok("analytics");

    let days = data["days"].as_array().expect("days");
    assert_eq!(days.len(), 30, "the default window is 30 business days");
    assert!(
        days.iter().all(|day| day["state"] == "PENDING"),
        "nothing has been aggregated yet"
    );
    assert!(
        days.iter().all(|day| day["metrics"].is_null()),
        "a pending day carries no numbers at all"
    );
    assert_eq!(data["pending_days"], 30);
    assert_eq!(data["finalised_days"], 0);
}

#[tokio::test]
async fn a_cohort_below_the_privacy_floor_loses_its_breakdown() {
    // §19: 소규모 집단의 개인 식별을 막기 위해 세그먼트가 기준 인원 미만이면 상세 분해를
    // 숨긴다.
    let harness = harness_or_skip!(json!({ "analytics_min_cohort_size": 3 }));
    let store = active_store(&harness, "cohort").await;

    let yesterday = Utc::now().date_naive() - Duration::days(1);

    // Two customers: below the floor of three.
    for index in 0..2 {
        let customer = bootstrap(&harness.app, &uid(&format!("cohort-c{index}")), "손님").await;
        seed_transaction(
            &harness.pool,
            store.id,
            store.owner_user_id,
            store.policy_id,
            customer,
            yesterday,
        )
        .await;
    }

    harness
        .state
        .analytics
        .aggregate_day(&harness.pool, store.id, yesterday, None, Utc::now())
        .await
        .expect("aggregate");

    let response = send(
        &harness.app,
        "GET",
        &format!(
            "/api/coupon/v1/owner/analytics?from={yesterday}&to={yesterday}"
        ),
        &store.owner_uid,
        None,
    )
    .await;
    let data = response.expect_ok("analytics");
    let day = &data["days"][0];

    assert_eq!(day["state"], "FINAL", "the business day has closed");
    assert_eq!(day["suppressed"], true);
    assert!(
        day["metrics"]["active_customer_count"].is_null(),
        "the per-person figure is withheld: {}",
        day["metrics"]
    );
    assert_eq!(
        day["metrics"]["stamp_transaction_count"], 2,
        "the shop's own transaction count is not personal data about a customer"
    );

    // A third customer reaches the floor, and the breakdown returns.
    let third = bootstrap(&harness.app, &uid("cohort-c2"), "손님").await;
    seed_transaction(
        &harness.pool,
        store.id,
        store.owner_user_id,
        store.policy_id,
        third,
        yesterday,
    )
    .await;
    harness
        .state
        .analytics
        .aggregate_day(&harness.pool, store.id, yesterday, None, Utc::now())
        .await
        .expect("re-aggregate");

    let response = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/owner/analytics?from={yesterday}&to={yesterday}"),
        &store.owner_uid,
        None,
    )
    .await;
    let day = &response.expect_ok("analytics")["days"][0];

    assert_eq!(day["suppressed"], false);
    assert_eq!(day["metrics"]["active_customer_count"], 3);
}

#[tokio::test]
async fn aggregating_twice_produces_the_same_numbers() {
    // §19: 지표는 원장을 기준으로 재산출 가능해야 한다. The rollup is an upsert over the
    // ledgers, so a re-run after a correction reproduces rather than accumulates.
    let harness = harness_or_skip!(json!({ "analytics_min_cohort_size": 1 }));
    let store = active_store(&harness, "idempotent").await;
    let yesterday = Utc::now().date_naive() - Duration::days(1);

    let customer = bootstrap(&harness.app, &uid("idem-c"), "손님").await;
    seed_transaction(
        &harness.pool,
        store.id,
        store.owner_user_id,
        store.policy_id,
        customer,
        yesterday,
    )
    .await;

    for _ in 0..3 {
        harness
            .state
            .analytics
            .aggregate_day(&harness.pool, store.id, yesterday, None, Utc::now())
            .await
            .expect("aggregate");
    }

    let response = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/owner/analytics?from={yesterday}&to={yesterday}"),
        &store.owner_uid,
        None,
    )
    .await;
    let day = &response.expect_ok("analytics")["days"][0];

    assert_eq!(day["metrics"]["stamp_transaction_count"], 1);
    assert_eq!(day["metrics"]["stamp_earned_count"], 1);
    assert_eq!(day["metrics"]["active_customer_count"], 1);
}

// ---------------------------------------------------------------------------
// Sanctions and sessions (§11.5, §3.3, ADMIN-002)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_permanent_sanction_needs_a_second_administrator() {
    // §3.3 / ADMIN-002: 영구 정지와 폐쇄는 이중 확인한다.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "sanction-admin").await;
    let admin_id = bootstrap(&harness.app, &admin_uid, "관리자").await;
    let subject = bootstrap(&harness.app, &uid("sanction-subject"), "대상").await;
    let case_id = open_case(&harness, &admin_uid, Some(subject)).await;

    let unapproved = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{subject}/suspend"),
        &admin_uid,
        Some(json!({
            "sanction_type": "PERMANENT",
            "case_id": case_id,
            "public_reason": "이용약관 위반",
            "internal_reason": "다중 계정으로 선착순 쿠폰 반복 수령",
        })),
    )
    .await;
    assert_eq!(unapproved.status, StatusCode::FORBIDDEN);
    assert_eq!(unapproved.error_code(), "APPROVAL_SEPARATION_REQUIRED");

    // Naming oneself as the approver is the same failure.
    let self_approved = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{subject}/suspend"),
        &admin_uid,
        Some(json!({
            "sanction_type": "PERMANENT",
            "case_id": case_id,
            "public_reason": "이용약관 위반",
            "internal_reason": "다중 계정",
            "approved_by_user_id": admin_id,
        })),
    )
    .await;
    assert_eq!(self_approved.status, StatusCode::FORBIDDEN);

    // A second administrator makes it valid.
    let approver_uid = uid("sanction-approver");
    let approver_id = bootstrap(&harness.app, &approver_uid, "승인자").await;
    grant_role(&harness.pool, approver_id, "SUPER_ADMIN").await;

    let approved = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{subject}/suspend"),
        &admin_uid,
        Some(json!({
            "sanction_type": "PERMANENT",
            "case_id": case_id,
            "public_reason": "이용약관 위반",
            "internal_reason": "다중 계정",
            "approved_by_user_id": approver_id,
        })),
    )
    .await;
    let sanction = approved.expect_ok("permanent sanction");
    assert_eq!(sanction["status"], "ACTIVE");
    assert_eq!(sanction["sanction_type"], "PERMANENT");

    // ADMIN-002 / AUTH-008: the account is suspended and its live sessions are cut.
    let account: (String, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT status::text, sessions_valid_after FROM coupon.users WHERE id = $1",
    )
    .bind(subject)
    .fetch_one(&harness.pool)
    .await
    .expect("read the subject");
    assert_eq!(account.0, "SUSPENDED");
    assert!(
        account.1.is_some(),
        "a suspension that leaves live sessions alone is not a suspension"
    );

    // One live sanction per subject.
    let repeat = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{subject}/suspend"),
        &admin_uid,
        Some(json!({
            "sanction_type": "TEMPORARY",
            "case_id": case_id,
            "public_reason": "추가 제재",
            "internal_reason": "중복",
            "expires_at": (Utc::now() + Duration::days(1)).to_rfc3339(),
        })),
    )
    .await;
    assert_eq!(repeat.status, StatusCode::CONFLICT);
    assert_eq!(repeat.error_code(), "SANCTION_ALREADY_ACTIVE");
}

#[tokio::test]
async fn a_temporary_sanction_expires_and_the_account_comes_back() {
    // ADMIN-002: 임시 정지는 종료 시각을 둘 수 있고 만료 시 자동 복구 후보가 된다.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "temp-admin").await;
    let subject = bootstrap(&harness.app, &uid("temp-subject"), "대상").await;
    let case_id = open_case(&harness, &admin_uid, Some(subject)).await;

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{subject}/suspend"),
        &admin_uid,
        Some(json!({
            "sanction_type": "TEMPORARY",
            "case_id": case_id,
            "public_reason": "일시 이용 제한",
            "internal_reason": "위험 신호 검토 중",
            "expires_at": (Utc::now() + Duration::minutes(5)).to_rfc3339(),
        })),
    )
    .await
    .expect_ok("temporary sanction");

    // Nothing is due yet.
    let none = harness
        .state
        .operations
        .expire_due_sanctions(&harness.pool, Utc::now())
        .await
        .expect("expire");
    assert_eq!(none, 0);

    let expired = harness
        .state
        .operations
        .expire_due_sanctions(&harness.pool, Utc::now() + Duration::minutes(10))
        .await
        .expect("expire");
    assert!(expired >= 1);

    let status: String = sqlx::query_scalar("SELECT status::text FROM coupon.users WHERE id = $1")
        .bind(subject)
        .fetch_one(&harness.pool)
        .await
        .expect("read the subject");
    assert_eq!(status, "ACTIVE", "the account is a recovery candidate");
}

#[tokio::test]
async fn revoking_sessions_records_the_cut_off_and_the_reason() {
    // §11.5 `POST /admin/users/:id/revoke-sessions`.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "revoke-admin").await;
    let subject = bootstrap(&harness.app, &uid("revoke-subject"), "대상").await;

    let response = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{subject}/revoke-sessions"),
        &admin_uid,
        Some(json!({ "reason": "계정 도용 신고 접수" })),
    )
    .await;
    let revocation = response.expect_ok("revoke sessions");

    assert_eq!(revocation["provider_result"], "PENDING");
    assert!(!revocation["valid_after"].is_null());

    let valid_after: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT sessions_valid_after FROM coupon.users WHERE id = $1")
            .bind(subject)
            .fetch_one(&harness.pool)
            .await
            .expect("read the subject");
    assert!(
        valid_after.is_some(),
        "the cut-off binds here, whatever Firebase is doing"
    );
}

// ---------------------------------------------------------------------------
// Cases and audit (§11.5, §12.5, ADMIN-004, SEC-005)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_case_carries_its_resolution_and_its_audit_trail() {
    // ADMIN-004: 처리 결과를 사건과 감사 로그에 연결한다.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "case-admin").await;
    let case_id = open_case(&harness, &admin_uid, None).await;

    let updated = send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/admin/cases/{case_id}"),
        &admin_uid,
        Some(json!({
            "status": "RESOLVED",
            "resolution_type": "EXPLANATION",
            "public_resolution": "정상 사용으로 확인되었습니다.",
            "internal_resolution": "동일 기기에서의 재발급으로 확인",
            "reason": "조사 완료",
        })),
    )
    .await;
    let case = updated.expect_ok("resolve case");

    assert_eq!(case["status"], "RESOLVED");
    assert_eq!(case["resolution_type"], "EXPLANATION");
    assert!(!case["resolved_at"].is_null(), "the CHECK demands a timestamp");

    let audit = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/admin/audit-logs?case_id={case_id}"),
        &admin_uid,
        None,
    )
    .await;
    let entries = audit.expect_ok("audit search")["items"]
        .as_array()
        .expect("items")
        .clone();

    assert!(
        entries.len() >= 2,
        "opening and resolving are both audited: {entries:?}"
    );
    assert!(
        entries.iter().all(|entry| entry["chain_intact"] == true),
        "§12.5: every entry still chains to its predecessor"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["action"] == "admin_case.updated"),
    );
}

#[tokio::test]
async fn a_tampered_audit_entry_is_reported_as_broken() {
    // §12.5: append-only·변조 탐지. The trigger stops an UPDATE, so tampering has to look
    // like an insert with the wrong chain — which is exactly what the read-time check
    // catches.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "tamper-admin").await;
    let case_id = open_case(&harness, &admin_uid, None).await;

    // The database refuses to let even an administrator rewrite an entry.
    let rewrite = sqlx::query("UPDATE coupon.audit_logs SET reason = '변조' WHERE case_id = $1")
        .bind(case_id)
        .execute(&harness.pool)
        .await;
    assert!(rewrite.is_err(), "audit_logs is append-only");

    // An entry written outside the service, with a plausible-looking but wrong hash.
    sqlx::query(
        "INSERT INTO coupon.audit_logs
             (actor_type, action, resource_type, resource_id, case_id, reason, metadata,
              previous_entry_hash, entry_hash)
         VALUES ('SYSTEM_ADMIN', 'admin_case.updated', 'admin_case', $1, $1, '위조',
                 '{}'::jsonb, repeat('0', 64), repeat('a', 64))",
    )
    .bind(case_id)
    .execute(&harness.pool)
    .await
    .expect("insert a forged entry");

    let audit = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/admin/audit-logs?case_id={case_id}"),
        &admin_uid,
        None,
    )
    .await;
    let entries = audit.expect_ok("audit search")["items"]
        .as_array()
        .expect("items")
        .clone();

    assert!(
        entries
            .iter()
            .any(|entry| entry["chain_intact"] == false && entry["reason"] == "위조"),
        "the forged entry is reported as broken: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["chain_intact"] == true),
        "the genuine entries are still intact"
    );
}

#[tokio::test]
async fn support_reads_a_case_without_its_investigation_notes() {
    // ADMIN-002 / §3.3: 공개 가능한 사유와 내부 사유를 분리한다.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "notes-admin").await;
    let case_id = open_case(&harness, &admin_uid, None).await;

    send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/admin/cases/{case_id}"),
        &admin_uid,
        Some(json!({
            "public_resolution": "확인 중입니다.",
            "internal_resolution": "내부 위험 점수 92",
            "reason": "메모",
        })),
    )
    .await
    .expect_ok("annotate");

    let support_uid = uid("support");
    let support_id = bootstrap(&harness.app, &support_uid, "상담원").await;
    grant_role(&harness.pool, support_id, "SUPPORT").await;

    let as_support = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/admin/cases/{case_id}"),
        &support_uid,
        None,
    )
    .await;
    let case = as_support.expect_ok("read as support");
    assert_eq!(case["public_resolution"], "확인 중입니다.");
    assert!(
        case["internal_resolution"].is_null(),
        "support does not read the investigation notes"
    );

    let as_security = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/admin/cases/{case_id}"),
        &admin_uid,
        None,
    )
    .await;
    assert_eq!(
        as_security.expect_ok("read as security")["internal_resolution"],
        "내부 위험 점수 92"
    );

    // SEC-005: reading the audit trail is itself privileged.
    let refused = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/admin/audit-logs",
        &support_uid,
        None,
    )
    .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_review_queue_activates_a_store_on_approval() {
    // §11.5 / STORE-002: 검수 큐와 승인·보완·거절.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "review-admin").await;

    let owner_uid = uid("review-owner");
    bootstrap(&harness.app, &owner_uid, "점주").await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({
            "name": "검수 베이커리",
            "slug": format!("rev-{}", Uuid::new_v4().simple()),
        })),
    )
    .await
    .expect_ok("create store");

    send(
        &harness.app,
        "PATCH",
        "/api/coupon/v1/owner/store",
        &owner_uid,
        Some(json!({
            "address": { "road": "성수이로 1" },
            "business_profile": {
                "registration_no": "123-45-67890",
                "representative_name": "김대표",
            },
        })),
    )
    .await
    .expect_ok("business profile");

    let submitted = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store/submit-review",
        &owner_uid,
        Some(json!({ "note": "검수 요청드립니다." })),
    )
    .await;
    let store = submitted.expect_ok("submit review");
    assert_eq!(store["status"], "PENDING_REVIEW");
    let review_id = store["latest_review"]["id"].as_str().expect("review id");

    // The queue is oldest-first (§11.5), so a submission made just now sits on the *last*
    // page — and this database is shared and long-lived. Reading only the first page passes
    // on an empty database and starts failing the moment the queue outgrows one page, which
    // reads as flakiness rather than as the accumulation it is.
    let entry = find_across_pages(
        &harness.app,
        "/api/coupon/v1/admin/store-reviews?status=PENDING&limit=100",
        &admin_uid,
        |entry| entry["id"] == review_id,
    )
    .await
    .expect("the submission is in the queue");
    assert_eq!(entry["store_status"], "PENDING_REVIEW");

    let decided = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/store-reviews/{review_id}/decision"),
        &admin_uid,
        Some(json!({
            "decision": "APPROVED",
            "public_reason": "승인되었습니다.",
            "reason": "서류 확인 완료",
        })),
    )
    .await;
    let entry = decided.expect_ok("decide");
    assert_eq!(entry["status"], "APPROVED");
    assert_eq!(entry["store_status"], "ACTIVE");

    // A decided review cannot be decided again.
    let repeat = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/store-reviews/{review_id}/decision"),
        &admin_uid,
        Some(json!({ "decision": "REJECTED", "reason": "번복" })),
    )
    .await;
    assert_eq!(repeat.status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------
// Privacy (§17.3, ADMIN-006, §18.5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_erasure_leaves_a_tombstone_and_keeps_the_transaction_ledger() {
    // §17.3: 탈퇴자는 거래 원장의 user FK 를 가명 tombstone 으로 치환할 수 있게 설계한다.
    let harness = harness_or_skip!(json!({ "privacy_deletion_grace_days": 0 }));
    let admin_uid = admin(&harness, "erase-admin").await;
    let store = active_store(&harness, "erase").await;

    let subject_uid = uid("erase-subject");
    let subject = bootstrap(&harness.app, &subject_uid, "파기대상").await;
    seed_transaction(
        &harness.pool,
        store.id,
        store.owner_user_id,
        store.policy_id,
        subject,
        Utc::now().date_naive() - Duration::days(1),
    )
    .await;

    let case_id = open_case(&harness, &admin_uid, Some(subject)).await;

    let requested = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/privacy/erasures",
        &admin_uid,
        Some(json!({
            "subject_user_id": subject,
            "case_id": case_id,
            "reason": "본인 삭제 요청",
        })),
    )
    .await;
    let erasure = requested.expect_ok("request erasure");
    let erasure_id = Uuid::parse_str(erasure["id"].as_str().expect("id")).expect("uuid");
    assert_eq!(erasure["status"], "PENDING");

    harness
        .state
        .privacy
        .execute(&harness.pool, erasure_id)
        .await
        .expect("execute the erasure");

    let subject_row: (String, String, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT display_name, status::text, tombstoned_at FROM coupon.users WHERE id = $1",
    )
    .bind(subject)
    .fetch_one(&harness.pool)
    .await
    .expect("the user row survives");

    assert!(
        subject_row.0.starts_with("탈퇴회원-"),
        "the row carries a pseudonym: {}",
        subject_row.0
    );
    assert_eq!(subject_row.1, "WITHDRAWN");
    assert!(subject_row.2.is_some());

    let email: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT primary_email_ciphertext FROM coupon.users WHERE id = $1")
            .bind(subject)
            .fetch_one(&harness.pool)
            .await
            .expect("read the email column");
    assert!(email.is_none(), "the contact detail is gone");

    // §17.1 keeps the transaction record, and it still resolves to the tombstone.
    let transactions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_transactions WHERE user_id = $1",
    )
    .bind(subject)
    .fetch_one(&harness.pool)
    .await
    .expect("count transactions");
    assert_eq!(transactions, 1, "the ledger is not what was erased");
}

#[tokio::test]
async fn a_legal_hold_stops_the_erasure_and_says_so() {
    // §17.3: 법적 보존 또는 분쟁 hold 가 없으면 파기 작업을 큐에 등록한다.
    let harness = harness_or_skip!(json!({ "privacy_deletion_grace_days": 0 }));
    let admin_uid = admin(&harness, "hold-admin").await;
    let subject = bootstrap(&harness.app, &uid("hold-subject"), "보류대상").await;
    let case_id = open_case(&harness, &admin_uid, Some(subject)).await;

    send(
        &harness.app,
        "PATCH",
        &format!("/api/coupon/v1/admin/cases/{case_id}"),
        &admin_uid,
        Some(json!({
            "legal_hold_until": (Utc::now() + Duration::days(30)).to_rfc3339(),
            "reason": "분쟁 진행 중",
        })),
    )
    .await
    .expect_ok("apply the hold");

    let refused = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/privacy/erasures",
        &admin_uid,
        Some(json!({
            "subject_user_id": subject,
            "case_id": case_id,
            "reason": "본인 삭제 요청",
        })),
    )
    .await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.error_code(), "LEGAL_HOLD_ACTIVE");

    let name: String = sqlx::query_scalar("SELECT display_name FROM coupon.users WHERE id = $1")
        .bind(subject)
        .fetch_one(&harness.pool)
        .await
        .expect("read the subject");
    assert_eq!(name, "보류대상", "nothing was erased");
}

#[tokio::test]
async fn replaying_the_deletion_ledger_re_erases_a_restored_subject() {
    // §18.5: 복원 후 deletion ledger 를 재적용해 이미 파기된 사용자가 되살아나지 않게 한다.
    let harness = harness_or_skip!(json!({ "privacy_deletion_grace_days": 0 }));
    let admin_uid = admin(&harness, "restore-admin").await;
    let subject = bootstrap(&harness.app, &uid("restore-subject"), "복원대상").await;
    let case_id = open_case(&harness, &admin_uid, Some(subject)).await;

    let erasure = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/privacy/erasures",
        &admin_uid,
        Some(json!({
            "subject_user_id": subject,
            "case_id": case_id,
            "reason": "본인 삭제 요청",
        })),
    )
    .await;
    let erasure_id = Uuid::parse_str(
        erasure.expect_ok("request erasure")["id"]
            .as_str()
            .expect("id"),
    )
    .expect("uuid");

    let executed = harness
        .state
        .privacy
        .execute(&harness.pool, erasure_id)
        .await
        .expect("execute");
    let pseudonym = executed.pseudonym_label.clone().expect("a pseudonym");

    // Simulate a restore from backup: the person is back.
    sqlx::query(
        "UPDATE coupon.users
         SET display_name = '복원대상', status = 'ACTIVE', withdrawn_at = NULL,
             tombstoned_at = NULL, pseudonym_label = NULL, firebase_uid = $2
         WHERE id = $1",
    )
    .bind(subject)
    .bind(format!("restored-{}", Uuid::new_v4().simple()))
    .execute(&harness.pool)
    .await
    .expect("restore the subject");

    let result = harness
        .state
        .privacy
        .reapply(&harness.pool)
        .await
        .expect("reapply");
    assert!(
        result.reapplied >= 1,
        "the restore brought at least one erased subject back: {result:?}"
    );

    let after: (String, String) =
        sqlx::query_as("SELECT display_name, status::text FROM coupon.users WHERE id = $1")
            .bind(subject)
            .fetch_one(&harness.pool)
            .await
            .expect("read the subject");

    assert_eq!(
        after.0, pseudonym,
        "the replay reproduces the same tombstone rather than minting a second identity"
    );
    assert_eq!(after.1, "WITHDRAWN");

    // Running it again is a no-op: nobody is alive to re-erase.
    let idempotent = harness
        .state
        .privacy
        .reapply(&harness.pool)
        .await
        .expect("reapply again");
    let subject_still_erased: String =
        sqlx::query_scalar("SELECT display_name FROM coupon.users WHERE id = $1")
            .bind(subject)
            .fetch_one(&harness.pool)
            .await
            .expect("read the subject");
    assert_eq!(subject_still_erased, pseudonym);
    assert!(idempotent.examined >= result.examined);
}

#[tokio::test]
async fn retention_periods_are_configuration_and_the_change_is_audited() {
    // §17.3: 보존기간을 설정 테이블로 관리한다.
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "retention-admin").await;

    let listed = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/admin/retention-policies",
        &admin_uid,
        None,
    )
    .await;
    let policies = listed.expect_ok("list policies")["policies"]
        .as_array()
        .expect("policies")
        .clone();
    assert!(
        policies.iter().any(|policy| policy["data_category"] == "TRANSACTION"),
        "every §17.3 category is present: {policies:?}"
    );

    let updated = send(
        &harness.app,
        "PATCH",
        "/api/coupon/v1/admin/retention-policies/NOTIFICATION",
        &admin_uid,
        Some(json!({
            "retention_days": 90,
            "legal_basis": "발송 이력 보관 기간 단축 결정",
            "reason": "법무 검토 결과 반영",
        })),
    )
    .await;
    assert_eq!(updated.expect_ok("update policy")["retention_days"], 90);

    let audit = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/admin/audit-logs?action=retention_policy.updated&limit=5",
        &admin_uid,
        None,
    )
    .await;
    assert!(
        !audit.expect_ok("audit")["items"]
            .as_array()
            .expect("items")
            .is_empty(),
        "shortening a retention period is a decision somebody answers for"
    );

    // Put it back so a re-run of the suite starts from the seeded value.
    send(
        &harness.app,
        "PATCH",
        "/api/coupon/v1/admin/retention-policies/NOTIFICATION",
        &admin_uid,
        Some(json!({
            "retention_days": 180,
            "legal_basis": "발송 이력·수신 거부 증빙. §23.2 법률 검토 전 잠정값.",
            "reason": "테스트 원복",
        })),
    )
    .await
    .expect_ok("restore policy");
}

// ---------------------------------------------------------------------------
// Pagination (§11.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_campaign_list_pages_with_a_cursor() {
    // §11.1: 기본 20, 최대 100. Phase 3 returned every campaign a shop had ever run.
    let harness = harness_or_skip!();
    let store = active_store(&harness, "paging").await;

    for index in 0..3 {
        send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/campaigns",
            &store.owner_uid,
            Some(json!({
                "name": format!("캠페인 {index}"),
                "customer_description": "3,000원 할인",
                "benefit": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 3000 },
                "minimum_order_amount": 0,
                "issue_mode": "FIRST_COME",
                "audience_type": "ALL_CUSTOMERS",
                "total_quantity": { "mode": "LIMITED", "quantity": 10 },
                "per_user_quantity": 1,
                "issue_starts_at": "2020-01-01T00:00:00Z",
                "issue_ends_at": "2099-01-01T00:00:00Z",
                "usable_until": "2099-06-01T00:00:00Z",
            })),
        )
        .await
        .expect_ok("create campaign");
    }

    let first = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/owner/campaigns?limit=2",
        &store.owner_uid,
        None,
    )
    .await;
    let page = first.expect_ok("first page");

    assert_eq!(page["items"].as_array().expect("items").len(), 2);
    assert_eq!(page["has_more"], true);
    let cursor = page["next_cursor"].as_str().expect("a cursor").to_owned();

    let second = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/owner/campaigns?limit=2&cursor={cursor}"),
        &store.owner_uid,
        None,
    )
    .await;
    let page = second.expect_ok("second page");

    assert_eq!(page["items"].as_array().expect("items").len(), 1);
    assert_eq!(page["has_more"], false);
    assert!(page["next_cursor"].is_null());

    let malformed = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/owner/campaigns?cursor=not-a-cursor",
        &store.owner_uid,
        None,
    )
    .await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
    assert_eq!(malformed.error_code(), "INVALID_CURSOR");
}

// ---------------------------------------------------------------------------
// Observability (§18.4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_operations_metrics_report_every_18_4_signal() {
    let harness = harness_or_skip!();
    let admin_uid = admin(&harness, "metrics-admin").await;

    let response = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/admin/metrics",
        &admin_uid,
        None,
    )
    .await;
    let metrics = response.expect_ok("metrics");

    for pointer in [
        "/process/error_rate",
        "/process/invariant_violations",
        "/queues/campaign_backlog",
        "/queues/outbox_unpublished_age_secs",
        "/queues/dead_letters_last_hour",
        "/notifications/pending_deliveries",
        "/notifications/provider_failure_rate_1h",
    ] {
        assert!(
            metrics.pointer(pointer).is_some(),
            "{pointer} must be reported: {metrics}"
        );
    }

    // The Prometheus surface is unauthenticated infrastructure, like the health probes.
    let scrape = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .expect("valid request");
    let scraped = harness
        .app
        .clone()
        .oneshot(scrape)
        .await
        .expect("router responds");
    assert_eq!(scraped.status(), StatusCode::OK);

    let body = axum::body::to_bytes(scraped.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("coupon_requests_total"), "{text}");
    assert!(text.contains("coupon_invariant_violations_total"), "{text}");
}
