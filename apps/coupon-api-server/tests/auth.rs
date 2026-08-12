//! 인증 스위트 — 이메일 인증 승격과 카카오 OIDC 로그인 (§9.2, §11.2, AUTH-002, AUTH-003).
//!
//! ## 카카오는 왜 mock 인가
//!
//! §19.3 은 카카오를 **contract mock** 으로 검증하라고 한다. 실제 카카오 앱 키·시크릿·
//! 리다이렉트 URI 가 아직 등록되지 않았기도 하지만, 그보다 여기서 확인하고 싶은 것이
//! "카카오 서버가 살아 있는가"가 아니라 "우리가 §9.2 의 여덟 단계를 정확히 밟는가"이기
//! 때문이다. state 재사용, nonce 불일치, JWKS `kid` 회전, 교환 코드 2회 사용 — 실제
//! 카카오를 상대로는 만들어 내기 어렵거나 불가능한 상황들이고, 정작 틀리기 쉬운 곳도
//! 거기다.
//!
//! mock 이 흉내 내지 *않는* 것이 하나 있다: `iss`. mock 이 서명하는 `id_token` 도
//! `iss=https://kauth.kakao.com` 을 달고 오고, 서버는 그 값을 설정이 아니라 상수로
//! 검사한다(`COUPON_KAKAO_OIDC_BASE_URL` 은 **어디로 요청을 보낼지**만 바꾼다). 그래서
//! 이 스위트가 통과한다는 것은 진짜 issuer 검사를 통과했다는 뜻이지, issuer 검사를
//! 우회했다는 뜻이 아니다.
//!
//! ```sh
//! ./scripts/coupon/db-up.sh
//! ./scripts/coupon/test.sh --test auth
//! ```

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::{Form, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use common::{Harness, Response, harness_or_skip, harness_with, send};
use coupon_api_server::auth::custom_token::IDENTITY_TOOLKIT_AUDIENCE;
use coupon_api_server::auth::kakao::KAKAO_ISSUER;
use hmac::{Hmac, Mac};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Fixtures: throwaway RSA keys. See `tests/fixtures/README.md`.
// ---------------------------------------------------------------------------

const KAKAO_KEY_1_PEM: &str = include_str!("fixtures/kakao-signing-key-1.pem");
const KAKAO_KEY_1_JWK: &str = include_str!("fixtures/kakao-signing-key-1.jwk.json");
const KAKAO_KEY_2_PEM: &str = include_str!("fixtures/kakao-signing-key-2.pem");
const KAKAO_KEY_2_JWK: &str = include_str!("fixtures/kakao-signing-key-2.jwk.json");
const FIREBASE_KEY_PEM: &str = include_str!("fixtures/firebase-service-account-key.pem");
const FIREBASE_KEY_PUB_PEM: &str = include_str!("fixtures/firebase-service-account-key.pub.pem");

const KAKAO_CLIENT_ID: &str = "test-kakao-rest-key";
const KAKAO_REDIRECT_URI: &str = "https://app.ddadan.test/auth/kakao/callback";
const KAKAO_WEBHOOK_SECRET: &str = "kakao-webhook-secret";
const SERVICE_ACCOUNT_EMAIL: &str = "coupon-signer@ddadan-test.iam.gserviceaccount.com";

const KID_1: &str = "mock-key-1";
const KID_2: &str = "mock-key-2";

// ---------------------------------------------------------------------------
// The Kakao contract mock
// ---------------------------------------------------------------------------

/// What the mock's token endpoint should do on the next call.
#[derive(Clone)]
enum TokenPlan {
    /// Sign these claims with `kid` and return them as an `id_token`.
    IdToken { claims: Value, kid: String },
    /// Answer with this status and body instead.
    Failure { status: u16, body: String },
}

#[derive(Default)]
struct MockCounters {
    jwks_fetches: AtomicUsize,
    token_calls: AtomicUsize,
}

struct MockState {
    /// Which key the JWKS advertises right now. Rotating this is how §9.2-4 gets tested.
    published_kid: Mutex<String>,
    plan: Mutex<Option<TokenPlan>>,
    /// The form the token endpoint last received, so a test can assert PKCE actually
    /// travelled.
    last_token_form: Mutex<Option<HashMap<String, String>>>,
    counters: MockCounters,
    base_url: Mutex<String>,
}

struct KakaoMock {
    base_url: String,
    state: Arc<MockState>,
}

impl KakaoMock {
    /// Bind an ephemeral port and serve Kakao's three OIDC endpoints.
    async fn start() -> Self {
        let state = Arc::new(MockState {
            published_kid: Mutex::new(KID_1.to_owned()),
            plan: Mutex::new(None),
            last_token_form: Mutex::new(None),
            counters: MockCounters::default(),
            base_url: Mutex::new(String::new()),
        });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the kakao mock");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        *state.base_url.lock().expect("base url") = base_url.clone();

        let app = Router::new()
            .route("/.well-known/openid-configuration", get(mock_discovery))
            .route("/.well-known/jwks.json", get(mock_jwks))
            .route("/oauth/token", post(mock_token))
            .with_state(state.clone());

        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { base_url, state }
    }

    /// Have the next token call return an `id_token` with these claims, signed by the
    /// currently published key.
    fn will_return_id_token(&self, claims: Value) {
        let kid = self.state.published_kid.lock().expect("kid").clone();
        *self.state.plan.lock().expect("plan") = Some(TokenPlan::IdToken { claims, kid });
    }

    /// Have the next token call sign with a key the JWKS does not advertise yet.
    fn will_sign_with(&self, kid: &str, claims: Value) {
        *self.state.plan.lock().expect("plan") = Some(TokenPlan::IdToken {
            claims,
            kid: kid.to_owned(),
        });
    }

    fn will_fail(&self, status: u16, body: &str) {
        *self.state.plan.lock().expect("plan") = Some(TokenPlan::Failure {
            status,
            body: body.to_owned(),
        });
    }

    /// Publish a different signing key, as Kakao does when it rotates.
    fn rotate_to(&self, kid: &str) {
        *self.state.published_kid.lock().expect("kid") = kid.to_owned();
    }

    fn jwks_fetches(&self) -> usize {
        self.state.counters.jwks_fetches.load(Ordering::SeqCst)
    }

    fn token_calls(&self) -> usize {
        self.state.counters.token_calls.load(Ordering::SeqCst)
    }

    fn last_token_form(&self) -> HashMap<String, String> {
        self.state
            .last_token_form
            .lock()
            .expect("form")
            .clone()
            .expect("the token endpoint was called")
    }
}

async fn mock_discovery(State(state): State<Arc<MockState>>) -> Json<Value> {
    let base = state.base_url.lock().expect("base").clone();
    Json(json!({
        // Kakao's real issuer, served from wherever the mock happens to live. The server
        // checks the issuer against a constant, so this is the value that must match.
        "issuer": KAKAO_ISSUER,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "jwks_uri": format!("{base}/.well-known/jwks.json"),
    }))
}

async fn mock_jwks(State(state): State<Arc<MockState>>) -> Json<Value> {
    state.counters.jwks_fetches.fetch_add(1, Ordering::SeqCst);

    let kid = state.published_kid.lock().expect("kid").clone();
    let raw = if kid == KID_1 {
        KAKAO_KEY_1_JWK
    } else {
        KAKAO_KEY_2_JWK
    };

    let mut jwk: Value = serde_json::from_str(raw).expect("jwk fixture");
    jwk["kid"] = json!(kid);
    Json(json!({ "keys": [jwk] }))
}

async fn mock_token(
    State(state): State<Arc<MockState>>,
    Form(form): Form<HashMap<String, String>>,
) -> axum::response::Response {
    state.counters.token_calls.fetch_add(1, Ordering::SeqCst);
    *state.last_token_form.lock().expect("form") = Some(form);

    let plan = state.plan.lock().expect("plan").clone();
    match plan {
        Some(TokenPlan::IdToken { claims, kid }) => {
            let pem = if kid == KID_1 {
                KAKAO_KEY_1_PEM
            } else {
                KAKAO_KEY_2_PEM
            };
            let mut header = Header::new(Algorithm::RS256);
            header.kid = Some(kid);
            let key = EncodingKey::from_rsa_pem(pem.as_bytes()).expect("mock signing key");
            let id_token = jsonwebtoken::encode(&header, &claims, &key).expect("sign id_token");

            // The real response also carries `access_token` and `refresh_token`. They are
            // here precisely so a test can prove they go nowhere.
            Json(json!({
                "token_type": "bearer",
                "id_token": id_token,
                "access_token": "kakao-access-token-that-must-be-discarded",
                "refresh_token": "kakao-refresh-token-that-must-be-discarded",
                "expires_in": 21599,
            }))
            .into_response()
        }
        Some(TokenPlan::Failure { status, body }) => (
            StatusCode::from_u16(status).expect("status"),
            [("content-type", "application/json")],
            body,
        )
            .into_response(),
        None => panic!("the kakao mock was called with no plan set"),
    }
}

/// Claims a well-behaved Kakao would put in an `id_token`.
fn kakao_claims(subject: &str, nonce: &str, email: Option<&str>) -> Value {
    let now = Utc::now().timestamp();
    json!({
        "iss": KAKAO_ISSUER,
        "aud": KAKAO_CLIENT_ID,
        "sub": subject,
        "iat": now,
        "exp": now + 600,
        "auth_time": now,
        "nonce": nonce,
        "email": email,
        "email_verified": email.is_some(),
    })
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct KakaoHarness {
    harness: Harness,
    mock: KakaoMock,
}

/// A harness with Kakao wired to a fresh mock. `overrides` tunes the server config.
async fn kakao_harness_with(overrides: Value) -> Option<KakaoHarness> {
    let mock = KakaoMock::start().await;

    let mut settings = json!({
        "kakao_client_id": KAKAO_CLIENT_ID,
        "kakao_redirect_uri": KAKAO_REDIRECT_URI,
        "kakao_webhook_secret": KAKAO_WEBHOOK_SECRET,
        "kakao_oidc_base_url": mock.base_url,
        "firebase_service_account_email": SERVICE_ACCOUNT_EMAIL,
        "firebase_service_account_private_key": FIREBASE_KEY_PEM,
        // §16.4 limits are lifted unless a test is about one of them.
        "rate_limit_login_start_per_10min": 100_000,
        "rate_limit_kakao_callback_failure_per_10min": 100_000,
    });
    for (key, value) in overrides.as_object().expect("overrides object") {
        settings[key] = value.clone();
    }

    Some(KakaoHarness {
        harness: harness_with(settings).await?,
        mock,
    })
}

macro_rules! kakao_or_skip {
    () => {
        kakao_or_skip!(serde_json::json!({}))
    };
    ($overrides:expr) => {
        match kakao_harness_with($overrides).await {
            Some(harness) => harness,
            None => {
                eprintln!("skipping: COUPON_TEST_DATABASE_URL is not set");
                return;
            }
        }
    };
}

impl KakaoHarness {
    /// §9.2 steps 1–2. Returns `(state, nonce)` read back out of the authorize URL —
    /// which is exactly what the browser and then Kakao would carry.
    async fn authorize(&self) -> (String, String) {
        let response = send(
            &self.harness.app,
            "GET",
            "/api/coupon/v1/auth/kakao/authorize",
            "anonymous",
            None,
        )
        .await;
        let data = response.expect_ok("authorize");

        let url = data["authorize_url"].as_str().expect("authorize_url");
        let params = query_params(url);

        assert_eq!(
            params.get("code_challenge_method").map(String::as_str),
            Some("S256"),
            "§9.2-2 asks for PKCE: {url}"
        );
        assert_eq!(
            params.get("redirect_uri").map(String::as_str),
            Some(KAKAO_REDIRECT_URI)
        );

        (
            data["state"].as_str().expect("state").to_owned(),
            params.get("nonce").expect("nonce").clone(),
        )
    }

    /// §9.2 steps 3–5, with whatever query Kakao is pretending to send.
    async fn callback(&self, query: &str) -> Response {
        send(
            &self.harness.app,
            "GET",
            &format!("/api/coupon/v1/auth/kakao/callback?{query}"),
            "anonymous",
            None,
        )
        .await
    }

    /// The whole authorize → callback leg for a Kakao account, ending in an exchange
    /// code. `email` is `None` for a member who declined to share one (AUTH-002).
    async fn sign_in_through_kakao(&self, subject: &str, email: Option<&str>) -> String {
        let (state, nonce) = self.authorize().await;
        self.mock
            .will_return_id_token(kakao_claims(subject, &nonce, email));

        let response = self
            .callback(&format!("code=kakao-auth-code&state={state}"))
            .await;
        response.expect_ok("callback")["exchange_code"]
            .as_str()
            .expect("exchange_code")
            .to_owned()
    }

    /// §9.2 steps 6–7.
    async fn exchange(&self, exchange_code: &str) -> Response {
        send(
            &self.harness.app,
            "POST",
            "/api/coupon/v1/auth/kakao/exchange",
            "anonymous",
            Some(json!({ "exchange_code": exchange_code })),
        )
        .await
    }
}

fn query_params(url: &str) -> HashMap<String, String> {
    let query = url.split_once('?').map(|(_, q)| q).unwrap_or_default();
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), urldecode(value)))
        .collect()
}

