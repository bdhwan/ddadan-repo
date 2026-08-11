//! End-to-end tests over the real router and a real PostgreSQL.
//!
//! These assert the behaviour that only shows up once the middleware stack, the
//! extractors and the database constraints are all in play — idempotent replay, key
//! reuse, optimistic concurrency, the one-store-per-owner invariant.
//!
//! They need a migrated database and are skipped without one:
//!
//! ```sh
//! docker compose -f docker-compose.dev.yml up -d
//! DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon sqlx migrate run
//! COUPON_TEST_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon cargo test
//! ```

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use coupon_api_server::config::Config;
use coupon_api_server::crypto::{LookupHash, Sealer};
use coupon_api_server::state::AppState;
use coupon_api_server::{db, http};
use tower::ServiceExt;
use uuid::Uuid;

/// Build the full application against the test database, or `None` when the environment
/// has not provided one.
async fn test_app() -> Option<Router> {
    let database_url = std::env::var("COUPON_TEST_DATABASE_URL").ok()?;

    let config: Config = serde_json::from_value(serde_json::json!({
        "env": "test",
        "database_url": database_url,
        "firebase_project_id": "ddadan-test",
        // Exercised through the bypass: a real Firebase ID token cannot be minted in a
        // unit test, and the bypass path shares every downstream code path with it.
        "auth_dev_bypass": true,
    }))
    .expect("test configuration");

    let pool = db::connect(&config)
        .await
        .expect("connect to the test database");

    let sealer = Sealer::from_config(&config).expect("sealer");
    let lookup_hash = LookupHash::from_config(&config).expect("lookup hash");
    let state =
        AppState::new(Arc::new(config), pool, None, sealer, lookup_hash).expect("build state");

    Some(http::router::build(state))
}

/// Skip with a visible note rather than passing silently on a machine with no database.
macro_rules! app_or_skip {
    () => {
        match test_app().await {
            Some(app) => app,
            None => {
                eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
                return;
            }
        }
    };
}

fn unique_uid(label: &str) -> String {
    format!("test-{label}-{}", Uuid::new_v4().simple())
}

struct Response {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    json: serde_json::Value,
}

impl Response {
    fn data(&self) -> &serde_json::Value {
        &self.json["data"]
    }

    fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    fn request_id(&self) -> &str {
        self.json
            .get("request_id")
            .or_else(|| self.json["error"].get("request_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }
}

async fn send(
    app: &Router,
    method: &str,
    path: &str,
    uid: &str,
    idempotency_key: Option<Uuid>,
    body: Option<serde_json::Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-dev-firebase-uid", uid)
        .header("content-type", "application/json");

    if let Some(key) = idempotency_key {
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
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");

    Response {
        status,
        headers,
        json: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    }
}

#[tokio::test]
async fn health_endpoints_report_the_migrated_schema() {
    let app = app_or_skip!();

    let live = send(&app, "GET", "/api/coupon/v1/health/live", "", None, None).await;
    assert_eq!(live.status, StatusCode::OK);
    assert_eq!(live.json["status"], "live");

    let ready = send(&app, "GET", "/api/coupon/v1/health/ready", "", None, None).await;
    assert_eq!(ready.status, StatusCode::OK, "body: {}", ready.json);
    assert_eq!(ready.json["status"], "ready");
    assert_eq!(ready.json["postgres"]["status"], "ok");
    assert_eq!(
        ready.json["migration_version"],
        ready.json["expected_migration_version"]
    );
    // Redis is absent in this fixture, and that must not fail readiness (§18.2).
    assert_eq!(ready.json["redis"]["status"], "disabled");
}

#[tokio::test]
async fn bootstrap_creates_an_account_once_and_then_reports_it() {
    let app = app_or_skip!();
    let uid = unique_uid("bootstrap");

    let created = send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "통합 테스터" })),
    )
    .await;

    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.data()["display_name"], "통합 테스터");
    assert_eq!(created.data()["roles"][0], "CONSUMER");
    assert!(created.request_id().starts_with("req_"));
    assert!(
        created.json["transaction_id"].is_string(),
        "a successful mutation must carry a transaction_id (§11.1)"
    );

    // A second call with a different key is still not a second account.
    let again = send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "다른 이름" })),
    )
    .await;

    assert_eq!(again.status, StatusCode::OK, "a repeat is not a creation");
    assert_eq!(again.data()["id"], created.data()["id"]);
    assert_eq!(
        again.data()["display_name"],
        "통합 테스터",
        "bootstrap must not overwrite an existing profile"
    );
}

#[tokio::test]
async fn a_signed_in_user_without_an_account_cannot_read_me() {
    let app = app_or_skip!();

    let response = send(
        &app,
        "GET",
        "/api/coupon/v1/me",
        &unique_uid("no-account"),
        None,
        None,
    )
    .await;

    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.error_code(), "USER_NOT_FOUND");
}

