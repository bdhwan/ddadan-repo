//! §20 MVP 인수 시나리오와 §19.3 계약 테스트를 **실제로 기동한 서버**에 대고 돈다.
//!
//! Phase 1–5 의 통합 테스트는 `Router` 를 프로세스 안에서 직접 호출한다. 그건 도메인
//! 로직을 확인하기에 옳은 방법이지만, 그 방식으로는 절대 지나가지 않는 층이 있다.
//!
//! * `main.rs` 의 부팅 순서와 설정 검증 — 잘못된 설정은 요청을 받기 전에 죽어야 한다
//! * 진짜 Firebase ID Token 검증 (여기서는 Auth emulator 가 발급한 것, §20.1 `local`)
//! * TCP·HTTP 파싱, CORS, Origin 검사, 미들웨어 순서
//! * `coupon-worker` 가 **별개 프로세스로** 잡을 집어가는 경로 (§14.2)
//!
//! 그래서 이 파일은 `coupon-api` 와 `coupon-worker` 바이너리를 실제로 띄우고 `reqwest` 로
//! 붙는다. 느리다. 그 대신, 여기가 통과하면 "서버를 켜면 동작한다"가 참이 된다.
//!
//! ```sh
//! ./apps/coupon-api-server/scripts/auth-emulator.sh up
//! ./scripts/coupon/test.sh
//! ```
//!
//! emulator 나 `COUPON_TEST_DATABASE_URL` 이 없으면 조용히 통과하지 않고 눈에 보이게
//! 건너뛴다 — 다른 스위트와 같은 규약이다.

#![allow(clippy::items_after_test_module)]

use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// emulator 가 쓰는 프로젝트. 서버의 `aud`/`iss` 기대값과 같아야 한다.
const PROJECT_ID: &str = "ddadan-dev";
/// 브라우저 앱이 쓸 오리진 하나. Origin 검사(§16.3)를 실제로 태우기 위한 값이다.
const ALLOWED_ORIGIN: &str = "https://192.168.150.185:4310";

// ---------------------------------------------------------------------------
// 기동
// ---------------------------------------------------------------------------

/// 띄워 둔 스택 하나. 테스트마다 서버를 새로 띄우면 테스트 DB 연결이 금세 바닥난다.
struct Stack {
    api: String,
    http: reqwest::Client,
    emulator: EmulatorClient,
}

/// 한 번만 띄운 스택의 주소. `reqwest::Client` 는 여기 담지 않는다 — 커넥션 풀이 그것을
/// 만든 tokio 런타임에 묶여 있어서, 처음 만든 테스트의 런타임이 끝나면 뒤따르는 테스트가
/// "runtime dropped the dispatch task" 로 죽는다. 주소만 나눠 쓰고 클라이언트는 각자 만든다.
static ENDPOINT: OnceCell<Option<(String, String)>> = OnceCell::const_new();

async fn stack() -> Option<Arc<Stack>> {
    let (api, emulator_host) = ENDPOINT
        .get_or_init(|| async {
            let database_url = std::env::var("COUPON_TEST_DATABASE_URL").ok()?;
            let emulator_host = emulator_host();

            if !emulator_reachable(&emulator_host).await {
                eprintln!(
                    "건너뜀: Firebase Auth emulator({emulator_host})가 응답하지 않습니다. \
                     ./apps/coupon-api-server/scripts/auth-emulator.sh up"
                );
                return None;
            }

            let port = free_port();
            // 이 두 프로세스는 스위트 전체가 쓴다. 지킴이 스레드가 붙들고 있어야 한다 —
            // 이유는 `keep_alive` 에 적어 두었다.
            spawn_kept(
                env!("CARGO_BIN_EXE_coupon-api"),
                &[
                    ("COUPON_BIND_ADDR", &format!("127.0.0.1:{port}")),
                    ("COUPON_DATABASE_URL", &database_url),
                    ("COUPON_FIREBASE_AUTH_EMULATOR_HOST", &emulator_host),
                    ("COUPON_ALLOWED_ORIGINS", ALLOWED_ORIGIN),
                    // §16.4 의 한도는 이 스위트의 주제가 아니다. 동시 claim 테스트가
                    // 자기 자신의 100번째 요청 때문에 거절당하면 아무것도 증명하지 못한다.
                    ("COUPON_RATE_LIMIT_QR_ISSUE_PER_MIN", "100000"),
                    ("COUPON_RATE_LIMIT_STAMP_APPROVAL_PER_MIN", "100000"),
                    ("COUPON_RATE_LIMIT_CAMPAIGN_CLAIM_PER_MIN", "100000"),
                    ("COUPON_RATE_LIMIT_QR_RESOLVE_FAILURE_PER_MIN", "100000"),
                ],
            );

            // §14.2 의 "워커가 처리한다"를 별개 프로세스로 실제 확인하기 위한 것.
            spawn_kept(
                env!("CARGO_BIN_EXE_coupon-worker"),
                &[
                    ("COUPON_DATABASE_URL", &database_url),
                    ("COUPON_FIREBASE_AUTH_EMULATOR_HOST", &emulator_host),
                ],
            );

            let api = format!("http://127.0.0.1:{port}/api/coupon/v1");
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client");

            wait_until_ready(&http, &api).await;

            Some((api, emulator_host))
        })
        .await
        .clone()?;

    Some(Arc::new(Stack {
        api,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client"),
        emulator: EmulatorClient::new(&emulator_host),
    }))
}

macro_rules! stack_or_skip {
    () => {
        match stack().await {
            Some(stack) => stack,
            None => {
                eprintln!("건너뜀: 실제 스택을 띄울 수 없습니다");
                return;
            }
        }
    };
}

fn emulator_host() -> String {
    std::env::var("COUPON_FIREBASE_AUTH_EMULATOR_HOST")
        .unwrap_or_else(|_| "127.0.0.1:9099".to_owned())
}

async fn emulator_reachable(host: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    client.get(format!("http://{host}/")).send().await.is_ok()
}