fn urldecode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The claims inside a Firebase custom token, verified against the signing key.
#[derive(Debug, Deserialize)]
struct CustomTokenClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
    uid: String,
}

fn decode_custom_token(token: &str) -> CustomTokenClaims {
    let key = DecodingKey::from_rsa_pem(FIREBASE_KEY_PUB_PEM.as_bytes()).expect("public key");
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[IDENTITY_TOOLKIT_AUDIENCE]);
    validation.set_issuer(&[SERVICE_ACCOUNT_EMAIL]);

    decode::<CustomTokenClaims>(token, &key, &validation)
        .expect("the custom token verifies against the service account key")
        .claims
}

async fn user_status(pool: &PgPool, user_id: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT status::text FROM coupon.users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read the user status")
}

async fn set_status(pool: &PgPool, user_id: Uuid, status: &str) {
    // `ck_users_status_timestamps` demands the matching timestamp for the two states
    // that carry one.
    sqlx::query(
        "UPDATE coupon.users
         SET status = $2::text::coupon.user_status,
             email_verified_at = NULL,
             suspended_at = CASE WHEN $2 = 'SUSPENDED' THEN clock_timestamp() ELSE suspended_at END,
             withdrawn_at = CASE WHEN $2 = 'WITHDRAWN' THEN clock_timestamp() ELSE withdrawn_at END
         WHERE id = $1",
    )
    .bind(user_id)
    .bind(status)
    .execute(pool)
    .await
    .expect("set the user status");
}