#[tokio::test]
async fn mutations_require_an_idempotency_key() {
    let app = app_or_skip!();
    let uid = unique_uid("no-key");

    let response = send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        None,
        Some(serde_json::json!({})),
    )
    .await;

    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.error_code(), "IDEMPOTENCY_KEY_REQUIRED");
}

#[tokio::test]
async fn an_identical_retry_replays_and_a_different_body_conflicts() {
    let app = app_or_skip!();
    let uid = unique_uid("idempotency");

    send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "원본" })),
    )
    .await;

    let key = Uuid::new_v4();
    let body = serde_json::json!({ "display_name": "변경됨" });

    let first = send(
        &app,
        "PATCH",
        "/api/coupon/v1/me",
        &uid,
        Some(key),
        Some(body.clone()),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);

    let replay = send(
        &app,
        "PATCH",
        "/api/coupon/v1/me",
        &uid,
        Some(key),
        Some(body),
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(
        replay
            .headers
            .get("idempotent-replay")
            .map(|v| v.to_str().unwrap()),
        Some("true"),
        "a replay must be marked so the client knows nothing new happened"
    );
    assert_eq!(
        replay.json["transaction_id"], first.json["transaction_id"],
        "a replay returns the original transaction, not a new one"
    );

    let reused = send(
        &app,
        "PATCH",
        "/api/coupon/v1/me",
        &uid,
        Some(key),
        Some(serde_json::json!({ "display_name": "전혀 다른 값" })),
    )
    .await;

    assert_eq!(reused.status, StatusCode::CONFLICT);
    assert_eq!(reused.error_code(), "IDEMPOTENCY_KEY_REUSED");
    assert_eq!(reused.json["error"]["retryable"], false);
}

#[tokio::test]
async fn a_stale_version_loses_to_whoever_wrote_first() {
    let app = app_or_skip!();
    let uid = unique_uid("version");

    let created = send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "버전" })),
    )
    .await;
    let version = created.data()["version"].as_i64().expect("version");

    let first = send(
        &app,
        "PATCH",
        "/api/coupon/v1/me",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "첫번째", "version": version })),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.data()["version"], version + 1);

    // Same expected version again: the row has moved on.
    let stale = send(
        &app,
        "PATCH",
        "/api/coupon/v1/me",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "두번째", "version": version })),
    )
    .await;

    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(stale.error_code(), "VERSION_CONFLICT");
    assert_eq!(
        stale.json["error"]["retryable"], true,
        "the client can re-read and retry"
    );
}

#[tokio::test]
async fn consent_is_appended_and_required_scopes_cannot_be_revoked() {
    let app = app_or_skip!();
    let uid = unique_uid("consent");

    send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "동의" })),
    )
    .await;

    let granted = send(
        &app,
        "POST",
        "/api/coupon/v1/me/consents",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({
            "consents": [
                { "scope": "TERMS_OF_SERVICE", "action": "GRANTED", "source": "signup" },
                { "scope": "MARKETING_ALL", "action": "GRANTED", "source": "settings/alerts" },
            ]
        })),
    )
    .await;
    assert_eq!(granted.status, StatusCode::OK);

    let find = |response: &Response, scope: &str| -> bool {
        response.data()["consents"]
            .as_array()
            .expect("consents array")
            .iter()
            .find(|entry| entry["scope"] == scope)
            .map(|entry| entry["granted"] == true)
            .unwrap_or(false)
    };
    assert!(find(&granted, "TERMS_OF_SERVICE"));
    assert!(find(&granted, "MARKETING_ALL"));

    // Revoking optional marketing is a new event, not an edit.
    let revoked = send(
        &app,
        "POST",
        "/api/coupon/v1/me/consents",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({
            "consents": [{ "scope": "MARKETING_ALL", "action": "REVOKED", "source": "settings/alerts" }]
        })),
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK);
    assert!(!find(&revoked, "MARKETING_ALL"));
    assert!(
        find(&revoked, "TERMS_OF_SERVICE"),
        "unrelated consent is untouched"
    );

    let refused = send(
        &app,
        "POST",
        "/api/coupon/v1/me/consents",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({
            "consents": [{ "scope": "TERMS_OF_SERVICE", "action": "REVOKED", "source": "settings" }]
        })),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn an_owner_gets_one_store_and_walks_it_to_review() {
    let app = app_or_skip!();
    let uid = unique_uid("store");
    let slug = format!("test-{}", Uuid::new_v4().simple());

    send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "점주" })),
    )
    .await;

    let missing = send(&app, "GET", "/api/coupon/v1/owner/store", &uid, None, None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.error_code(), "STORE_NOT_FOUND");

    let created = send(
        &app,
        "POST",
        "/api/coupon/v1/owner/store",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "name": "테스트 상점", "slug": slug })),
    )
    .await;
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "body: {}",
        created.json
    );
    assert_eq!(created.data()["status"], "DRAFT");
    assert_eq!(created.data()["business_profile_complete"], false);

    // §12.6-1: one active store per owner.
    let second = send(
        &app,
        "POST",
        "/api/coupon/v1/owner/store",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({
            "name": "두번째 상점",
            "slug": format!("test-{}", Uuid::new_v4().simple()),
        })),
    )
    .await;
    assert_eq!(second.status, StatusCode::CONFLICT);
    assert_eq!(second.error_code(), "STORE_ALREADY_EXISTS");

    // Creating a store grants the owner role.
    let roles = send(&app, "GET", "/api/coupon/v1/me/roles", &uid, None, None).await;
    let owner_role = roles.data()["roles"]
        .as_array()
        .expect("roles")
        .iter()
        .find(|role| role["role"] == "STORE_OWNER")
        .expect("STORE_OWNER granted with the first store");
    assert_eq!(owner_role["store_id"], created.data()["id"]);

    let premature = send(
        &app,
        "POST",
        "/api/coupon/v1/owner/store/submit-review",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(premature.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(premature.error_code(), "STORE_NOT_READY_FOR_REVIEW");

    let completed = send(
        &app,
        "PATCH",
        "/api/coupon/v1/owner/store",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({
            "address": { "road": "성수이로 1" },
            "business_profile": {
                "registration_no": "123-45-67890",
                "representative_name": "김대표",
            },
        })),
    )
    .await;
    assert_eq!(completed.status, StatusCode::OK, "body: {}", completed.json);
    assert_eq!(completed.data()["business_profile_complete"], true);

    let submitted = send(
        &app,
        "POST",
        "/api/coupon/v1/owner/store/submit-review",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "note": "검수 부탁드립니다" })),
    )
    .await;
    assert_eq!(submitted.status, StatusCode::OK, "body: {}", submitted.json);
    assert_eq!(submitted.data()["status"], "PENDING_REVIEW");
    assert_eq!(submitted.data()["latest_review"]["status"], "PENDING");

    // A store under review is frozen.
    let frozen = send(
        &app,
        "PATCH",
        "/api/coupon/v1/owner/store",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "name": "수정 시도" })),
    )
    .await;
    assert_eq!(frozen.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(frozen.error_code(), "INVALID_STATE_TRANSITION");
}