/// 커널에게 빈 포트를 하나 물어보고 곧바로 놓아준다.
///
/// 놓아준 뒤 서버가 잡기까지 사이에 이론적인 틈이 있지만, 고정 포트를 쓰면 같은 기계에서
/// 다른 작업이 돌 때 **반드시** 부딪힌다. 드문 실패가 확실한 실패보다 낫다.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// 부모가 죽으면 같이 죽는 자식 프로세스.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 스위트가 끝날 때까지 살아 있어야 하는 프로세스를 **지킴이 스레드가** 낳게 한다.
///
/// `PR_SET_PDEATHSIG` 는 부모 *프로세스* 가 아니라 부모 *스레드* 가 죽을 때 발동한다.
/// 테스트는 스레드마다 돌기 때문에, 스택을 처음 만든 테스트가 끝나는 순간 그 스레드가
/// 사라지고 서버가 함께 죽는다 — 뒤따르던 테스트들이 전부 "connection refused" 로 무너진다.
/// 그래서 공용 프로세스는 프로세스가 끝날 때까지 잠들어 있는 스레드가 낳고, 그 스레드가
/// 계속 붙들고 있는다.
fn spawn_kept(binary: &'static str, extra_env: &[(&str, &str)]) {
    type Job = Box<dyn FnOnce() -> Process + Send>;
    static KEEPER: std::sync::OnceLock<std::sync::mpsc::Sender<(Job, std::sync::mpsc::Sender<()>)>> =
        std::sync::OnceLock::new();

    let keeper = KEEPER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<(Job, std::sync::mpsc::Sender<()>)>();
        std::thread::spawn(move || {
            // `Process` 는 Drop 에서 죽인다. 이 벡터가 스레드와 함께 프로세스 종료까지
            // 살아 있어야 서버와 워커가 스위트 내내 떠 있다.
            let mut held = Vec::new();
            while let Ok((job, done)) = receiver.recv() {
                held.push(job());
                let _ = done.send(());
            }
            drop(held);
        });
        sender
    });

    let env: Vec<(String, String)> = extra_env
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    let (done, wait) = std::sync::mpsc::channel();
    keeper
        .send((
            Box::new(move || {
                let borrowed: Vec<(&str, &str)> = env
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect();
                spawn(binary, &borrowed)
            }),
            done,
        ))
        .expect("지킴이 스레드가 살아 있다");
    wait.recv().expect("프로세스가 떴다");
}

fn spawn(binary: &str, extra_env: &[(&str, &str)]) -> Process {
    let mut command = Command::new(binary);

    // 껍데기 환경에 남아 있는 COUPON_* 가 새어 들어오면 테스트가 무엇을 검증하는지
    // 알 수 없게 된다. 필요한 것만 명시적으로 넣는다.
    command.env_clear();
    if let Ok(path) = std::env::var("PATH") {
        command.env("PATH", path);
    }
    command
        .env("COUPON_ENV", "test")
        .env("COUPON_FIREBASE_PROJECT_ID", PROJECT_ID)
        .env("COUPON_DATABASE_MAX_CONNECTIONS", "8")
        .env("COUPON_LOG_FORMAT", "pretty")
        .env("COUPON_LOG_FILTER", "warn");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    // stderr 는 그대로 흘려보낸다. `COUPON_LOG_FILTER=warn` 이라 평소에는 조용하고,
    // 서버나 워커가 부팅에 실패하면 그 이유가 테스트 출력에 바로 보인다. 파이프로
    // 받아 두고 아무도 읽지 않으면, 로그가 조금만 많아져도 자식이 버퍼에서 멈춘다.
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit());

    // 테스트 바이너리가 어떤 이유로든 죽었을 때 서버와 워커가 살아남으면, 다음 실행이
    // 낡은 프로세스와 붙어 진단 불가능한 실패를 낸다. 커널에게 같이 죽으라고 시켜 둔다.
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            // prctl(PR_SET_PDEATHSIG, SIGKILL)
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    Process(command.spawn().unwrap_or_else(|error| {
        panic!("{binary} 를 띄우지 못했습니다: {error}");
    }))
}