/// Sign a webhook body the way Kakao is configured to.
fn sign_webhook(timestamp: &str, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(KAKAO_WEBHOOK_SECRET.as_bytes()).expect("hmac");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

async fn post_unlink_webhook(
    harness: &Harness,
    timestamp: &str,
    body: &str,
    signature: &str,
) -> Response {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let request = Request::builder()
        .method("POST")
        .uri("/api/coupon/v1/webhooks/kakao/unlink")
        .header("content-type", "application/json")
        .header("x-kakao-signature", signature)
        .header("x-kakao-signature-timestamp", timestamp)
        .body(Body::from(body.to_owned()))
        .expect("request");

    let response = harness
        .app
        .clone()
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");

    Response {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

// ===========================================================================
// 1. 이메일 인증 승격
// ===========================================================================

#[tokio::test]
async fn a_verified_email_promotes_the_account_and_brings_it_back_into_every_audience() {
    // 이 테스트의 앞선 형태는 정반대를 못 박고 있었다 — acceptance 스위트의
    // `an_unverified_email_quietly_falls_out_of_every_campaign_audience` 는 결함을
    // 재현해 두는 테스트였다(지금은
    // `verifying_an_email_promotes_the_account_that_was_created_before_it` 로 바뀌었다).
    // `POST /users/bootstrap` 이 처음 본 토큰의 `email_verified` 로 상태를 정하고
    // `ON CONFLICT DO NOTHING` 으로 끝냈기 때문에, 인증 전에 가입한 사람은 나중에 메일을 인증해도
    // `PENDING_VERIFICATION` 에 남았고, 대상자 질의는 `status = 'ACTIVE'` 만 세므로 모든
    // 캠페인에서 조용히 빠졌다. 아무 오류도 나지 않는다는 점이 특히 나빴다.
    //
    // 이제 인증된 토큰으로 다시 bootstrap 하면 승격된다. 승격 그 자체보다, 승격 뒤에
    // *실제로 대상자에 들어오는지* 가 이 테스트의 요점이다.
    let harness = harness_or_skip!();

    let store = common::store(&harness, "auth-promote").await;
    let consumer = common::consumer(&harness.app, "auth-promote").await;

    // 개발용 bypass 토큰은 언제나 `email_verified: true` 라 이 경로를 밟지 않는다.
    // 인증 전에 가입한 사람의 행을 그대로 재현한다.
    set_status(&harness.pool, consumer.user_id, "PENDING_VERIFICATION").await;
    assert_eq!(
        user_status(&harness.pool, consumer.user_id).await,
        "PENDING_VERIFICATION"
    );

    // 대상자로 직접 지목해도 대기 상태에서는 세어지지 않는다.
    let draft = json!({
        "name": "인증 대상 확인",
        "customer_description": "인증을 마친 분께",
        "benefit": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 1000 },
        "issue_mode": "DIRECT",
        "audience_type": "SPECIFIC_USERS",
        "audience_criteria": { "user_ids": [consumer.user_id] },
        "total_quantity": { "mode": "LIMITED", "quantity": 10 },
        "per_user_quantity": 1,
        "issue_starts_at": "2020-01-01T00:00:00Z",
        "issue_ends_at": "2099-01-01T00:00:00Z",
        "usable_until": "2099-06-01T00:00:00Z",
    });
    let campaign_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/campaigns",
        &store.owner_uid,
        Some(draft),
    )
    .await
    .id("draft campaign");

    async fn audience_size(harness: &Harness, owner_uid: &str, campaign_id: Uuid) -> i64 {
        send(
            &harness.app,
            "GET",
            &format!("/api/coupon/v1/owner/campaigns/{campaign_id}/estimate"),
            owner_uid,
            None,
        )
        .await
        .expect_ok("estimate")["audience_size"]
            .as_i64()
            .expect("audience_size")
    }

    assert_eq!(
        audience_size(&harness, &store.owner_uid, campaign_id).await,
        0,
        "인증 전 계정은 대상자에 들어오지 않는다 — 고치기 전의 영구 상태였다"
    );

    // 사용자가 인증 메일을 누르고 앱으로 돌아온다. 앱은 bootstrap 을 다시 부른다.
    let bootstrapped = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/users/bootstrap",
        &consumer.uid,
        Some(json!({ "display_name": "김손님" })),
    )
    .await;
    assert_eq!(
        bootstrapped.expect_ok("re-bootstrap")["status"],
        "ACTIVE",
        "인증된 토큰으로 다시 오면 승격된다"
    );
    assert_eq!(
        bootstrapped.expect_ok("re-bootstrap")["email_verified"],
        true
    );

    assert_eq!(
        audience_size(&harness, &store.owner_uid, campaign_id).await,
        1,
        "승격된 회원은 이제 대상자에 들어온다"
    );

    // §12.5: 상태 변화는 추적 가능해야 한다.
    let audited = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.audit_logs
         WHERE resource_type = 'user' AND resource_id = $1 AND action = 'user.email_verified'",
    )
    .bind(consumer.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count audit rows");
    assert_eq!(audited, 1, "승격이 감사 기록으로 남아야 한다");
}