#[tokio::test]
async fn a_slug_already_in_use_is_reported_as_such() {
    let app = app_or_skip!();
    let slug = format!("test-{}", Uuid::new_v4().simple());

    for (index, uid) in [unique_uid("slug-a"), unique_uid("slug-b")]
        .iter()
        .enumerate()
    {
        send(
            &app,
            "POST",
            "/api/coupon/v1/users/bootstrap",
            uid,
            Some(Uuid::new_v4()),
            Some(serde_json::json!({ "display_name": "점주" })),
        )
        .await;

        let response = send(
            &app,
            "POST",
            "/api/coupon/v1/owner/store",
            uid,
            Some(Uuid::new_v4()),
            Some(serde_json::json!({ "name": "같은 주소", "slug": slug })),
        )
        .await;

        if index == 0 {
            assert_eq!(response.status, StatusCode::CREATED);
        } else {
            assert_eq!(response.status, StatusCode::CONFLICT);
            assert_eq!(response.error_code(), "STORE_SLUG_TAKEN");
        }
    }
}

#[tokio::test]
async fn a_high_risk_endpoint_refuses_a_stale_sign_in() {
    let app = app_or_skip!();
    let uid = unique_uid("reauth");

    send(
        &app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &uid,
        Some(Uuid::new_v4()),
        Some(serde_json::json!({ "display_name": "재인증" })),
    )
    .await;

    let request = Request::builder()
        .method("POST")
        .uri("/api/coupon/v1/owner/store/submit-review")
        .header("x-dev-firebase-uid", &uid)
        // Older than the 10-minute `auth_time` window (§9.3).
        .header("x-dev-auth-age-secs", "9000")
        .header("idempotency-key", Uuid::new_v4().to_string())
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("valid request");

    let response = app.oneshot(request).await.expect("router responds");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn every_response_carries_a_request_id() {
    let app = app_or_skip!();

    let unauthenticated = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/coupon/v1/me")
                .body(Body::empty())
                .expect("valid request"),
        )
        .await
        .expect("router responds");

    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    assert!(
        unauthenticated.headers().contains_key("x-request-id"),
        "the id must be readable even when the body is not parsed"
    );

    let bytes = axum::body::to_bytes(unauthenticated.into_body(), 64 * 1024)
        .await
        .expect("body");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("error envelope");

    assert_eq!(json["error"]["code"], "UNAUTHENTICATED");
    assert!(
        json["error"]["request_id"]
            .as_str()
            .expect("request_id")
            .starts_with("req_"),
        "the error body carries the id too (§11.1)"
    );
}