async fn wait_until_ready(http: &reqwest::Client, api: &str) {
    for _ in 0..120 {
        if let Ok(response) = http.get(format!("{api}/health/ready")).send().await
            && response.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("{api} 가 30초 안에 준비되지 않았습니다");
}

// ---------------------------------------------------------------------------
// emulator
// ---------------------------------------------------------------------------

/// Auth emulator 의 Identity Toolkit 표면. 앱이 Firebase SDK 로 하는 것과 같은 호출이다.
struct EmulatorClient {
    base: String,
    http: reqwest::Client,
}

impl EmulatorClient {
    fn origin(&self) -> &str {
        self.base
            .trim_end_matches("/identitytoolkit.googleapis.com/v1")
    }

    fn new(host: &str) -> Self {
        Self {
            base: format!("http://{host}/identitytoolkit.googleapis.com/v1"),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("client"),
        }
    }

    /// 이메일/비밀번호 가입 후 이메일 인증까지 마친 사람. `(uid, id_token, refresh_token)`.
    ///
    /// 인증을 마친 상태로 만드는 데는 이유가 있다. 나머지 시나리오는 실제 사용자가 밟는
    /// 순서, 즉 인증을 마치고 로그인한 상태에서 시작해야 한다. 인증 전에 가입하면 계정은
    /// `PENDING_VERIFICATION` 이고, 그 상태에서는 캠페인 대상에 들어오지 않는다 —
    /// 인증 후 bootstrap 이 `ACTIVE` 로 승격시키는 흐름은 아래
    /// `verifying_an_email_promotes_the_account_that_was_created_before_it` 가 따로 본다.
    async fn sign_up(&self, label: &str) -> (String, String, String) {
        let (uid, email, ..) = self.sign_up_unverified(label).await;
        self.verify_email(&uid).await;

        // 인증 뒤 다시 로그인해야 토큰이 `email_verified: true` 로 온다.
        let body = self.sign_in(&email).await;
        (
            uid,
            body["idToken"].as_str().expect("idToken").to_owned(),
            body["refreshToken"]
                .as_str()
                .expect("refreshToken")
                .to_owned(),
        )
    }

    /// 가입만. `(uid, email, id_token)`.
    async fn sign_up_unverified(&self, label: &str) -> (String, String, String) {
        let email = format!("{label}-{}@ddadan.test", Uuid::new_v4().simple());
        let body: Value = self
            .http
            .post(format!("{}/accounts:signUp?key=fake-api-key", self.base))
            .json(&json!({
                "email": email,
                "password": "AcceptancePass!234",
                "returnSecureToken": true,
            }))
            .send()
            .await
            .expect("emulator 가입")
            .json()
            .await
            .expect("가입 응답");

        (
            body["localId"].as_str().expect("localId").to_owned(),
            email,
            body["idToken"].as_str().expect("idToken").to_owned(),
        )
    }

    /// 사용자가 인증 메일의 링크를 눌렀을 때와 같은 상태로 만든다.
    async fn verify_email(&self, uid: &str) {
        let response = self
            .http
            .post(format!("{}/accounts:update", self.base))
            .header("Authorization", "Bearer owner")
            .json(&json!({ "localId": uid, "emailVerified": true }))
            .send()
            .await
            .expect("이메일 인증 표시");
        assert!(response.status().is_success(), "이메일 인증에 실패했습니다");
    }

    async fn sign_in(&self, email: &str) -> Value {
        self.http
            .post(format!(
                "{}/accounts:signInWithPassword?key=fake-api-key",
                self.base
            ))
            .json(&json!({
                "email": email,
                "password": "AcceptancePass!234",
                "returnSecureToken": true,
            }))
            .send()
            .await
            .expect("emulator 로그인")
            .json()
            .await
            .expect("로그인 응답")
    }

    /// 계정을 잠근다. Firebase 의 세션 폐기와 같은 효과다 — 기존 ID Token 은 만료까지
    /// 살아 있지만 새 토큰을 받을 수 없다.
    async fn disable(&self, uid: &str) {
        let response = self
            .http
            .post(format!("{}/accounts:update", self.base))
            .header("Authorization", "Bearer owner")
            .json(&json!({ "localId": uid, "disableUser": true }))
            .send()
            .await
            .expect("계정 잠금");
        assert!(response.status().is_success(), "계정을 잠그지 못했습니다");
    }

    /// refresh token 으로 새 ID Token 을 받는다. 폐기된 계정이면 실패한다.
    ///
    /// 갱신은 Identity Toolkit 이 아니라 securetoken 표면에 있다 — 실제 Firebase 와 같다.
    async fn refresh(&self, refresh_token: &str) -> reqwest::StatusCode {
        self.http
            .post(format!(
                "{}/securetoken.googleapis.com/v1/token?key=fake-api-key",
                self.origin()
            ))
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .expect("토큰 갱신 시도")
            .status()
    }
}

/// emulator 가 발급한 토큰의 클레임을 바꿔 다시 조립한다.
///
/// emulator 토큰은 서명이 없으므로(`alg: none`) 이렇게 만든 것도 **emulator 가 낼 수 있는
/// 토큰과 완전히 같은 모양**이다. 만료된 세션이나 다른 프로젝트의 토큰을 한 시간 기다리거나
/// 두 번째 프로젝트를 띄우지 않고 시험할 수 있는 이유다.
fn restamp(token: &str, mutate: impl FnOnce(&mut Value)) -> String {
    let mut parts = token.split('.');
    let header = parts.next().expect("header");
    let payload = parts.next().expect("payload");

    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .expect("payload is base64url");
    let mut claims: Value = serde_json::from_slice(&bytes).expect("payload is JSON");
    mutate(&mut claims);

    format!(
        "{header}.{}.",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

// ---------------------------------------------------------------------------
// 요청
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Reply {
    status: reqwest::StatusCode,
    json: Value,
}

impl Reply {
    fn data(&self) -> &Value {
        &self.json["data"]
    }

    fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    #[track_caller]
    fn expect_ok(&self, what: &str) -> &Value {
        assert!(
            self.status.is_success(),
            "{what} 실패 ({}): {}",
            self.status,
            self.json
        );
        self.data()
    }

    #[track_caller]
    fn id(&self, what: &str) -> Uuid {
        Uuid::parse_str(self.expect_ok(what)["id"].as_str().expect("id")).expect("uuid")
    }
}

impl Stack {
    async fn send(&self, method: &str, path: &str, token: &str, body: Option<Value>) -> Reply {
        self.send_full(method, path, Some(token), body, None, None)
            .await
    }

    /// 헤더 하나하나를 정하고 싶은 경우. Origin 검사와 멱등키 재사용이 여기를 쓴다.
    async fn send_full(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        idempotency_key: Option<Uuid>,
        origin: Option<&str>,
    ) -> Reply {
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("method");
        let mutation = method != reqwest::Method::GET;

        let mut request = self
            .http
            .request(method, format!("{}{path}", self.api))
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        if mutation {
            request = request.header(
                "idempotency-key",
                idempotency_key.unwrap_or_else(Uuid::new_v4).to_string(),
            );
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }

        let response = request.send().await.expect("서버가 응답합니다");
        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        Reply {
            status,
            json: serde_json::from_str(&text).unwrap_or(Value::String(text)),
        }
    }

}

// ---------------------------------------------------------------------------
// 픽스처
// ---------------------------------------------------------------------------

/// emulator 로 가입하고 내부 계정까지 만든 사람.
struct Person {
    /// Firebase UID. 토큰이 말하는 그 값이다.
    #[allow(dead_code)]
    uid: String,
    token: String,
    user_id: Uuid,
}

async fn person(stack: &Stack, label: &str, name: &str) -> Person {
    let (uid, token, _) = stack.emulator.sign_up(label).await;
    let user_id = stack
        .send(
            "POST",
            "/users/bootstrap",
            &token,
            Some(json!({ "display_name": name })),
        )
        .await
        .id("bootstrap");
    Person {
        uid,
        token,
        user_id,
    }
}

/// 관리자 역할은 부여하는 API 가 없다(최초 관리자는 밖에서 온다, §3.3). 시드 도구가
/// 하는 것과 같은 일을 여기서도 SQL 로 한다.
async fn grant_role(user_id: Uuid, role: &str) {
    let url = std::env::var("COUPON_TEST_DATABASE_URL").expect("test database");
    let pool = sqlx::PgPool::connect(&url).await.expect("connect");
    sqlx::query(
        "INSERT INTO coupon.user_roles (user_id, role)
         VALUES ($1, $2::text::coupon.account_role) ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(role)
    .execute(&pool)
    .await
    .expect("역할 부여");
    pool.close().await;
}

async fn administrator(stack: &Stack, label: &str) -> Person {
    let admin = person(stack, label, "관리자").await;
    for role in ["OPERATIONS", "SECURITY", "SUPPORT"] {
        grant_role(admin.user_id, role).await;
    }
    admin
}

struct Shop {
    owner: Person,
    id: Uuid,
}

/// 초안 → 사업자 정보 → 검수 제출 → 관리자 승인. §20 시나리오 2 그대로다.
async fn approved_store(stack: &Stack, admin: &Person, label: &str) -> Shop {
    let owner = person(stack, label, "점주").await;

    let created = stack
        .send(
            "POST",
            "/owner/store",
            &owner.token,
            Some(json!({
                "name": "인수 시나리오 카페",
                "slug": format!("acc-{}", Uuid::new_v4().simple()),
            })),
        )
        .await;
    let store_id = created.id("상점 초안");

    stack
        .send(
            "PATCH",
            "/owner/store",
            &owner.token,
            Some(json!({
                "address": { "road": "성수이로 1" },
                "business_profile": {
                    "registration_no": "123-45-67890",
                    "representative_name": "김대표",
                },
            })),
        )
        .await
        .expect_ok("사업자 정보");

    let submitted = stack
        .send(
            "POST",
            "/owner/store/submit-review",
            &owner.token,
            Some(json!({ "note": "검수 요청" })),
        )
        .await;
    let review_id = submitted.expect_ok("검수 제출")["latest_review"]["id"]
        .as_str()
        .expect("검수 id")
        .to_owned();

    let decided = stack
        .send(
            "POST",
            &format!("/admin/store-reviews/{review_id}/decision"),
            &admin.token,
            Some(json!({
                "decision": "APPROVED",
                "public_reason": "승인되었습니다.",
                "reason": "서류 확인 완료",
            })),
        )
        .await;
    decided.expect_ok("검수 승인");

    let store = stack.send("GET", "/owner/store", &owner.token, None).await;
    assert_eq!(
        store.expect_ok("상점 조회")["status"],
        "ACTIVE",
        "승인 뒤에는 ACTIVE 여야 한다"
    );

    Shop {
        owner,
        id: store_id,
    }
}

fn policy_rules(target: i64) -> Value {
    json!({
        "target_stamp_count": target,
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

async fn publish_policy(stack: &Stack, shop: &Shop, target: i64) -> Uuid {
    let draft = stack
        .send(
            "POST",
            "/owner/loyalty-policies",
            &shop.owner.token,
            Some(json!({
                "name": "10회 방문 도장",
                "rules": policy_rules(target),
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
    let policy_id = draft.id("정책 초안");

    stack
        .send(
            "POST",
            &format!("/owner/loyalty-policies/{policy_id}/publish"),
            &shop.owner.token,
            Some(json!({})),
        )
        .await
        .expect_ok("정책 게시");

    policy_id
}

async fn issue_qr(stack: &Stack, person: &Person) -> String {
    let issued = stack
        .send("POST", "/me/qr-tokens", &person.token, Some(json!({})))
        .await;
    issued.expect_ok("QR 발급")["token"]
        .as_str()
        .expect("token")
        .to_owned()
}

/// 도장 한 번 적립. 반환값은 거래 본문이다.
async fn earn_a_stamp(stack: &Stack, shop: &Shop, customer: &Person, order_ref: &str) -> Value {
    let token = issue_qr(stack, customer).await;
    let accrued = stack
        .send(
            "POST",
            "/owner/stamp-transactions",
            &shop.owner.token,
            Some(json!({
                "qr_token": token,
                "order": {
                    "external_order_ref": order_ref,
                    "gross_amount": 12_000,
                    "currency": "KRW",
                    "items": [],
                },
                "acknowledge_duplicate": true,
            })),
        )
        .await;
    accrued.expect_ok("도장 적립").clone()
}

fn campaign_draft(name: &str, issue_mode: &str, total_quantity: Value) -> Value {
    json!({
        "name": name,
        "customer_description": "시원한 한 잔 2,000원 할인",
        "benefit": { "benefit_type": "FIXED_AMOUNT", "fixed_amount": 2000 },
        "minimum_order_amount": 0,
        "issue_mode": issue_mode,
        "audience_type": "ALL_CUSTOMERS",
        "total_quantity": total_quantity,
        "per_user_quantity": 1,
        "issue_starts_at": "2020-01-01T00:00:00Z",
        "issue_ends_at": "2099-01-01T00:00:00Z",
        "usable_until": "2099-06-01T00:00:00Z",
    })
}

async fn publish_campaign(stack: &Stack, shop: &Shop, draft: Value) -> Uuid {
    let created = stack
        .send("POST", "/owner/campaigns", &shop.owner.token, Some(draft))
        .await;
    let campaign_id = created.id("캠페인 초안");

    stack
        .send(
            "POST",
            &format!("/owner/campaigns/{campaign_id}/publish"),
            &shop.owner.token,
            Some(json!({})),
        )
        .await
        .expect_ok("캠페인 게시");

    campaign_id
}

/// 지갑에 쿠폰이 들어올 때까지 기다린다. 워커가 별개 프로세스이므로 즉시가 아니다.
async fn wait_for_coupon(stack: &Stack, customer: &Person, timeout: Duration) -> Option<Value> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let wallet = stack
            .send(
                "GET",
                "/me/wallet/coupons?status=AVAILABLE&limit=50",
                &customer.token,
                None,
            )
            .await;
        if let Some(coupon) = wallet.expect_ok("지갑")["items"]
            .as_array()
            .and_then(|items| items.first())
        {
            return Some(coupon.clone());
        }
        if std::time::Instant::now() > deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

// ---------------------------------------------------------------------------
// §19.3 — Firebase 계약
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_emulator_signup_bootstraps_an_account_and_answers_me() {
    // §9.1 의 첫 문장. 이메일/비밀번호 가입 → ID Token → 내부 계정 → /me.
    // 여기가 통과하면 서버가 진짜 토큰 검증 경로로 사람을 식별했다는 뜻이다.
    let stack = stack_or_skip!();
    let (uid, token, _) = stack.emulator.sign_up("acc-signup").await;

    let bootstrapped = stack
        .send(
            "POST",
            "/users/bootstrap",
            &token,
            Some(json!({ "display_name": "김손님" })),
        )
        .await;
    assert_eq!(
        bootstrapped.status,
        reqwest::StatusCode::CREATED,
        "첫 bootstrap 은 201: {}",
        bootstrapped.json
    );

    let me = stack.send("GET", "/me", &token, None).await;
    assert_eq!(me.expect_ok("/me")["display_name"], "김손님");

    // 같은 토큰으로 다시 부르면 새 계정이 아니라 있던 계정이다(§11.2).
    let again = stack
        .send(
            "POST",
            "/users/bootstrap",
            &token,
            Some(json!({ "display_name": "김손님" })),
        )
        .await;
    assert_eq!(again.status, reqwest::StatusCode::OK);
    assert_eq!(again.data()["id"], bootstrapped.data()["id"]);

    // 토큰이 말하는 uid 가 그대로 내부 계정의 외부 식별자다.
    assert!(!uid.is_empty());
}

#[tokio::test]
async fn an_expired_id_token_is_refused_as_expired() {
    // 클라이언트의 재시도가 이 구분에 달려 있다. TOKEN_EXPIRED 는 "갱신하고 다시",
    // TOKEN_INVALID 는 "다시 로그인"이다.
    let stack = stack_or_skip!();
    let (_, token, _) = stack.emulator.sign_up("acc-expired").await;

    let now = chrono::Utc::now().timestamp();
    let expired = restamp(&token, |claims| {
        claims["iat"] = json!(now - 7200);
        claims["exp"] = json!(now - 3600);
    });

    let refused = stack.send("GET", "/me", &expired, None).await;
    assert_eq!(refused.status, reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(refused.error_code(), "TOKEN_EXPIRED", "{}", refused.json);
}

#[tokio::test]
async fn an_id_token_for_another_project_is_refused() {
    // §9.3: audience 와 issuer 를 확인한다. 다른 Firebase 프로젝트에서 발급된 토큰은
    // 서명이 유효하더라도 우리 것이 아니다.
    let stack = stack_or_skip!();
    let (_, token, _) = stack.emulator.sign_up("acc-audience").await;

    for (label, mutate) in [
        (
            "audience",
            Box::new(|claims: &mut Value| claims["aud"] = json!("someone-elses-project"))
                as Box<dyn FnOnce(&mut Value)>,
        ),
        (
            "issuer",
            Box::new(|claims: &mut Value| {
                claims["iss"] = json!("https://securetoken.google.com/someone-elses-project")
            }),
        ),
    ] {
        let foreign = restamp(&token, mutate);
        let refused = stack.send("GET", "/me", &foreign, None).await;
        assert_eq!(
            (refused.status, refused.error_code()),
            (reqwest::StatusCode::UNAUTHORIZED, "TOKEN_INVALID"),
            "{label} 불일치는 거절되어야 한다: {}",
            refused.json
        );
    }
}

#[tokio::test]
async fn a_disabled_firebase_account_cannot_get_a_new_token() {
    // §9.1 의 "계정 비활성화·토큰 폐기". Firebase 의 폐기는 발급된 ID Token 을 즉시
    // 무효로 만들지 않는다 — 갱신을 막는다. 그래서 확인할 것은 갱신이 막히는가이다.
    let stack = stack_or_skip!();
    let (uid, token, refresh_token) = stack.emulator.sign_up("acc-revoked").await;

    stack
        .send(
            "POST",
            "/users/bootstrap",
            &token,
            Some(json!({ "display_name": "폐기 대상" })),
        )
        .await
        .expect_ok("bootstrap");

    assert!(
        stack.emulator.refresh(&refresh_token).await.is_success(),
        "잠그기 전에는 갱신된다"
    );

    stack.emulator.disable(&uid).await;

    let after = stack.emulator.refresh(&refresh_token).await;
    assert!(
        !after.is_success(),
        "잠근 계정은 새 ID Token 을 받지 못해야 한다: {after}"
    );
}

#[tokio::test]
async fn the_emulator_path_is_not_a_second_bypass() {
    // emulator 를 켜 두면 서명 검증이 없는 토큰을 받아들인다. 그것이 "아무 요청이나
    // 통과한다"로 번지지 않는지가 이 설정의 안전성 전부다.
    let stack = stack_or_skip!();

    let no_credential = stack
        .send_full("GET", "/me", None, None, None, None)
        .await;
    assert_eq!(no_credential.status, reqwest::StatusCode::UNAUTHORIZED);

    for junk in ["not-a-token", "a.b.c", ""] {
        let refused = stack.send("GET", "/me", junk, None).await;
        assert_eq!(
            refused.status,
            reqwest::StatusCode::UNAUTHORIZED,
            "{junk:?} 는 거절되어야 한다: {}",
            refused.json
        );
    }

    // COUPON_AUTH_DEV_BYPASS 를 켜지 않았으므로 개발용 헤더는 아무 힘이 없다.
    let dev_header = stack
        .http
        .get(format!("{}/me", stack.api))
        .header("x-dev-firebase-uid", "누구나")
        .send()
        .await
        .expect("응답");
    assert_eq!(dev_header.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn verifying_an_email_promotes_the_account_that_was_created_before_it() {
    // 이 스위트가 처음 드러낸 결함의 자리. 프로세스 안에서 도는 스위트들은
    // `COUPON_AUTH_DEV_BYPASS` 를 쓰고, 그 가짜 토큰은 언제나 `email_verified: true` 라서
    // 이 경로를 밟지 않는다 — 실제 Firebase 토큰으로만 드러난다.
    //
    // 예전 동작: `POST /users/bootstrap` 이 처음 본 토큰의 `email_verified` 로 상태를 정하고
    // `ON CONFLICT DO NOTHING` 으로 끝냈다. 나중에 이메일을 인증해도 그 행은 그대로였고,
    // 대상자 질의는 `users.status = 'ACTIVE'` 만 세므로 인증 전에 가입한 사람은 그 뒤
    // 무엇을 하든 캠페인 대상에서 영원히 빠졌다. 아무 오류도 나지 않았다.
    //
    // 지금 동작: 인증된 토큰으로 bootstrap 이 다시 오면 `PENDING_VERIFICATION → ACTIVE`
    // 로 승격한다. 승격 이후 실제로 대상자에 들어오는지는 `tests/auth.rs` 가 캠페인까지
    // 세워 확인한다. 여기서는 **실제 Firebase 토큰**으로 그 승격이 일어나는지를 본다.
    let stack = stack_or_skip!();

    let (uid, email, token) = stack.emulator.sign_up_unverified("acc-unverified").await;
    let bootstrapped = stack
        .send(
            "POST",
            "/users/bootstrap",
            &token,
            Some(json!({ "display_name": "미인증" })),
        )
        .await;
    assert_eq!(
        bootstrapped.expect_ok("bootstrap")["status"],
        "PENDING_VERIFICATION",
        "인증 전 가입은 대기 상태로 만들어진다"
    );

    // 인증 전 토큰으로는 아무리 다시 불러도 승격되지 않는다. 승격의 근거는 토큰이 실제로
    // 인증됐다고 말하는 것 하나뿐이다.
    let unchanged = stack
        .send("POST", "/users/bootstrap", &token, Some(json!({})))
        .await;
    assert_eq!(
        unchanged.expect_ok("bootstrap")["status"],
        "PENDING_VERIFICATION",
        "미인증 토큰은 승격의 근거가 아니다"
    );

    // 사용자가 인증 메일을 누르고 다시 로그인한다.
    stack.emulator.verify_email(&uid).await;
    let verified_token = stack.emulator.sign_in(&email).await["idToken"]
        .as_str()
        .expect("idToken")
        .to_owned();

    // 아직은 그대로다 — 승격은 bootstrap 이 한다. §6.1 `/verify-email` 의 "인증 완료
    // 재조회"가 바로 이 호출이다.
    let before = stack.send("GET", "/me", &verified_token, None).await;
    assert_eq!(
        before.expect_ok("/me")["status"],
        "PENDING_VERIFICATION",
        "토큰만으로는 계정 상태가 바뀌지 않는다"
    );

    let promoted = stack
        .send("POST", "/users/bootstrap", &verified_token, Some(json!({})))
        .await;
    assert_eq!(
        promoted.expect_ok("re-bootstrap")["status"],
        "ACTIVE",
        "인증된 토큰으로 다시 오면 승격된다"
    );
    assert_eq!(promoted.expect_ok("re-bootstrap")["email_verified"], true);

    let after = stack.send("GET", "/me", &verified_token, None).await;
    assert_eq!(after.expect_ok("/me")["status"], "ACTIVE");
}

#[tokio::test]
async fn a_stale_sign_in_cannot_reach_a_high_risk_endpoint() {
    // §9.3: 고위험 API 는 `auth_time` 이 10분 이내여야 한다. 10분을 기다리는 대신
    // 그 설정을 1초로 둔 서버를 따로 띄운다 — 검사하는 코드는 완전히 같다.
    let Ok(database_url) = std::env::var("COUPON_TEST_DATABASE_URL") else {
        eprintln!("건너뜀: COUPON_TEST_DATABASE_URL 이 없습니다");
        return;
    };
    let emulator_host = emulator_host();
    if !emulator_reachable(&emulator_host).await {
        eprintln!("건너뜀: Auth emulator 가 응답하지 않습니다");
        return;
    }

    let port = free_port();
    let _server = spawn(
        env!("CARGO_BIN_EXE_coupon-api"),
        &[
            ("COUPON_BIND_ADDR", &format!("127.0.0.1:{port}")),
            ("COUPON_DATABASE_URL", &database_url),
            ("COUPON_FIREBASE_AUTH_EMULATOR_HOST", &emulator_host),
            ("COUPON_RECENT_AUTH_MAX_AGE_SECS", "1"),
        ],
    );

    let strict = Stack {
        api: format!("http://127.0.0.1:{port}/api/coupon/v1"),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("client"),
        emulator: EmulatorClient::new(&emulator_host),
    };
    wait_until_ready(&strict.http, &strict.api).await;

    let owner = person(&strict, "acc-stale", "점주").await;
    strict
        .send(
            "POST",
            "/owner/store",
            &owner.token,
            Some(json!({
                "name": "느린 로그인 카페",
                "slug": format!("acc-{}", Uuid::new_v4().simple()),
            })),
        )
        .await
        .expect_ok("상점 초안");
    strict
        .send(
            "PATCH",
            "/owner/store",
            &owner.token,
            Some(json!({
                "address": { "road": "성수이로 1" },
                "business_profile": {
                    "registration_no": "123-45-67890",
                    "representative_name": "김대표",
                },
            })),
        )
        .await
        .expect_ok("사업자 정보");

    // 로그인이 오래되도록 둔다.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let refused = strict
        .send(
            "POST",
            "/owner/store/submit-review",
            &owner.token,
            Some(json!({ "note": "검수 요청" })),
        )
        .await;
    assert_eq!(
        (refused.status, refused.error_code()),
        (
            reqwest::StatusCode::FORBIDDEN,
            "REAUTHENTICATION_REQUIRED"
        ),
        "오래된 로그인은 고위험 API 에 닿지 못한다: {}",
        refused.json
    );
}

#[tokio::test]
async fn a_mutation_from_an_unlisted_origin_is_refused() {
    // §16.3. 프로세스 안에서 라우터를 직접 부르는 테스트로는 이 층을 지나갈 일이 없다.
    let stack = stack_or_skip!();
    let (_, token, _) = stack.emulator.sign_up("acc-origin").await;

    let refused = stack
        .send_full(
            "POST",
            "/users/bootstrap",
            Some(&token),
            Some(json!({ "display_name": "다른 출처" })),
            None,
            Some("https://evil.example"),
        )
        .await;
    assert_eq!(
        (refused.status, refused.error_code()),
        (reqwest::StatusCode::FORBIDDEN, "ORIGIN_NOT_ALLOWED"),
        "{}",
        refused.json
    );

    // 허용된 오리진은 그대로 통과한다.
    let allowed = stack
        .send_full(
            "POST",
            "/users/bootstrap",
            Some(&token),
            Some(json!({ "display_name": "우리 앱" })),
            None,
            Some(ALLOWED_ORIGIN),
        )
        .await;
    allowed.expect_ok("허용된 오리진");
}

// ---------------------------------------------------------------------------
// §20 — MVP 인수 시나리오
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_2_a_store_is_opened_approved_and_given_a_ten_visit_policy() {
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s2-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s2").await;

    let policy_id = publish_policy(&stack, &shop, 10).await;

    let policies = stack
        .send(
            "GET",
            "/owner/loyalty-policies?limit=50",
            &shop.owner.token,
            None,
        )
        .await;
    let active = policies.expect_ok("정책 목록")["policies"]
        .as_array()
        .expect("policies")
        .iter()
        .find(|policy| policy["id"] == policy_id.to_string())
        .expect("게시한 정책")
        .clone();

    assert_eq!(active["status"], "ACTIVE");
    assert_eq!(active["target_stamp_count"], 10);
}

#[tokio::test]
async fn scenario_3_the_tenth_stamp_issues_exactly_one_reward() {
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s3-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s3").await;
    publish_policy(&stack, &shop, 10).await;

    let customer = person(&stack, "acc-s3-customer", "김손님").await;

    for visit in 1..=9 {
        let accrued = earn_a_stamp(&stack, &shop, &customer, &format!("ACC-S3-{visit}")).await;
        assert!(
            accrued["issued_rewards"]
                .as_array()
                .expect("issued_rewards")
                .is_empty(),
            "{visit}번째에는 리워드가 나오면 안 된다: {accrued}"
        );
    }

    let tenth = earn_a_stamp(&stack, &shop, &customer, "ACC-S3-10").await;
    assert_eq!(
        tenth["issued_rewards"]
            .as_array()
            .expect("issued_rewards")
            .len(),
        1,
        "10번째에 리워드가 정확히 한 장 나온다: {tenth}"
    );

    let wallet = stack
        .send(
            "GET",
            "/me/wallet/coupons?status=AVAILABLE&limit=50",
            &customer.token,
            None,
        )
        .await;
    let coupons = wallet.expect_ok("지갑")["items"]
        .as_array()
        .expect("items")
        .clone();
    assert_eq!(coupons.len(), 1, "리워드는 한 장뿐이다: {coupons:?}");
}

#[tokio::test]
async fn scenario_4_a_retried_use_with_the_same_key_is_recorded_once() {
    // §20 시나리오 4. 네트워크 응답이 유실된 뒤 클라이언트가 같은 멱등키로 다시 보낸다.
    // 사용은 한 번만 기록되어야 하고, 두 번째 응답은 첫 번째와 같아야 한다.
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s4-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s4").await;
    publish_policy(&stack, &shop, 10).await;

    let customer = person(&stack, "acc-s4-customer", "김손님").await;
    earn_a_stamp(&stack, &shop, &customer, "ACC-S4-1").await;

    let campaign_id = publish_campaign(
        &stack,
        &shop,
        campaign_draft(
            "재시도 시나리오",
            "FIRST_COME",
            json!({ "mode": "LIMITED", "quantity": 5 }),
        ),
    )
    .await;
    stack
        .send(
            "POST",
            &format!("/campaigns/{campaign_id}/claims"),
            &customer.token,
            Some(json!({})),
        )
        .await
        .expect_ok("받기");

    let coupon = wait_for_coupon(&stack, &customer, Duration::from_secs(5))
        .await
        .expect("받은 쿠폰");
    let coupon_id = coupon["id"].as_str().expect("id").to_owned();

    let order = json!({
        "external_order_ref": "ACC-S4-USE",
        "gross_amount": 12_000,
        "currency": "KRW",
        "items": [],
    });
    let qr = issue_qr(&stack, &customer).await;
    let reserved = stack
        .send(
            "POST",
            "/owner/redemptions/preview",
            &shop.owner.token,
            Some(json!({
                "qr_token": qr,
                "coupon_id": coupon_id,
                "owner_session_id": "acc-till",
                "order": order,
            })),
        )
        .await;
    let reservation_id = reserved.expect_ok("사용 예약")["reservation_id"]
        .as_str()
        .expect("id")
        .to_owned();

    // 같은 멱등키로 두 번. 클라이언트가 첫 응답을 못 받은 상황 그대로다.
    let key = Uuid::new_v4();
    let path = format!("/owner/redemptions/{reservation_id}/confirm");
    let body = json!({ "owner_session_id": "acc-till", "order": order });

    let first = stack
        .send_full(
            "POST",
            &path,
            Some(&shop.owner.token),
            Some(body.clone()),
            Some(key),
            None,
        )
        .await;
    let first = first.expect_ok("첫 승인").clone();

    let replay = stack
        .send_full(
            "POST",
            &path,
            Some(&shop.owner.token),
            Some(body),
            Some(key),
            None,
        )
        .await;
    let replayed = replay.expect_ok("재시도").clone();

    assert_eq!(first, replayed, "재시도는 첫 응답을 그대로 돌려준다");
    assert_eq!(first["coupon_status"], "USED");

    // 지갑에서도 한 번만 쓰였다.
    let detail = stack
        .send(
            "GET",
            &format!("/me/wallet/coupons/{coupon_id}"),
            &customer.token,
            None,
        )
        .await;
    let detail = detail.expect_ok("쿠폰 상세");
    assert_eq!(detail["effective_status"], "USED");
    let uses = detail["history"]
        .as_array()
        .expect("history")
        .iter()
        .filter(|event| event["to_status"] == "USED")
        .count();
    assert_eq!(uses, 1, "사용 기록은 한 건뿐이다: {detail}");
}

#[tokio::test]
async fn scenario_5_simultaneous_claims_on_the_last_coupon_produce_one_winner() {
    // §20 시나리오 5. 프로세스 안에서 도는 판본이 이미 있지만, 여기서는 요청이 각각
    // 별개 TCP 연결로 들어온다 — 서버가 실제로 받는 모양 그대로다.
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s5-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s5").await;
    publish_policy(&stack, &shop, 10).await;

    let campaign_id = publish_campaign(
        &stack,
        &shop,
        campaign_draft(
            "마지막 한 장",
            "FIRST_COME",
            json!({ "mode": "LIMITED", "quantity": 1 }),
        ),
    )
    .await;

    // 열 명이 동시에 손을 뻗는다.
    let mut contenders = Vec::new();
    for index in 0..10 {
        contenders.push(person(&stack, &format!("acc-s5-{index}"), "김손님").await);
    }

    let attempts: Vec<_> = contenders
        .iter()
        .map(|contender| {
            let stack = stack.clone();
            let token = contender.token.clone();
            let path = format!("/campaigns/{campaign_id}/claims");
            tokio::spawn(async move { stack.send("POST", &path, &token, Some(json!({}))).await })
        })
        .collect();

    let mut winners = 0;
    for attempt in attempts {
        let reply = attempt.await.expect("작업이 죽지 않았다");
        if reply.status.is_success() {
            winners += 1;
        } else {
            assert_eq!(
                reply.error_code(),
                "CAMPAIGN_SOLD_OUT",
                "진 사람은 소진으로 진다 ({}): {}",
                reply.status,
                reply.json
            );
        }
    }
    assert_eq!(winners, 1, "마지막 한 장은 한 명에게만 간다");
}

#[tokio::test]
async fn scenario_7_a_separate_worker_process_issues_a_direct_campaign_once() {
    // §20 시나리오 7 의 절반. 발급을 하는 것은 API 가 아니라 **다른 프로세스**인
    // `coupon-worker` 다. 프로세스 안에서 런타임을 프로세스 안에서 직접 돌리는 테스트로는 그 사실이
    // 확인되지 않는다.
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s7-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s7").await;
    publish_policy(&stack, &shop, 10).await;

    // 대상자가 되려면 이 가게의 손님이어야 한다. 도장 한 번이 그렇게 만든다.
    let customer = person(&stack, "acc-s7-customer", "김단골").await;
    earn_a_stamp(&stack, &shop, &customer, "ACC-S7-1").await;

    let campaign_id = publish_campaign(
        &stack,
        &shop,
        campaign_draft(
            "단골 직접 지급",
            "DIRECT",
            json!({ "mode": "UNLIMITED", "operational_cap": 100 }),
        ),
    )
    .await;

    // 워커가 대상자를 만들고 발급할 때까지 기다린다.
    //
    // 넉넉한 이유가 있다. 테스트 데이터베이스는 비워지지 않고, 프로세스 안에서 도는
    // 다른 스위트들은 자기가 지정한 잡만 실행하므로 실행되지 않은 `QUEUED` 잡이 계속
    // 쌓인다. 실제 워커는 due 순서대로 한 번에 열여섯 개씩 가져가므로, 우리 잡 차례가
    // 오기까지 그 밀린 만큼을 먼저 치워야 한다. 폴링 간격(5초)만 보고 잡은 시간이
    // "워커가 안 돈다"로 보이는 실패가 되기 쉽다.
    let coupon = match wait_for_coupon(&stack, &customer, Duration::from_secs(240)).await {
        Some(coupon) => coupon,
        None => {
            let backlog = stack
                .send("GET", "/admin/jobs?status=QUEUED", &admin.token, None)
                .await;
            panic!(
                "워커가 발급하지 않았다 (campaign {campaign_id}). \
                 밀린 QUEUED 잡: {}",
                backlog
                    .data()
                    .as_array()
                    .map(|jobs| jobs.len().to_string())
                    .unwrap_or_else(|| backlog.json.to_string())
            )
        }
    };
    assert_eq!(coupon["effective_status"], "AVAILABLE");

    // 두 잡 모두 성공으로 남았고, 고객에게 간 것은 한 장뿐이다.
    let jobs = stack
        .send(
            "GET",
            &format!("/admin/jobs?resource_id={campaign_id}"),
            &admin.token,
            None,
        )
        .await;
    let issue = jobs
        .expect_ok("잡 목록")
        .as_array()
        .expect("jobs")
        .iter()
        .find(|job| job["job_type"] == "issue_campaign")
        .cloned()
        .unwrap_or_else(|| panic!("발급 잡이 없다: {}", jobs.json));
    assert_eq!(issue["status"], "SUCCEEDED", "{issue}");

    let wallet = stack
        .send(
            "GET",
            "/me/wallet/coupons?status=AVAILABLE&limit=50",
            &customer.token,
            None,
        )
        .await;
    assert_eq!(
        wallet.expect_ok("지갑")["items"]
            .as_array()
            .expect("items")
            .len(),
        1,
        "고객별 발급은 한 번뿐이다"
    );
}

#[tokio::test]
async fn scenario_9_voiding_an_accrual_restores_the_balance_and_takes_back_the_reward() {
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s9-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s9").await;
    publish_policy(&stack, &shop, 3).await;

    let customer = person(&stack, "acc-s9-customer", "김손님").await;
    earn_a_stamp(&stack, &shop, &customer, "ACC-S9-1").await;
    earn_a_stamp(&stack, &shop, &customer, "ACC-S9-2").await;
    let third = earn_a_stamp(&stack, &shop, &customer, "ACC-S9-3").await;

    assert_eq!(
        third["issued_rewards"]
            .as_array()
            .expect("issued_rewards")
            .len(),
        1,
        "세 번째에 리워드가 나온다: {third}"
    );
    let transaction_id = third["transaction_id"]
        .as_str()
        .expect("거래 id")
        .to_owned();

    let voided = stack
        .send(
            "POST",
            &format!("/owner/stamp-transactions/{transaction_id}/void"),
            &shop.owner.token,
            Some(json!({ "reason": "주문 취소" })),
        )
        .await;
    voided.expect_ok("적립 취소");

    // 원장이 되돌아왔다: 도장은 2개, 리워드는 회수.
    let stamps = stack
        .send("GET", "/me/wallet/stamps", &customer.token, None)
        .await;
    assert_eq!(
        stamps.expect_ok("도장 지갑")["total_available"],
        2,
        "취소한 만큼만 줄어든다"
    );

    let wallet = stack
        .send(
            "GET",
            "/me/wallet/coupons?status=AVAILABLE&limit=50",
            &customer.token,
            None,
        )
        .await;
    assert!(
        wallet.expect_ok("지갑")["items"]
            .as_array()
            .expect("items")
            .is_empty(),
        "회수된 리워드는 사용 가능 목록에 없다"
    );
}

#[tokio::test]
async fn scenario_10_an_administrator_can_follow_a_transaction_and_the_audit_log_holds() {
    let stack = stack_or_skip!();
    let admin = administrator(&stack, "acc-s10-admin").await;
    let shop = approved_store(&stack, &admin, "acc-s10").await;
    publish_policy(&stack, &shop, 10).await;

    let customer = person(&stack, "acc-s10-customer", "김손님").await;
    let accrued = earn_a_stamp(&stack, &shop, &customer, "ACC-S10-1").await;
    let transaction_id = accrued["transaction_id"]
        .as_str()
        .expect("거래 id")
        .to_owned();

    // 민원 증거: 관리자가 거래 하나를 끝까지 따라간다.
    let evidence = stack
        .send(
            "GET",
            &format!("/admin/transactions/{transaction_id}"),
            &admin.token,
            None,
        )
        .await;
    let evidence = evidence.expect_ok("거래 조회");
    assert_eq!(evidence["store_id"], shop.id.to_string());

    // 그리고 그 조회·승인이 감사 로그에 남는다. 체인이 성한지는 서버가 스스로 말한다.
    let audit = stack
        .send("GET", "/admin/audit-logs?limit=1", &admin.token, None)
        .await;
    let audit = audit.expect_ok("감사 로그");
    let entries = audit["items"].as_array().expect("items");
    assert!(!entries.is_empty(), "감사 로그가 비어 있다: {audit}");
    assert!(
        entries
            .iter()
            .all(|entry| entry["chain_intact"].as_bool().unwrap_or(false)),
        "감사 로그 체인이 끊어졌다: {audit}"
    );
}