#[tokio::test]
async fn promotion_is_one_directional_and_cannot_revive_a_suspended_or_withdrawn_account() {
    // 승격은 `PENDING_VERIFICATION → ACTIVE` 한 방향뿐이다. 정지·탈퇴는 누군가 내린
    // 결정이고, 인증된 이메일로 다시 로그인했다는 사실은 그 결정을 되돌릴 근거가 아니다.
    let harness = harness_or_skip!();

    for status in ["SUSPENDED", "WITHDRAWAL_PENDING", "WITHDRAWN"] {
        let consumer = common::consumer(&harness.app, "auth-frozen").await;
        set_status(&harness.pool, consumer.user_id, status).await;

        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/users/bootstrap",
            &consumer.uid,
            Some(json!({ "display_name": "김손님" })),
        )
        .await;

        // bootstrap 은 계정이 있다는 사실만 알려 주고, 상태는 건드리지 않는다.
        assert_eq!(
            response.expect_ok("bootstrap")["status"], status,
            "{status} 는 인증된 토큰으로도 되살아나면 안 된다"
        );
        assert_eq!(
            user_status(&harness.pool, consumer.user_id).await,
            status,
            "{status} 행이 그대로여야 한다"
        );
    }
}

// ===========================================================================
// 2. 카카오 로그인 — 정상 흐름
// ===========================================================================

#[tokio::test]
async fn a_first_kakao_login_creates_a_member_and_returns_a_firebase_custom_token() {
    // AUTH-002 기본 흐름 1–7, 그리고 §9.2-7 이 요구하는 Custom Token 의 모양.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness
        .sign_in_through_kakao(&subject, Some("dahye@kakao.test"))
        .await;

    // PKCE 는 실제로 토큰 요청까지 갔는가 (§9.2-2).
    let form = harness.mock.last_token_form();
    assert_eq!(
        form.get("grant_type").map(String::as_str),
        Some("authorization_code")
    );
    assert!(
        form.get("code_verifier").is_some_and(|v| v.len() >= 32),
        "PKCE verifier 가 교환 요청에 실려야 한다: {form:?}"
    );
    assert_eq!(
        form.get("redirect_uri").map(String::as_str),
        Some(KAKAO_REDIRECT_URI)
    );

    let signed_in = harness.exchange(&code).await;
    let data = signed_in.expect_ok("exchange");
    assert_eq!(data["created"], true, "처음 온 카카오 계정은 회원을 만든다");

    let claims = decode_custom_token(data["custom_token"].as_str().expect("custom_token"));
    assert_eq!(claims.aud, IDENTITY_TOOLKIT_AUDIENCE);
    assert_eq!(claims.iss, SERVICE_ACCOUNT_EMAIL);
    assert_eq!(claims.sub, SERVICE_ACCOUNT_EMAIL);
    assert!(
        claims.exp - claims.iat <= 3600,
        "§9.2-7: 최대 1시간 — {}초는 너무 길다",
        claims.exp - claims.iat
    );

    // canonical Firebase UID 로 회원 행이 만들어졌고, 그게 토큰의 uid 다.
    let user_id = Uuid::parse_str(data["user_id"].as_str().expect("user_id")).expect("uuid");
    let firebase_uid = sqlx::query_scalar::<_, String>(
        "SELECT firebase_uid FROM coupon.users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&harness.harness.pool)
    .await
    .expect("read the member");
    assert_eq!(claims.uid, firebase_uid);

    // 카카오가 준 access/refresh 토큰은 어디에도 남지 않는다 (§9.2).
    let leaked = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.oauth_exchange_codes
         WHERE email_ciphertext IS NOT NULL
           AND encode(email_ciphertext, 'escape') LIKE '%kakao-access-token%'",
    )
    .fetch_one(&harness.harness.pool)
    .await
    .expect("scan for leaked tokens");
    assert_eq!(leaked, 0);

    // 두 번째 로그인은 같은 회원이다.
    let again = harness
        .sign_in_through_kakao(&subject, Some("dahye@kakao.test"))
        .await;
    let second = harness.exchange(&again).await;
    let second = second.expect_ok("second exchange");
    assert_eq!(second["created"], false);
    assert_eq!(second["user_id"], data["user_id"]);
    assert_eq!(
        decode_custom_token(second["custom_token"].as_str().expect("token")).uid,
        firebase_uid,
        "같은 회원은 언제나 같은 canonical UID 로 로그인한다"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn two_first_logins_racing_for_one_kakao_account_produce_one_member() {
    // 두 탭에서 동시에 첫 로그인을 끝내는 경우. `SELECT ... FOR UPDATE` 는 없는 행을
    // 잠그지 못하므로 둘 다 "신규"라고 판단하고 둘 다 INSERT 한다. unique 인덱스가 그걸
    // 잡아 재시도 가능한 409 로 만들어 주기는 하지만, 처음 가입하는 사람에게 보여 줄
    // 화면은 아니다. 같은 `sub` 에 대한 advisory lock 이 뒤 트랜잭션을 기다리게 해서
    // 앞선 쪽의 회원을 그대로 찾게 한다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    // 두 흐름 모두 끝까지 진행해 각자 교환 코드를 쥔다.
    let first_code = harness.sign_in_through_kakao(&subject, None).await;
    let second_code = harness.sign_in_through_kakao(&subject, None).await;

    let (first, second) = tokio::join!(
        harness.exchange(&first_code),
        harness.exchange(&second_code)
    );

    let first = first.expect_ok("first exchange").clone();
    let second = second.expect_ok("second exchange").clone();

    assert_eq!(
        first["user_id"], second["user_id"],
        "같은 카카오 계정은 한 회원이다"
    );
    assert_eq!(
        [first["created"].as_bool(), second["created"].as_bool()]
            .iter()
            .filter(|created| **created == Some(true))
            .count(),
        1,
        "회원을 만든 쪽은 정확히 하나여야 한다"
    );

    let members = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.auth_identities
         WHERE provider = 'KAKAO' AND provider_subject = $1",
    )
    .bind(&subject)
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert_eq!(members, 1);
}

#[tokio::test]
async fn a_member_who_shared_no_email_can_still_sign_up() {
    // AUTH-002: 이메일 제공 동의가 없으면 이메일 없이 가입할 수 있다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;

    let signed_in = harness.exchange(&code).await;
    let data = signed_in.expect_ok("exchange");
    assert_eq!(data["created"], true);

    let user_id = Uuid::parse_str(data["user_id"].as_str().expect("user_id")).expect("uuid");
    assert_eq!(user_status(&harness.harness.pool, user_id).await, "ACTIVE");
}

// ===========================================================================
// 3. 카카오 로그인 — 보안 검증
// ===========================================================================

#[tokio::test]
async fn a_state_that_was_never_issued_or_has_already_been_spent_is_refused() {
    // AUTH-002: state/nonce 불일치나 재사용된 코드는 보안 사건으로 기록한다.
    let harness = kakao_or_skip!();

    let unknown = harness
        .callback("code=whatever&state=never-issued-by-us")
        .await;
    unknown.expect_error(
        StatusCode::UNAUTHORIZED,
        "KAKAO_SECURITY_CHECK_FAILED",
        "본 적 없는 state",
    );

    // 한 번 쓴 state 는 두 번 쓸 수 없다.
    let (state, nonce) = harness.authorize().await;
    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    harness
        .mock
        .will_return_id_token(kakao_claims(&subject, &nonce, None));
    harness
        .callback(&format!("code=first&state={state}"))
        .await
        .expect_ok("first callback");

    harness
        .mock
        .will_return_id_token(kakao_claims(&subject, &nonce, None));
    let replayed = harness.callback(&format!("code=second&state={state}")).await;
    replayed.expect_error(
        StatusCode::UNAUTHORIZED,
        "KAKAO_SECURITY_CHECK_FAILED",
        "재사용된 state",
    );
}

#[tokio::test]
async fn an_id_token_bound_to_a_different_session_is_refused() {
    // nonce 가 하는 일이 이것 하나다: 다른 세션에서 가로챈 토큰이 이 세션의 로그인을
    // 완성하지 못하게 막는다.
    let harness = kakao_or_skip!();

    let (state, _nonce) = harness.authorize().await;
    harness.mock.will_return_id_token(kakao_claims(
        "kakao-1",
        "a-nonce-from-somebody-elses-login",
        None,
    ));

    let refused = harness.callback(&format!("code=c&state={state}")).await;
    refused.expect_error(
        StatusCode::UNAUTHORIZED,
        "KAKAO_SECURITY_CHECK_FAILED",
        "nonce 불일치",
    );
}

#[tokio::test]
async fn an_id_token_for_another_app_or_issuer_is_refused() {
    let harness = kakao_or_skip!();

    for (label, mutate) in [
        ("audience", json!({ "aud": "somebody-elses-kakao-app" })),
        ("issuer", json!({ "iss": "https://accounts.google.com" })),
    ] {
        let (state, nonce) = harness.authorize().await;
        let mut claims = kakao_claims("kakao-1", &nonce, None);
        for (key, value) in mutate.as_object().expect("object") {
            claims[key] = value.clone();
        }
        harness.mock.will_return_id_token(claims);

        let refused = harness.callback(&format!("code=c&state={state}")).await;
        refused.expect_error(
            StatusCode::UNAUTHORIZED,
            "KAKAO_SECURITY_CHECK_FAILED",
            label,
        );
    }
}

#[tokio::test]
async fn a_rotated_signing_key_costs_exactly_one_extra_jwks_fetch() {
    // §9.2-4: JWKS 는 캐시하되 `kid` 미일치 시 한 번만 갱신한다. "한 번"이 지시의 전부다
    // — 모르는 `kid` 는 회전(한 번 다시 받으면 영원히 해결)이거나 위조(몇 번을 받아도
    // 해결되지 않음)이고, 무한 재시도는 후자를 카카오를 향한 요청 증폭기로 만든다.
    let harness = kakao_or_skip!();

    let first = format!("kakao-{}", Uuid::new_v4().simple());
    harness.sign_in_through_kakao(&first, None).await;
    assert_eq!(harness.mock.jwks_fetches(), 1, "첫 검증에서 한 번 받는다");

    // 두 번째 로그인은 캐시로 해결된다.
    let second = format!("kakao-{}", Uuid::new_v4().simple());
    harness.sign_in_through_kakao(&second, None).await;
    assert_eq!(harness.mock.jwks_fetches(), 1, "캐시가 있으면 다시 받지 않는다");

    // 카카오가 키를 회전한다.
    harness.mock.rotate_to(KID_2);
    let (state, nonce) = harness.authorize().await;
    let third = format!("kakao-{}", Uuid::new_v4().simple());
    harness
        .mock
        .will_sign_with(KID_2, kakao_claims(&third, &nonce, None));

    harness
        .callback(&format!("code=c&state={state}"))
        .await
        .expect_ok("회전한 키로 서명된 토큰도 받아들인다");
    assert_eq!(harness.mock.jwks_fetches(), 2, "회전은 갱신 한 번으로 끝난다");

    // 아무도 발행하지 않은 `kid` 는 갱신 한 번 뒤 거절된다 — 두 번 받지 않는다.
    let (state, nonce) = harness.authorize().await;
    harness
        .mock
        .will_sign_with(KID_1, kakao_claims("kakao-forged", &nonce, None));
    let refused = harness.callback(&format!("code=c&state={state}")).await;
    refused.expect_error(
        StatusCode::UNAUTHORIZED,
        "KAKAO_SECURITY_CHECK_FAILED",
        "발행되지 않은 kid",
    );
    assert_eq!(
        harness.mock.jwks_fetches(),
        3,
        "거절 한 번에 갱신도 한 번뿐이어야 한다"
    );
}

#[tokio::test]
async fn an_exchange_code_is_single_use() {
    // §9.2-3: 일회용. 두 번째 제출은 §6.1 의 보안 검증 실패다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;

    harness.exchange(&code).await.expect_ok("first exchange");

    let replayed = harness.exchange(&code).await;
    replayed.expect_error(
        StatusCode::UNAUTHORIZED,
        "KAKAO_SECURITY_CHECK_FAILED",
        "재사용된 교환 코드",
    );
}

// ===========================================================================
// 4. 카카오 로그인 — 세 가지 오류 (§6.1)
// ===========================================================================

#[tokio::test]
async fn cancelling_creates_no_account_and_is_not_reported_as_a_failure() {
    // AUTH-002: 사용자가 동의를 취소하면 아무 계정도 생성하지 않는다.
    let harness = kakao_or_skip!();

    let (state, _) = harness.authorize().await;
    let cancelled = harness
        .callback(&format!(
            "error=access_denied&error_description=user%20denied&state={state}"
        ))
        .await;
    cancelled.expect_error(
        StatusCode::BAD_REQUEST,
        "KAKAO_LOGIN_CANCELLED",
        "취소는 보안 실패가 아니다",
    );

    // 계정이 만들어지지 않았다는 것은 총 회원 수로 확인할 수 없다 — 같은 DB 를 쓰는 다른
    // 테스트가 동시에 회원을 만든다. 대신 흐름이 어디서 멈췄는지를 본다: 취소는 카카오
    // 토큰 요청조차 보내지 않고 끝나므로, 신원 자체가 생기지 않고 따라서 만들 계정도 없다.
    assert_eq!(
        harness.mock.token_calls(),
        0,
        "취소는 인가 코드를 교환하지 않는다"
    );
}

#[tokio::test]
async fn a_temporary_kakao_outage_is_told_apart_from_a_security_failure() {
    // §6.1 은 카카오 오류를 취소·일시 장애·보안 검증 실패 셋으로만 구분한다. 셋을
    // 뭉뚱그리면 사용자가 "다시 시도"와 "처음부터"를 고를 수 없게 된다.
    let harness = kakao_or_skip!();

    let (state, _) = harness.authorize().await;
    harness.mock.will_fail(500, r#"{"error":"server_error"}"#);
    let outage = harness.callback(&format!("code=c&state={state}")).await;
    outage.expect_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "DEPENDENCY_UNAVAILABLE",
        "일시 장애는 재시도할 수 있어야 한다",
    );

    let (state, _) = harness.authorize().await;
    harness
        .mock
        .will_fail(400, r#"{"error":"invalid_grant","error_description":"spent"}"#);
    let spent = harness.callback(&format!("code=c&state={state}")).await;
    spent.expect_error(
        StatusCode::UNAUTHORIZED,
        "KAKAO_SECURITY_CHECK_FAILED",
        "이미 쓴 인가 코드는 보안 검증 실패",
    );
}

#[tokio::test]
async fn callback_failures_are_rate_limited_by_ip_and_state_prefix() {
    // §16.4: 카카오 callback 실패 20회/10분, IP+state prefix. 성공은 세지 않는다 —
    // 붐비는 월요일 아침은 공격이 아니다 (SEC-003).
    let harness = kakao_or_skip!(json!({
        "rate_limit_kakao_callback_failure_per_10min": 2,
    }));

    for attempt in 1..=2 {
        let refused = harness.callback("code=c&state=deadbeef-attempt").await;
        assert_eq!(
            refused.error_code(),
            "KAKAO_SECURITY_CHECK_FAILED",
            "attempt {attempt}"
        );
    }

    let throttled = harness.callback("code=c&state=deadbeef-attempt").await;
    throttled.expect_error(
        StatusCode::TOO_MANY_REQUESTS,
        "RATE_LIMITED",
        "세 번째 실패는 막힌다",
    );
}

// ===========================================================================
// 5. 설정이 없을 때
// ===========================================================================

#[tokio::test]
async fn an_unregistered_kakao_app_refuses_clearly_instead_of_404ing() {
    let harness = harness_or_skip!();

    let response = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/auth/kakao/authorize",
        "anonymous",
        None,
    )
    .await;
    response.expect_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "SERVICE_UNAVAILABLE",
        "앱 키가 없으면 경로가 사라지는 게 아니라 이유를 말한다",
    );
    assert!(
        !response.message().contains("CLIENT_ID"),
        "사용자에게 설정 이름을 노출하지 않는다: {}",
        response.message()
    );
}

#[tokio::test]
async fn sign_in_fails_clearly_when_no_firebase_service_account_is_configured() {
    // 실제 Firebase 서비스 계정 키는 아직 없다. 없을 때 조용히 성공하거나 500 을 내는
    // 대신, 여기까지는 정상으로 진행하고 마지막 단계에서 명확히 거절해야 한다.
    let harness = kakao_or_skip!(json!({
        "firebase_service_account_email": Value::Null,
        "firebase_service_account_private_key": Value::Null,
    }));

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;

    let refused = harness.exchange(&code).await;
    refused.expect_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "SERVICE_UNAVAILABLE",
        "서비스 계정이 없으면 Custom Token 을 만들 수 없다",
    );
}

// ===========================================================================
// 6. AUTH-003 동일인 계정 연결
// ===========================================================================

#[tokio::test]
async fn linking_kakao_to_an_existing_account_keeps_that_accounts_canonical_firebase_uid() {
    // §9.2-7: 기존 계정에 카카오를 연결한 경우를 포함하여 **항상** 그 회원의 canonical
    // Firebase UID 로 토큰을 만든다. 연결 뒤 카카오로 들어와도 원래 UID 로 로그인되어야
    // 지갑·원장·쿠폰이 한 사람 것으로 남는다.
    let harness = kakao_or_skip!();

    let member = common::consumer(&harness.harness.app, "auth-link").await;
    let canonical_uid = member.uid.clone();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;

    let linked = send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        Some(json!({ "exchange_code": code })),
    )
    .await;
    let link = linked.expect_ok("link kakao");
    assert_eq!(link["provider"], "KAKAO");
    assert_eq!(link["status"], "ACTIVE");

    // 이제 카카오로 들어온다. 새 회원이 아니라 그 회원이어야 한다.
    let second = harness.sign_in_through_kakao(&subject, None).await;
    let signed_in = harness.exchange(&second).await;
    let data = signed_in.expect_ok("exchange after linking");

    assert_eq!(data["created"], false, "연결된 계정은 새로 만들지 않는다");
    assert_eq!(
        Uuid::parse_str(data["user_id"].as_str().expect("user_id")).expect("uuid"),
        member.user_id
    );
    assert_eq!(
        decode_custom_token(data["custom_token"].as_str().expect("token")).uid,
        canonical_uid,
        "카카오로 들어와도 원래 Firebase UID 로 로그인된다"
    );

    // 두 수단이 모두 보인다 (§6.1 /account/security).
    let links = send(
        &harness.harness.app,
        "GET",
        "/api/coupon/v1/me/auth-links",
        &member.uid,
        None,
    )
    .await;
    let providers: Vec<&str> = links
        .expect_ok("list links")
        .as_array()
        .expect("array")
        .iter()
        .map(|link| link["provider"].as_str().expect("provider"))
        .collect();
    assert!(providers.contains(&"FIREBASE_PASSWORD"), "{providers:?}");
    assert!(providers.contains(&"KAKAO"), "{providers:?}");
}

#[tokio::test]
async fn a_kakao_account_linked_to_someone_else_is_sent_to_support_not_transferred() {
    // AUTH-003: 서로 다른 내부 회원에 이미 연결된 인증수단은 자동 이전하지 않는다.
    let harness = kakao_or_skip!();

    let first = common::consumer(&harness.harness.app, "auth-owner").await;
    let second = common::consumer(&harness.harness.app, "auth-claimer").await;

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;
    send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &first.uid,
        Some(json!({ "exchange_code": code })),
    )
    .await
    .expect_ok("first link");

    let stolen = harness.sign_in_through_kakao(&subject, None).await;
    let refused = send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &second.uid,
        Some(json!({ "exchange_code": stolen })),
    )
    .await;

    refused.expect_error(
        StatusCode::CONFLICT,
        "AUTH_LINK_ALREADY_CLAIMED",
        "이미 다른 회원의 것",
    );
    assert!(
        refused.message().contains("고객센터"),
        "본인 확인 경로를 안내해야 한다: {}",
        refused.message()
    );
}

#[tokio::test]
async fn a_member_may_hold_only_one_kakao_link() {
    // §11.2 는 이 경로를 단수로 쓴다(`/me/auth-links/kakao`). 두 개가 붙어 있으면
    // `DELETE` 가 둘 중 어느 것을 끊는지 정해지지 않는다.
    let harness = kakao_or_skip!();

    let member = common::consumer(&harness.harness.app, "auth-onelink").await;

    let first_subject = format!("kakao-{}", Uuid::new_v4().simple());
    let first = harness.sign_in_through_kakao(&first_subject, None).await;
    send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        Some(json!({ "exchange_code": first })),
    )
    .await
    .expect_ok("first link");

    // 같은 코드를 다시 내면 멱등이어야 하지만, *다른* 카카오 계정은 거절된다.
    let again = harness.sign_in_through_kakao(&first_subject, None).await;
    send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        Some(json!({ "exchange_code": again })),
    )
    .await
    .expect_ok("relinking the same account is a no-op");

    let second_subject = format!("kakao-{}", Uuid::new_v4().simple());
    let second = harness.sign_in_through_kakao(&second_subject, None).await;
    let refused = send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        Some(json!({ "exchange_code": second })),
    )
    .await;
    refused.expect_error(
        StatusCode::CONFLICT,
        "CONFLICT",
        "두 번째 카카오 계정은 붙지 않는다",
    );

    let active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.auth_identities
         WHERE user_id = $1 AND provider = 'KAKAO' AND status = 'ACTIVE'",
    )
    .bind(member.user_id)
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert_eq!(active, 1);
}

#[tokio::test]
async fn the_same_email_does_not_merge_two_accounts_by_itself() {
    // §9.2: 이메일은 매핑 힌트일 뿐 계정 자동 병합 키가 아니다.
    let harness = kakao_or_skip!();

    // 개발용 bypass 는 `{uid}@dev.invalid` 를 이메일로 준다. 카카오가 정확히 같은 주소를
    // 들고 와도 두 계정은 별개로 남아야 한다.
    let member = common::consumer(&harness.harness.app, "auth-sameemail").await;
    let shared_email = format!("{}@dev.invalid", member.uid);

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness
        .sign_in_through_kakao(&subject, Some(&shared_email))
        .await;
    let signed_in = harness.exchange(&code).await;
    let data = signed_in.expect_ok("exchange");

    assert_eq!(data["created"], true, "같은 이메일이어도 새 회원이다");
    assert_ne!(
        Uuid::parse_str(data["user_id"].as_str().expect("user_id")).expect("uuid"),
        member.user_id,
        "이메일이 같다는 이유로 병합하면 안 된다"
    );
}

#[tokio::test]
async fn the_last_login_method_cannot_be_unlinked() {
    // AUTH-002: 다른 로그인 수단이 없으면 로그인을 막는다. 회원이 스스로 그 상태를
    // 만들도록 두는 것은 다른 이야기다 — 끊는 순간 다시 들어올 방법이 없어진다.
    let harness = kakao_or_skip!();

    let member = common::consumer(&harness.harness.app, "auth-unlink").await;

    let absent = send(
        &harness.harness.app,
        "DELETE",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        None,
    )
    .await;
    absent.expect_error(
        StatusCode::NOT_FOUND,
        "AUTH_LINK_NOT_FOUND",
        "연결한 적 없는 수단",
    );

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;
    send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        Some(json!({ "exchange_code": code })),
    )
    .await
    .expect_ok("link");

    // 비밀번호 수단이 남아 있으므로 카카오는 끊을 수 있다.
    send(
        &harness.harness.app,
        "DELETE",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        None,
    )
    .await
    .expect_ok("unlink");

    // 이제 카카오만 남긴 뒤 마지막 하나를 끊으려 하면 막힌다.
    let kakao_only = harness.sign_in_through_kakao(&subject, None).await;
    send(
        &harness.harness.app,
        "POST",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        Some(json!({ "exchange_code": kakao_only })),
    )
    .await
    .expect_ok("relink");

    sqlx::query(
        "UPDATE coupon.auth_identities
         SET status = 'UNLINKED', unlinked_at = clock_timestamp()
         WHERE user_id = $1 AND provider = 'FIREBASE_PASSWORD'",
    )
    .bind(member.user_id)
    .execute(&harness.harness.pool)
    .await
    .expect("leave only kakao");

    let last = send(
        &harness.harness.app,
        "DELETE",
        "/api/coupon/v1/me/auth-links/kakao",
        &member.uid,
        None,
    )
    .await;
    last.expect_error(
        StatusCode::CONFLICT,
        "LAST_AUTH_LINK_CANNOT_BE_REMOVED",
        "마지막 수단",
    );
}

// ===========================================================================
// 7. 연결 해제 웹훅
// ===========================================================================

#[tokio::test]
async fn an_unsigned_or_tampered_unlink_webhook_changes_nothing() {
    // 서명이 없으면 낯선 사람이 아무 회원의 로그인이나 끊을 수 있다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;
    harness.exchange(&code).await.expect_ok("sign in");

    let timestamp = Utc::now().to_rfc3339();
    let body = json!({ "app_id": 1234, "user_id": subject, "referrer_type": "UNLINK_FROM_APPS" })
        .to_string();

    for (label, signature) in [
        ("빈 서명", String::new()),
        ("남의 서명", sign_webhook(&timestamp, "{\"user_id\":\"0\"}")),
        ("hex 가 아닌 값", "not-hex".to_owned()),
    ] {
        let refused = post_unlink_webhook(&harness.harness, &timestamp, &body, &signature).await;
        refused.expect_error(
            StatusCode::UNAUTHORIZED,
            "WEBHOOK_SIGNATURE_INVALID",
            label,
        );
    }

    // 서명이 맞아도 시각이 창 밖이면 재생이다 (§19.3 replay 방지).
    let stale = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    let stale_but_signed = post_unlink_webhook(
        &harness.harness,
        &stale,
        &body,
        &sign_webhook(&stale, &body),
    )
    .await;
    stale_but_signed.expect_error(
        StatusCode::UNAUTHORIZED,
        "WEBHOOK_SIGNATURE_INVALID",
        "창 밖에서 재생된 요청",
    );

    let still_active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.auth_identities
         WHERE provider = 'KAKAO' AND provider_subject = $1 AND status = 'ACTIVE'",
    )
    .bind(&subject)
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert_eq!(still_active, 1, "거절된 웹훅은 아무것도 바꾸지 않는다");
}

#[tokio::test]
async fn a_replayed_unlink_webhook_is_idempotent() {
    // §9.2 마지막: 서명을 검증하고 멱등 처리한다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;
    let signed_in = harness.exchange(&code).await;
    let user_id = Uuid::parse_str(signed_in.expect_ok("sign in")["user_id"].as_str().unwrap())
        .expect("uuid");

    let timestamp = Utc::now().to_rfc3339();
    let body = json!({ "app_id": 1234, "user_id": subject, "referrer_type": "UNLINK_FROM_APPS" })
        .to_string();
    let signature = sign_webhook(&timestamp, &body);

    let first = post_unlink_webhook(&harness.harness, &timestamp, &body, &signature).await;
    assert_eq!(first.expect_ok("first delivery")["outcome"], "APPLIED");

    // 같은 요청을 그대로 다시 보낸다 — 서명도 시각도 같다.
    let replay = post_unlink_webhook(&harness.harness, &timestamp, &body, &signature).await;
    assert_eq!(
        replay.expect_ok("replay")["outcome"],
        "ALREADY_PROCESSED",
        "같은 이벤트는 한 번만 처리된다"
    );

    let active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.auth_identities
         WHERE user_id = $1 AND provider = 'KAKAO' AND status = 'ACTIVE'",
    )
    .bind(user_id)
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert_eq!(active, 0, "연결이 끊긴 상태로 남는다");

    // 모르는 회원에 대한 이벤트는 조용히 기록만 된다.
    let unknown_body = json!({ "app_id": 1234, "user_id": "kakao-nobody-here" }).to_string();
    let unknown_ts = Utc::now().to_rfc3339();
    let unknown = post_unlink_webhook(
        &harness.harness,
        &unknown_ts,
        &unknown_body,
        &sign_webhook(&unknown_ts, &unknown_body),
    )
    .await;
    assert_eq!(
        unknown.expect_ok("unknown identity")["outcome"],
        "UNKNOWN_IDENTITY"
    );
}

#[tokio::test]
async fn signing_in_again_after_an_unlink_restores_the_link() {
    // AUTH-002: 다른 로그인 수단이 없으면 재인증 전까지 로그인을 막는다. 카카오로 다시
    // 로그인하는 것이 바로 그 재인증이다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;
    let first = harness.exchange(&code).await;
    let user_id =
        Uuid::parse_str(first.expect_ok("sign in")["user_id"].as_str().unwrap()).expect("uuid");

    let timestamp = Utc::now().to_rfc3339();
    let body = json!({ "user_id": subject }).to_string();
    post_unlink_webhook(
        &harness.harness,
        &timestamp,
        &body,
        &sign_webhook(&timestamp, &body),
    )
    .await
    .expect_ok("unlink");

    let again = harness.sign_in_through_kakao(&subject, None).await;
    let restored = harness.exchange(&again).await;
    let data = restored.expect_ok("sign in again");

    assert_eq!(data["created"], false, "새 회원을 만들지 않는다");
    assert_eq!(
        Uuid::parse_str(data["user_id"].as_str().unwrap()).expect("uuid"),
        user_id
    );

    let active = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.auth_identities
         WHERE user_id = $1 AND provider = 'KAKAO' AND status = 'ACTIVE'",
    )
    .bind(user_id)
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert_eq!(active, 1, "재로그인으로 연결이 살아난다");
}

// ===========================================================================
// 8. 살림살이
// ===========================================================================

#[tokio::test]
async fn expired_login_sessions_and_exchange_codes_can_be_swept() {
    // 만료 자체는 질의의 `WHERE` 절이 강제한다. 그래도 authorize 한 번에 한 행씩 쌓이는
    // 테이블을 영원히 두고 볼 수는 없어서, 쓸어 담는 쪽도 실제로 도는지 확인해 둔다.
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    harness.sign_in_through_kakao(&subject, None).await;

    // 아직 유효한 행은 남는다.
    let live_before = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.oauth_login_sessions WHERE expires_at > clock_timestamp()",
    )
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert!(live_before >= 1);

    let swept = coupon_api_server::auth::kakao::sessions::purge_expired(
        &harness.harness.pool,
        Utc::now(),
    )
    .await
    .expect("sweep");

    let live_after = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.oauth_login_sessions WHERE expires_at > clock_timestamp()",
    )
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert!(
        live_after >= 1,
        "아직 살아 있는 세션까지 지우면 진행 중인 로그인이 끊긴다 (지운 행: {swept})"
    );

    let expired = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM coupon.oauth_login_sessions WHERE expires_at < clock_timestamp()",
    )
    .fetch_one(&harness.harness.pool)
    .await
    .expect("count");
    assert_eq!(expired, 0, "만료된 행은 남지 않는다");
}

// ===========================================================================
// 9. 계정 상태
// ===========================================================================

#[tokio::test]
async fn a_suspended_member_cannot_sign_in_through_kakao_either() {
    // §9.3: 사용자 상태는 매 요청 DB 에서 확인한다. 카카오 경로라고 예외가 아니다
    // (SEC-003: 카카오 계정이라고 해서 무조건 신뢰하지 않는다).
    let harness = kakao_or_skip!();

    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;
    let first = harness.exchange(&code).await;
    let user_id =
        Uuid::parse_str(first.expect_ok("sign in")["user_id"].as_str().unwrap()).expect("uuid");

    set_status(&harness.harness.pool, user_id, "SUSPENDED").await;

    let again = harness.sign_in_through_kakao(&subject, None).await;
    let refused = harness.exchange(&again).await;
    refused.expect_error(
        StatusCode::FORBIDDEN,
        "ACCOUNT_SUSPENDED",
        "정지된 회원은 카카오로도 들어올 수 없다",
    );
}

#[tokio::test]
async fn linking_kakao_requires_a_recent_sign_in() {
    // AUTH-003: 로그인된 회원이 비밀번호 재확인 또는 최근 로그인을 수행한 후 연결한다.
    let harness = kakao_or_skip!();

    let member = common::consumer(&harness.harness.app, "auth-stale").await;
    let subject = format!("kakao-{}", Uuid::new_v4().simple());
    let code = harness.sign_in_through_kakao(&subject, None).await;

    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let request = Request::builder()
        .method("POST")
        .uri("/api/coupon/v1/me/auth-links/kakao")
        .header("x-dev-firebase-uid", &member.uid)
        // 한 시간 전에 로그인한 세션.
        .header("x-dev-auth-age-secs", "3600")
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::new_v4().to_string())
        .body(Body::from(json!({ "exchange_code": code }).to_string()))
        .expect("request");

    let response = harness
        .harness
        .app
        .clone()
        .oneshot(request)
        .await
        .expect("router responds");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "오래된 로그인으로는 연결할 수 없다"
    );
}
