//! `coupon-seed` — 인수 시나리오(§20)와 실기기 검증(§19.5)이 필요로 하는 상태를 한 번에 만든다.
//!
//! 사람이 손으로 만들 수 있는 상태가 아니다. 승인된 ACTIVE 상점 하나를 얻으려면 계정 생성 →
//! 상점 초안 → 사업자 정보 → 검수 제출 → 관리자 승인까지 다섯 번을 서로 다른 권한으로
//! 호출해야 하고, 그 위에 정책·캠페인·지갑이 얹힌다. 실기기 앞에서 그걸 매번 다시 하는 것은
//! 검증이 아니라 준비다.
//!
//! ## 원칙
//!
//! * **API 를 탄다.** 상점 승인은 `POST /admin/store-reviews/{id}/decision` 을 실제로
//!   호출해서 얻는다. `UPDATE stores SET status='ACTIVE'` 로 우겨넣으면 시드는 성공하지만
//!   그렇게 만든 상점은 실제 승인 경로를 한 번도 통과하지 않은 상점이다.
//! * **멱등하다.** 몇 번을 돌려도 같은 상태로 수렴한다. 계정·상점·정책·캠페인은 모두
//!   결정적 이름으로 찾고, 없을 때만 만든다.
//! * **DB 는 두 가지에만 쓴다.** 관리자 역할 부여(부여하는 API 가 없다 — 최초 관리자는 어디선가
//!   와야 한다)와 `--reset`. 나머지는 전부 HTTP 다.
//!
//! ## 쓰는 법
//!
//! ```sh
//! # dev bypass 로 (서버가 COUPON_AUTH_DEV_BYPASS=1 로 떠 있어야 한다)
//! export COUPON_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon
//! cargo run --bin coupon-seed -- --api-url http://127.0.0.1:7810
//!
//! # Firebase Auth emulator 로 (실기기 검증은 이쪽이다 — 진짜 이메일/비밀번호로 로그인한다)
//! ./apps/coupon-api-server/scripts/auth-emulator.sh up
//! export COUPON_FIREBASE_AUTH_EMULATOR_HOST=192.168.150.185:9099
//! export COUPON_FIREBASE_PROJECT_ID=ddadan-dev
//! cargo run --bin coupon-seed -- --api-url http://192.168.150.185:7810
//!
//! # 시드 데이터만 지우고 다시
//! cargo run --bin coupon-seed -- --reset
//! ```
//!
//! emulator 설정이 있으면 emulator 에 계정을 만들고 **진짜 ID Token 으로** API 를 부른다.
//! 없으면 `X-Dev-Firebase-Uid` 로 부른다. 어느 쪽이든 만들어지는 도메인 상태는 같다.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Context, anyhow, bail};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// 시드가 만든 것을 알아보는 표식. `--reset` 의 범위이기도 하다.
const SEED_PREFIX: &str = "seed-";
/// 상점 slug. 결정적이어야 두 번째 실행이 같은 상점을 찾는다.
const STORE_SLUG: &str = "seed-ddadan-cafe";
/// 시드 계정 공통 비밀번호. 로컬 emulator 전용 값이고 어디에도 재사용하지 않는다.
const SEED_PASSWORD: &str = "SeedPass!234";
/// §20 시나리오 2 의 "10회 방문".
const STAMP_GOAL: i64 = 10;
/// 단골 소비자가 들고 있을 도장 수. 목표 미만이어야 리워드가 아직 안 나온 상태가 된다.
const VETERAN_STAMPS: i64 = 7;

// ---------------------------------------------------------------------------
// 계정
// ---------------------------------------------------------------------------

/// 시드가 만드는 계정 하나.
#[derive(Debug, Clone)]
struct Account {
    /// `seed-` 로 시작하는 결정적 Firebase UID. emulator 에서도 이 값을 그대로 쓴다.
    uid: String,
    email: String,
    display_name: String,
    /// 이 계정으로 API 를 부를 때 붙일 것. emulator 면 ID Token, 아니면 dev 헤더.
    credential: Credential,
    user_id: Option<Uuid>,
    roles: Vec<&'static str>,
}

#[derive(Debug, Clone)]
enum Credential {
    /// `Authorization: Bearer <emulator ID token>` — 실제 토큰 검증 경로를 탄다.
    Bearer(String),
    /// `X-Dev-Firebase-Uid` — `COUPON_AUTH_DEV_BYPASS=1` 인 서버에서만 통한다.
    DevUid(String),
}

/// 계정 정의. 실제 생성은 [`ensure_accounts`] 가 한다.
struct AccountSpec {
    key: &'static str,
    suffix: &'static str,
    display_name: &'static str,
    /// DB 로 직접 부여할 관리자 역할. 소비자·점주는 비어 있다 —
    /// `CONSUMER` 는 bootstrap 이, `STORE_OWNER` 는 상점 생성이 준다.
    roles: &'static [&'static str],
}

const ACCOUNTS: &[AccountSpec] = &[
    AccountSpec {
        key: "consumer-veteran",
        suffix: "consumer-veteran",
        display_name: "김단골",
        roles: &[],
    },
    AccountSpec {
        key: "consumer-new",
        suffix: "consumer-new",
        display_name: "이신규",
        roles: &[],
    },
    AccountSpec {
        key: "owner",
        suffix: "owner",
        display_name: "박점주",
        roles: &[],
    },
    AccountSpec {
        key: "admin-support",
        suffix: "admin-support",
        display_name: "고객지원 담당",
        roles: &["SUPPORT"],
    },
    AccountSpec {
        key: "admin-operations",
        suffix: "admin-operations",
        display_name: "운영 담당",
        roles: &["OPERATIONS"],
    },
    AccountSpec {
        key: "admin-security",
        suffix: "admin-security",
        display_name: "보안 담당",
        roles: &["SECURITY"],
    },
    AccountSpec {
        key: "admin-super",
        suffix: "admin-super",
        display_name: "시스템 관리자",
        roles: &["SUPER_ADMIN"],
    },
];

// ---------------------------------------------------------------------------
// 실행
// ---------------------------------------------------------------------------

struct Options {
    api_url: String,
    reset: bool,
    /// `--reset-only` — 지우기만 하고 다시 만들지 않는다.
    reset_only: bool,
}

fn parse_options() -> anyhow::Result<Options> {
    let mut options = Options {
        api_url: std::env::var("COUPON_SEED_API_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:7810".to_owned()),
        reset: false,
        reset_only: false,
    };

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--api-url" => {
                options.api_url = args
                    .next()
                    .ok_or_else(|| anyhow!("--api-url 뒤에 주소가 필요합니다"))?
            }
            "--reset" => options.reset = true,
            "--reset-only" => {
                options.reset = true;
                options.reset_only = true;
            }
            "--help" | "-h" => {
                println!(
                    "coupon-seed [--api-url URL] [--reset] [--reset-only]\n\n\
                     환경변수:\n  \
                     COUPON_DATABASE_URL                 (필수)\n  \
                     COUPON_SEED_API_URL                 --api-url 기본값\n  \
                     COUPON_FIREBASE_AUTH_EMULATOR_HOST  있으면 emulator 계정과 진짜 ID Token 사용\n  \
                     COUPON_FIREBASE_PROJECT_ID          emulator 프로젝트 (기본 ddadan-dev)"
                );
                std::process::exit(0);
            }
            other => bail!("모르는 옵션입니다: {other}"),
        }
    }

    options.api_url = options.api_url.trim_end_matches('/').to_owned();
    Ok(options)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = parse_options()?;

    let database_url = std::env::var("COUPON_DATABASE_URL")
        .context("COUPON_DATABASE_URL 이 필요합니다 (관리자 역할 부여와 --reset 에 씁니다)")?;
    let pool = PgPool::connect(&database_url)
        .await
        .context("PostgreSQL 에 연결하지 못했습니다")?;

    let emulator = Emulator::from_env();
    let api = Api::new(&options.api_url);

    println!("API      : {}", options.api_url);
    match &emulator {
        Some(emulator) => println!(
            "인증     : Firebase Auth emulator {} (project {})",
            emulator.host, emulator.project_id
        ),
        None => println!(
            "인증     : COUPON_AUTH_DEV_BYPASS (X-Dev-Firebase-Uid). \
             실기기 검증은 emulator 로 하십시오."
        ),
    }

    api.wait_until_ready().await?;

    if options.reset {
        ensure_reset_is_local(&database_url)?;
        let removed = reset(&pool).await?;
        println!("초기화   : {removed}개 행 삭제");
        if options.reset_only {
            return Ok(());
        }
    }

    let mut accounts = ensure_accounts(&api, &pool, emulator.as_ref()).await?;
    let store = ensure_active_store(&api, &accounts).await?;
    // 품목 자체는 다른 단계가 참조하지 않는다. 점주 앱의 품목·정책 화면이 비어 있지
    // 않게 하는 것이 목적이다.
    ensure_catalog(&api, &accounts).await?;
    let policy = ensure_policy(&api, &accounts).await?;
    ensure_stamps(&api, &accounts, VETERAN_STAMPS).await?;
    let campaigns = ensure_campaigns(&api, &accounts).await?;
    let wallet = ensure_wallet(&api, &pool, &accounts, &campaigns).await?;

    // uid 는 bootstrap 이후에야 내부 user_id 를 갖는다. 표에 찍기 전에 채워 넣는다.
    for account in accounts.values_mut() {
        if account.user_id.is_none() {
            account.user_id = lookup_user_id(&pool, &account.uid).await?;
        }
    }

    print_report(
        &options,
        emulator.as_ref(),
        &accounts,
        &store,
        &policy,
        &campaigns,
        &wallet,
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Firebase Auth emulator
// ---------------------------------------------------------------------------

/// emulator 에 계정을 만들고 ID Token 을 받아오는 부분.
///
/// emulator 는 인증 없이 Identity Toolkit 의 관리자 엔드포인트를 열어 두므로 `localId` 를
/// 우리가 정할 수 있다. 그래서 emulator 를 써도 Firebase UID 가 `seed-` 로 시작하고,
/// `--reset` 의 범위가 dev bypass 일 때와 똑같아진다.
struct Emulator {
    host: String,
    project_id: String,
    http: reqwest::Client,
}

impl Emulator {
    fn from_env() -> Option<Self> {
        let host = std::env::var("COUPON_FIREBASE_AUTH_EMULATOR_HOST")
            .ok()
            .map(|host| host.trim().to_owned())
            .filter(|host| !host.is_empty())?;
        Some(Self {
            host,
            project_id: std::env::var("COUPON_FIREBASE_PROJECT_ID")
                .unwrap_or_else(|_| "ddadan-dev".to_owned()),
            http: reqwest::Client::new(),
        })
    }

    fn base(&self) -> String {
        format!("http://{}/identitytoolkit.googleapis.com/v1", self.host)
    }

    /// 계정이 없으면 만들고, 있으면 그대로 둔다. 어느 쪽이든 ID Token 을 돌려준다.
    async fn ensure_user(
        &self,
        uid: &str,
        email: &str,
        display_name: &str,
    ) -> anyhow::Result<String> {
        let created = self
            .http
            .post(format!("{}/projects/{}/accounts", self.base(), self.project_id))
            // emulator 의 관리자 인증 토큰. 실제 값은 검사하지 않는다.
            .header("Authorization", "Bearer owner")
            .json(&json!({
                "localId": uid,
                "email": email,
                "password": SEED_PASSWORD,
                "emailVerified": true,
                "displayName": display_name,
            }))
            .send()
            .await
            .context("emulator 에 계정을 만들지 못했습니다")?;

        let status = created.status();
        let body: Value = created.json().await.unwrap_or(Value::Null);
        let code = body["error"]["message"].as_str().unwrap_or_default();
        // 두 번째 실행에서 정상적으로 나오는 응답이다.
        let already_there = code == "DUPLICATE_LOCAL_ID" || code == "EMAIL_EXISTS";
        if !status.is_success() && !already_there {
            bail!("emulator 계정 생성 실패 ({status}): {body}");
        }

        if already_there {
            // 있는 계정을 그대로 두지 않고 이메일·비밀번호를 다시 못 박는다. emulator 는
            // 프로세스를 껐다 켜는 것만으로도 상태가 바뀌고, 이 도구가 표에 찍어 준
            // 로그인 정보가 실제로 통하지 않는 것이 가장 나쁜 실패다.
            self.reset_credentials(uid, email, display_name).await?;
        }

        self.sign_in(email).await
    }

    /// 기존 계정의 이메일·비밀번호를 시드가 약속한 값으로 되돌린다.
    async fn reset_credentials(
        &self,
        uid: &str,
        email: &str,
        display_name: &str,
    ) -> anyhow::Result<()> {
        let updated = self
            .http
            .post(format!("{}/accounts:update", self.base()))
            .header("Authorization", "Bearer owner")
            .json(&json!({
                "localId": uid,
                "email": email,
                "password": SEED_PASSWORD,
                "emailVerified": true,
                "displayName": display_name,
            }))
            .send()
            .await
            .context("emulator 계정을 갱신하지 못했습니다")?;

        if !updated.status().is_success() {
            let status = updated.status();
            let body: Value = updated.json().await.unwrap_or(Value::Null);
            bail!("emulator 계정 갱신 실패 ({status}): {body}");
        }
        Ok(())
    }

    /// 이메일/비밀번호로 로그인해 ID Token 을 받는다. 앱이 하는 것과 같은 호출이다.
    async fn sign_in(&self, email: &str) -> anyhow::Result<String> {
        let response = self
            .http
            .post(format!(
                "{}/accounts:signInWithPassword?key=fake-api-key",
                self.base()
            ))
            .json(&json!({
                "email": email,
                "password": SEED_PASSWORD,
                "returnSecureToken": true,
            }))
            .send()
            .await
            .context("emulator 로그인에 실패했습니다")?;

        let body: Value = response.json().await.context("emulator 응답을 읽지 못함")?;
        body["idToken"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("emulator 가 ID Token 을 주지 않았습니다: {body}"))
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

struct Api {
    base: String,
    http: reqwest::Client,
}

/// 응답 하나. 실패를 예외로 만들지 않는 이유는 시드가 "이미 있음"(409)을 정상 경로로
/// 다루기 때문이다.
struct Reply {
    status: reqwest::StatusCode,
    json: Value,
}

impl Reply {
    fn ok(&self, what: &str) -> anyhow::Result<&Value> {
        if !self.status.is_success() {
            bail!("{what} 실패 ({}): {}", self.status, self.json);
        }
        Ok(&self.json["data"])
    }

    fn error_code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    fn id(&self, what: &str) -> anyhow::Result<Uuid> {
        let data = self.ok(what)?;
        let raw = data["id"]
            .as_str()
            .ok_or_else(|| anyhow!("{what}: 응답에 id 가 없습니다: {data}"))?;
        Uuid::parse_str(raw).with_context(|| format!("{what}: id 가 UUID 가 아닙니다"))
    }
}

impl Api {
    fn new(base: &str) -> Self {
        Self {
            base: format!("{base}/api/coupon/v1"),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    /// 서버가 뜰 때까지 잠깐 기다린다. 시드를 서버 기동 직후에 돌리는 경우가 대부분이다.
    async fn wait_until_ready(&self) -> anyhow::Result<()> {
        for attempt in 0..30 {
            match self.http.get(format!("{}/health/ready", self.base)).send().await {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) if attempt == 29 => {
                    bail!("서버가 준비되지 않았습니다: {}", response.status())
                }
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
        bail!("{} 에 연결하지 못했습니다", self.base)
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        account: &Account,
        body: Option<Value>,
    ) -> anyhow::Result<Reply> {
        let mutation = method != reqwest::Method::GET;
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("content-type", "application/json");

        request = match &account.credential {
            Credential::Bearer(token) => request.header("authorization", format!("Bearer {token}")),
            Credential::DevUid(uid) => request.header("x-dev-firebase-uid", uid.as_str()),
        };
        if mutation {
            request = request.header("idempotency-key", Uuid::new_v4().to_string());
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("{path} 호출에 실패했습니다"))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        Ok(Reply {
            status,
            json: serde_json::from_str(&text).unwrap_or(Value::String(text)),
        })
    }

    async fn get(&self, path: &str, account: &Account) -> anyhow::Result<Reply> {
        self.call(reqwest::Method::GET, path, account, None).await
    }

    async fn post(&self, path: &str, account: &Account, body: Value) -> anyhow::Result<Reply> {
        self.call(reqwest::Method::POST, path, account, Some(body))
            .await
    }

    async fn patch(&self, path: &str, account: &Account, body: Value) -> anyhow::Result<Reply> {
        self.call(reqwest::Method::PATCH, path, account, Some(body))
            .await
    }
}

// ---------------------------------------------------------------------------
// 단계
// ---------------------------------------------------------------------------

type Accounts = BTreeMap<&'static str, Account>;

fn account<'a>(accounts: &'a Accounts, key: &str) -> &'a Account {
    accounts.get(key).expect("시드가 정의한 계정")
}

async fn ensure_accounts(
    api: &Api,
    pool: &PgPool,
    emulator: Option<&Emulator>,
) -> anyhow::Result<Accounts> {
    let mut accounts = Accounts::new();

    for spec in ACCOUNTS {
        let uid = format!("{SEED_PREFIX}{}", spec.suffix);
        let email = format!("{}@ddadan.test", uid.replace('-', "."));

        let credential = match emulator {
            Some(emulator) => Credential::Bearer(
                emulator
                    .ensure_user(&uid, &email, spec.display_name)
                    .await
                    .with_context(|| format!("{uid} 계정 준비"))?,
            ),
            None => Credential::DevUid(uid.clone()),
        };

        let mut account = Account {
            uid,
            email,
            display_name: spec.display_name.to_owned(),
            credential,
            user_id: None,
            roles: spec.roles.to_vec(),
        };

        // 201 이면 새로 만든 것, 200 이면 이미 있던 것. 둘 다 정상이다.
        let bootstrapped = api
            .post(
                "/users/bootstrap",
                &account,
                json!({ "display_name": spec.display_name }),
            )
            .await?;
        account.user_id = Some(bootstrapped.id("bootstrap")?);

        // 관리자 역할을 주는 API 는 없다 — 있으면 관리자를 스스로 임명할 수 있게 된다.
        // 최초 관리자는 이렇게 밖에서 들어온다(§3.3).
        for role in spec.roles {
            grant_role(pool, account.user_id.expect("bootstrapped"), role).await?;
        }

        accounts.insert(spec.key, account);
    }

    Ok(accounts)
}

async fn grant_role(pool: &PgPool, user_id: Uuid, role: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO coupon.user_roles (user_id, role)
         VALUES ($1, $2::text::coupon.account_role)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await
    .with_context(|| format!("{role} 역할 부여"))?;
    Ok(())
}

async fn lookup_user_id(pool: &PgPool, uid: &str) -> anyhow::Result<Option<Uuid>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM coupon.users WHERE firebase_uid = $1")
            .bind(uid)
            .fetch_optional(pool)
            .await?,
    )
}

#[derive(Debug)]
struct Store {
    id: Uuid,
    slug: String,
    status: String,
}

/// 초안 → 사업자 정보 → 검수 제출 → 관리자 승인. 전부 실제 엔드포인트다.
async fn ensure_active_store(api: &Api, accounts: &Accounts) -> anyhow::Result<Store> {
    let owner = account(accounts, "owner");
    let admin = account(accounts, "admin-operations");

    let existing = api.get("/owner/store", owner).await?;
    if !existing.status.is_success() {
        let created = api
            .post(
                "/owner/store",
                owner,
                json!({ "name": "따단 성수 카페", "slug": STORE_SLUG }),
            )
            .await?;
        created.ok("상점 초안 생성")?;
    }

    // 사업자 정보가 없으면 검수 제출이 STORE_NOT_READY_FOR_REVIEW 로 막힌다.
    api.patch(
        "/owner/store",
        owner,
        json!({
            "address": { "road": "서울 성동구 성수이로 1" },
            "business_profile": {
                "registration_no": "123-45-67890",
                "representative_name": "박점주",
            },
        }),
    )
    .await?
    .ok("사업자 정보 입력")?;

    let mut store = api.get("/owner/store", owner).await?;
    let mut status = store.ok("상점 조회")?["status"]
        .as_str()
        .unwrap_or_default()
        .to_owned();

    if status == "DRAFT" || status == "REJECTED" {
        let submitted = api
            .post(
                "/owner/store/submit-review",
                owner,
                json!({ "note": "시드 데이터 검수 요청" }),
            )
            .await?;
        submitted.ok("검수 제출")?;
        store = api.get("/owner/store", owner).await?;
        status = store.ok("상점 조회")?["status"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
    }

    if status == "PENDING_REVIEW" {
        let review_id = store.ok("상점 조회")?["latest_review"]["id"]
            .as_str()
            .ok_or_else(|| anyhow!("검수 대기 상태인데 검수 건 id 가 없습니다"))?
            .to_owned();

        api.post(
            &format!("/admin/store-reviews/{review_id}/decision"),
            admin,
            json!({
                "decision": "APPROVED",
                "public_reason": "승인되었습니다.",
                "reason": "시드 데이터: 서류 확인 완료",
            }),
        )
        .await?
        .ok("검수 승인")?;

        store = api.get("/owner/store", owner).await?;
        status = store.ok("상점 조회")?["status"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
    }

    let data = store.ok("상점 조회")?;
    if status != "ACTIVE" {
        bail!("상점이 ACTIVE 가 되지 않았습니다: {status}");
    }

    Ok(Store {
        id: Uuid::parse_str(data["id"].as_str().unwrap_or_default())?,
        slug: data["slug"].as_str().unwrap_or_default().to_owned(),
        status,
    })
}

const CATALOG_ITEMS: &[(&str, i64)] = &[
    ("아메리카노", 4_500),
    ("카페라떼", 5_000),
    ("소금빵", 3_800),
    ("바스크 치즈케이크", 6_500),
];

/// 품목은 이름으로 찾아 없는 것만 만든다.
async fn ensure_catalog(api: &Api, accounts: &Accounts) -> anyhow::Result<BTreeMap<String, Uuid>> {
    let owner = account(accounts, "owner");
    let listed = api.get("/owner/catalog/items?limit=100", owner).await?;
    let existing = listed.ok("품목 조회")?["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut items = BTreeMap::new();
    for item in &existing {
        if let (Some(name), Some(id)) = (item["name"].as_str(), item["id"].as_str()) {
            items.insert(name.to_owned(), Uuid::parse_str(id)?);
        }
    }

    for (name, price) in CATALOG_ITEMS {
        if items.contains_key(*name) {
            continue;
        }
        let created = api
            .post(
                "/owner/catalog/items",
                owner,
                json!({ "name": name, "reference_price": price }),
            )
            .await?;
        items.insert((*name).to_owned(), created.id("품목 생성")?);
    }

    Ok(items)
}

#[derive(Debug)]
struct Policy {
    id: Uuid,
    target: i64,
}

const POLICY_NAME: &str = "[시드] 10회 방문 도장";

/// 게시된 도장 정책. 이미 ACTIVE 인 같은 이름의 정책이 있으면 그대로 쓴다 — 새 버전을
/// 또 게시하면 실행할 때마다 정책 버전이 하나씩 늘어난다.
async fn ensure_policy(api: &Api, accounts: &Accounts) -> anyhow::Result<Policy> {
    let owner = account(accounts, "owner");
    let listed = api.get("/owner/loyalty-policies?limit=100", owner).await?;
    // 목록은 `policies`, 규칙은 펼쳐진 채로 온다. 여기서 키를 틀리면 조회가 조용히 빈
    // 결과가 되고, 실행할 때마다 정책 버전이 하나씩 늘어난다 — 멱등성이 깨지는 방식 중
    // 가장 알아채기 어려운 쪽이다.
    let policies = listed.ok("정책 조회")?["policies"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    if let Some(active) = policies
        .iter()
        .find(|policy| policy["name"] == POLICY_NAME && policy["status"] == "ACTIVE")
    {
        return Ok(Policy {
            id: Uuid::parse_str(active["id"].as_str().unwrap_or_default())?,
            target: active["target_stamp_count"].as_i64().unwrap_or(STAMP_GOAL),
        });
    }

    let draft = api
        .post(
            "/owner/loyalty-policies",
            owner,
            json!({
                "name": POLICY_NAME,
                "rules": {
                    "target_stamp_count": STAMP_GOAL,
                    "stamps_per_order": 1,
                    "minimum_order_amount": 0,
                    "daily_earning_limit": null,
                    // 같은 손님이 연속으로 적립할 때 STAMP-003 의 중복 경고가 뜬다.
                    // 시드는 주문번호를 매번 다르게 주고 명시적으로 확인한다.
                    "duplicate_warning_minutes": 1,
                    "stamp_validity_days": 180,
                    "eligible_item_ids": [],
                    "eligible_category_ids": [],
                    "excluded_item_ids": [],
                },
                "reward": {
                    "benefit_type": "FIXED_AMOUNT",
                    "fixed_amount": 3_000,
                    "free_item_ids": [],
                    "minimum_order_amount": 0,
                    "validity_days": 30,
                    "title": "3,000원 할인 쿠폰",
                    "description": "10회 방문 감사 쿠폰",
                    "customer_notice": "다른 할인과 중복 사용 불가",
                },
            }),
        )
        .await?;
    let policy_id = draft.id("정책 초안")?;

    api.post(
        &format!("/owner/loyalty-policies/{policy_id}/publish"),
        owner,
        json!({}),
    )
    .await?
    .ok("정책 게시")?;

    Ok(Policy {
        id: policy_id,
        target: STAMP_GOAL,
    })
}

/// 단골 소비자의 도장을 목표치까지 채운다. 이미 있는 만큼은 빼고 모자란 만큼만 적립한다 —
/// 그래야 두 번째 실행이 도장을 두 배로 만들지 않는다.
async fn ensure_stamps(api: &Api, accounts: &Accounts, target: i64) -> anyhow::Result<i64> {
    let owner = account(accounts, "owner");
    let veteran = account(accounts, "consumer-veteran");

    let held = current_stamp_balance(api, veteran).await?;
    if held >= target {
        return Ok(held);
    }

    for index in held..target {
        let qr = api.post("/me/qr-tokens", veteran, json!({})).await?;
        let token = qr.ok("QR 발급")?["token"]
            .as_str()
            .ok_or_else(|| anyhow!("QR 토큰이 없습니다"))?
            .to_owned();

        let accrued = api
            .post(
                "/owner/stamp-transactions",
                owner,
                json!({
                    "qr_token": token,
                    "order": {
                        // 매번 다른 주문번호. STAMP-003 은 이것이 있어야 중복 경고를
                        // 넘긴 적립을 별개 주문으로 인정한다.
                        "external_order_ref": format!("SEED-{}", index + 1),
                        "gross_amount": 12_000,
                        "currency": "KRW",
                        "items": [],
                    },
                    "acknowledge_duplicate": true,
                }),
            )
            .await?;
        accrued.ok("도장 적립")?;
    }

    current_stamp_balance(api, veteran).await
}

async fn current_stamp_balance(api: &Api, consumer: &Account) -> anyhow::Result<i64> {
    let wallet = api.get("/me/wallet/stamps", consumer).await?;
    let stamps = wallet.ok("도장 지갑 조회")?;
    // 응답의 키를 틀리면 잔액이 언제나 0 으로 읽히고, 실행할 때마다 도장을 목표치만큼
    // 더 적립한다 — 목표를 넘겨 리워드가 매번 새로 발급되고, 화면에 보여 주려던
    // "모으는 중" 상태가 사라진다. 그래서 값이 없으면 조용히 0 으로 넘기지 않는다.
    stamps["total_available"]
        .as_i64()
        .ok_or_else(|| anyhow!("도장 지갑 응답에 total_available 이 없습니다: {stamps}"))
}

#[derive(Debug, Clone)]
struct Campaign {
    key: &'static str,
    name: String,
    id: Uuid,
    issue_mode: &'static str,
    status: String,
}

/// 시드가 만드는 캠페인. 지갑의 세 가지 상태(사용 가능 / 만료 임박 / 사용 완료)는
/// 각각 자기 캠페인에서 나온다 — 한 캠페인에서 여러 장을 받으면 어떤 장이 어떤 상태인지
/// 사람이 화면에서 구별할 수 없다.
struct CampaignSpec {
    key: &'static str,
    name: &'static str,
    description: &'static str,
    issue_mode: &'static str,
    /// `None` 이면 수량 무제한(운영 상한만).
    quantity: Option<i64>,
    /// 발급된 쿠폰의 사용 만료까지 남는 일수.
    usable_days: i64,
}

const CAMPAIGNS: &[CampaignSpec] = &[
    CampaignSpec {
        key: "first-come",
        name: "[시드] 선착순 2,000원 할인",
        description: "선착순 50명, 2,000원 할인",
        issue_mode: "FIRST_COME",
        quantity: Some(50),
        usable_days: 60,
    },
    CampaignSpec {
        key: "expiring",
        name: "[시드] 곧 만료되는 1,000원 할인",
        description: "이틀 뒤 만료되는 1,000원 할인",
        issue_mode: "FIRST_COME",
        quantity: Some(20),
        usable_days: 2,
    },
    CampaignSpec {
        key: "used",
        name: "[시드] 사용 완료 데모 3,000원 할인",
        description: "사용 완료 상태를 보여 주기 위한 3,000원 할인",
        issue_mode: "FIRST_COME",
        quantity: Some(20),
        usable_days: 60,
    },
    CampaignSpec {
        key: "direct",
        name: "[시드] 단골 대상 직접 지급",
        description: "우리 가게 단골에게 드리는 1,500원 할인",
        issue_mode: "DIRECT",
        quantity: None,
        usable_days: 30,
    },
];

fn campaign_benefit(key: &str) -> i64 {
    match key {
        "expiring" => 1_000,
        "used" => 3_000,
        "direct" => 1_500,
        _ => 2_000,
    }
}

async fn ensure_campaigns(api: &Api, accounts: &Accounts) -> anyhow::Result<Vec<Campaign>> {
    let owner = account(accounts, "owner");
    let listed = api.get("/owner/campaigns?limit=100", owner).await?;
    let existing = listed.ok("캠페인 조회")?["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let now = Utc::now();
    let mut campaigns = Vec::new();

    for spec in CAMPAIGNS {
        if let Some(found) = existing.iter().find(|campaign| campaign["name"] == spec.name) {
            campaigns.push(Campaign {
                key: spec.key,
                name: spec.name.to_owned(),
                id: Uuid::parse_str(found["id"].as_str().unwrap_or_default())?,
                issue_mode: spec.issue_mode,
                status: found["status"].as_str().unwrap_or_default().to_owned(),
            });
            continue;
        }

        let total_quantity = match spec.quantity {
            Some(quantity) => json!({ "mode": "LIMITED", "quantity": quantity }),
            None => json!({ "mode": "UNLIMITED", "operational_cap": 500 }),
        };
        // 발급 창은 이미 열려 있고(진행 중), 쿠폰 만료는 그보다 뒤다.
        let issue_starts_at = now - Duration::hours(1);
        let issue_ends_at = now + Duration::days(spec.usable_days.clamp(1, 30));
        let usable_until = now + Duration::days(spec.usable_days);

        let draft = api
            .post(
                "/owner/campaigns",
                owner,
                json!({
                    "name": spec.name,
                    "customer_description": spec.description,
                    "benefit": {
                        "benefit_type": "FIXED_AMOUNT",
                        "fixed_amount": campaign_benefit(spec.key),
                    },
                    "minimum_order_amount": 0,
                    "issue_mode": spec.issue_mode,
                    "audience_type": "ALL_CUSTOMERS",
                    "total_quantity": total_quantity,
                    "per_user_quantity": 1,
                    "issue_starts_at": rfc3339(issue_starts_at),
                    "issue_ends_at": rfc3339(issue_ends_at.min(usable_until - Duration::hours(1))),
                    "usable_until": rfc3339(usable_until),
                }),
            )
            .await?;
        let campaign_id = draft.id("캠페인 초안")?;

        let published = api
            .post(
                &format!("/owner/campaigns/{campaign_id}/publish"),
                owner,
                json!({}),
            )
            .await?;
        let status = published.ok("캠페인 게시")?["status"]
            .as_str()
            .unwrap_or_default()
            .to_owned();

        campaigns.push(Campaign {
            key: spec.key,
            name: spec.name.to_owned(),
            id: campaign_id,
            issue_mode: spec.issue_mode,
            status,
        });
    }

    Ok(campaigns)
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Debug, Default)]
struct Wallet {
    available: usize,
    expiring: usize,
    used: usize,
    /// 직접 지급 캠페인이 실제로 쿠폰을 발급했는지. 캠페인 상태만으로는 알 수 없다 —
    /// 발급이 끝나도 진행 중인 캠페인은 `ISSUING` 으로 남는다.
    direct_issued: bool,
}

/// 지갑을 사용 가능 / 만료 임박 / 사용 완료 한 장씩으로 만든다.
async fn ensure_wallet(
    api: &Api,
    pool: &PgPool,
    accounts: &Accounts,
    campaigns: &[Campaign],
) -> anyhow::Result<Wallet> {
    let owner = account(accounts, "owner");
    let veteran = account(accounts, "consumer-veteran");

    for campaign in campaigns.iter().filter(|c| c.issue_mode == "FIRST_COME") {
        // 이미 한 장 받았으면 다시 받지 않는다. 서버의 1인당 수량은 *살아 있는* 쿠폰만
        // 세므로, 사용 완료 데모 쿠폰을 쓰고 나면 같은 캠페인에서 또 받아진다 — 실행할
        // 때마다 지갑에 한 장씩 쌓이는 방식으로 멱등성이 깨진다.
        let held: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM coupon.coupon_instances c
             JOIN coupon.users u ON u.id = c.user_id
             WHERE u.firebase_uid = $1 AND c.campaign_id = $2",
        )
        .bind(&veteran.uid)
        .bind(campaign.id)
        .fetch_one(pool)
        .await?;

        if held == 0 {
            claim(api, veteran, campaign).await?;
        }
    }

    // 사용 완료 한 장: 예약 → 승인. 이미 쓴 장이 있으면 다시 쓰지 않는다.
    if let Some(demo) = campaigns.iter().find(|campaign| campaign.key == "used") {
        let already_used: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM coupon.coupon_instances c
             JOIN coupon.users u ON u.id = c.user_id
             WHERE u.firebase_uid = $1 AND c.campaign_id = $2 AND c.status = 'USED'",
        )
        .bind(&veteran.uid)
        .bind(demo.id)
        .fetch_one(pool)
        .await?;

        if already_used == 0 {
            redeem_one(api, pool, owner, veteran, demo).await?;
        }
    }

    // DIRECT 캠페인은 워커가 발급한다. 워커가 돌고 있으면 곧 지갑에 들어오고, 아니면
    // 발급 대기로 남는다 — 어느 쪽인지는 보고 표에 그대로 찍는다.
    let mut wallet = summarise_wallet(api, veteran).await?;
    if let Some(direct) = campaigns.iter().find(|campaign| campaign.key == "direct") {
        let issued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM coupon.coupon_instances WHERE campaign_id = $1",
        )
        .bind(direct.id)
        .fetch_one(pool)
        .await?;
        wallet.direct_issued = issued > 0;
    }

    Ok(wallet)
}

async fn claim(api: &Api, consumer: &Account, campaign: &Campaign) -> anyhow::Result<()> {
    let claimed = api
        .post(
            &format!("/campaigns/{}/claims", campaign.id),
            consumer,
            json!({}),
        )
        .await?;

    if claimed.status.is_success() {
        return Ok(());
    }
    // 두 번째 실행에서 정상적으로 나오는 응답들. 1인 1장이 이미 채워졌다는 뜻이다.
    let benign = [
        "CAMPAIGN_ALREADY_CLAIMED",
        "PER_USER_LIMIT_REACHED",
        "CAMPAIGN_EXHAUSTED",
        "DUPLICATE_ISSUANCE",
    ];
    if benign.contains(&claimed.error_code()) {
        return Ok(());
    }
    bail!(
        "{} 받기 실패 ({}): {}",
        campaign.name,
        claimed.status,
        claimed.json
    );
}

async fn redeem_one(
    api: &Api,
    pool: &PgPool,
    owner: &Account,
    consumer: &Account,
    campaign: &Campaign,
) -> anyhow::Result<()> {
    // 지갑 API 는 쿠폰이 어느 캠페인에서 왔는지 알려 주지 않는다(소비자에게 필요한 정보가
    // 아니다). 시드는 "이 캠페인의 쿠폰"을 정확히 집어야 하므로 여기서만 DB 를 본다.
    let coupon_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT c.id FROM coupon.coupon_instances c
         JOIN coupon.users u ON u.id = c.user_id
         WHERE u.firebase_uid = $1 AND c.campaign_id = $2 AND c.status = 'AVAILABLE'
         ORDER BY c.created_at
         LIMIT 1",
    )
    .bind(&consumer.uid)
    .bind(campaign.id)
    .fetch_optional(pool)
    .await?;

    let Some(coupon_id) = coupon_id else {
        // 받기가 소진 등으로 막혔을 수 있다. 사용 완료 한 장이 없는 것은 시드 실패가 아니다.
        eprintln!(
            "주의: '{}' 쿠폰이 지갑에 없어 사용 완료 상태를 만들지 못했습니다.",
            campaign.name
        );
        return Ok(());
    };
    let coupon_id = coupon_id.to_string();

    let qr = api.post("/me/qr-tokens", consumer, json!({})).await?;
    let token = qr.ok("QR 발급")?["token"]
        .as_str()
        .ok_or_else(|| anyhow!("QR 토큰이 없습니다"))?
        .to_owned();

    let order = json!({
        "external_order_ref": format!("SEED-REDEEM-{}", Uuid::new_v4().simple()),
        "gross_amount": 12_000,
        "currency": "KRW",
        "items": [],
    });

    let reserved = api
        .post(
            "/owner/redemptions/preview",
            owner,
            json!({
                "qr_token": token,
                "coupon_id": coupon_id,
                "owner_session_id": "seed-till",
                "order": order,
            }),
        )
        .await?;
    let reservation_id = reserved.ok("사용 예약")?["reservation_id"]
        .as_str()
        .ok_or_else(|| anyhow!("예약 id 가 없습니다"))?
        .to_owned();

    api.post(
        &format!("/owner/redemptions/{reservation_id}/confirm"),
        owner,
        json!({ "owner_session_id": "seed-till", "order": order }),
    )
    .await?
    .ok("사용 승인")?;

    Ok(())
}

async fn summarise_wallet(api: &Api, consumer: &Account) -> anyhow::Result<Wallet> {
    let mut wallet = Wallet::default();
    let listed = api.get("/me/wallet/coupons?limit=100", consumer).await?;
    let coupons = listed.ok("지갑 조회")?["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let soon = Utc::now() + Duration::days(3);
    for coupon in &coupons {
        match coupon["effective_status"].as_str().unwrap_or_default() {
            "USED" => wallet.used += 1,
            "AVAILABLE" => {
                let expires = coupon["expires_at"]
                    .as_str()
                    .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| at.with_timezone(&Utc));
                if expires.is_some_and(|at| at <= soon) {
                    wallet.expiring += 1;
                } else {
                    wallet.available += 1;
                }
            }
            _ => {}
        }
    }

    Ok(wallet)
}

// ---------------------------------------------------------------------------
// 초기화
// ---------------------------------------------------------------------------

/// 시드가 만든 행을 지운다. 지우는 순서는 아래 목록의 순서가 아니라 **FK 가 정한다** —
/// 모든 외래키가 `ON DELETE RESTRICT` 라서, 순서를 하나만 틀려도 전체가 막힌다.
///
/// 그래서 순서를 손으로 완벽하게 맞추는 대신, 막힌 것은 남겨 두고 다음 바퀴에서 다시
/// 시도한다. 스키마가 자라도 이 함수는 계속 동작한다.
///
/// ## 추가 불가 로그
///
/// `audit_logs`·`stamp_ledger`·`coupon_status_events`·`consent_events`·
/// `user_session_revocations` 에는 DELETE 를 거부하는 트리거가 걸려 있다(§12.6, §13.5).
/// 그건 버그가 아니라 이 시스템의 핵심 불변식이고, 평상시에는 어떤 코드도 그걸 넘지
/// 못해야 한다. 그래서 `--reset` 은 그 트리거를 **세션 한정으로만** 끈다
/// (`session_replication_role = 'replica'`) — 연결이 끊기면 자동으로 원래대로 돌아가므로
/// 도중에 죽어도 가드가 꺼진 채 남지 않는다. 그래도 남의 DB 에서 벌일 일은 아니라서
/// [`ensure_reset_is_local`] 이 로컬이 아닌 대상은 거부한다.
async fn reset(pool: &PgPool) -> anyhow::Result<u64> {
    let users: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM coupon.users WHERE firebase_uid LIKE $1")
            .bind(format!("{SEED_PREFIX}%"))
            .fetch_all(pool)
            .await?;
    if users.is_empty() {
        return Ok(0);
    }

    let stores: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM coupon.stores WHERE owner_user_id = ANY($1)")
            .bind(&users)
            .fetch_all(pool)
            .await?;

    // outbox·job 처럼 FK 없이 id 만 들고 있는 테이블을 위해, 지우기 전에 관련 리소스 id 를
    // 모아 둔다. 지운 뒤에는 물어볼 곳이 없다.
    let mut resources: Vec<Uuid> = Vec::new();
    resources.extend(users.iter().copied());
    resources.extend(stores.iter().copied());
    for query in [
        "SELECT id FROM coupon.campaigns WHERE store_id = ANY($1)",
        "SELECT id FROM coupon.loyalty_policies WHERE store_id = ANY($1)",
        "SELECT id FROM coupon.coupon_instances WHERE store_id = ANY($1)",
        "SELECT id FROM coupon.stamp_transactions WHERE store_id = ANY($1)",
        "SELECT id FROM coupon.redemption_transactions WHERE store_id = ANY($1)",
    ] {
        let ids: Vec<Uuid> = sqlx::query_scalar(query)
            .bind(&stores)
            .fetch_all(pool)
            .await?;
        resources.extend(ids);
    }

    // 문장 하나하나가 같은 연결 위에서 돌아야 세션 설정이 적용된다.
    let mut connection = pool.acquire().await?;
    if let Err(error) = sqlx::query("SET session_replication_role = 'replica'")
        .execute(&mut *connection)
        .await
    {
        // 권한이 없으면 추가 불가 로그를 남긴 채 최선을 다한다. 무엇이 남았는지는
        // 아래 오류에 그대로 나온다.
        eprintln!("주의: 추가 불가 로그의 트리거를 끄지 못했습니다({error}). 일부가 남습니다.");
    }

    let mut pending: Vec<&(&str, &str)> = RESET_STATEMENTS.iter().collect();
    let mut deleted = 0_u64;
    let mut last_errors: BTreeMap<&str, String> = BTreeMap::new();

    for _ in 0..RESET_STATEMENTS.len() {
        let mut blocked = Vec::new();
        let mut progress = false;

        for statement in pending {
            match sqlx::query(statement.1)
                .bind(&users)
                .bind(&stores)
                .bind(&resources)
                .execute(&mut *connection)
                .await
            {
                Ok(result) => {
                    deleted += result.rows_affected();
                    last_errors.remove(statement.0);
                    progress = true;
                }
                // 대개는 "아직 자식이 남아 있다"는 뜻이다. 다음 바퀴에 다시 온다.
                Err(error) => {
                    last_errors.insert(statement.0, error.to_string());
                    blocked.push(statement);
                }
            }
        }

        if blocked.is_empty() {
            break;
        }
        if !progress {
            let detail = last_errors
                .iter()
                .map(|(table, error)| format!("  {table}: {error}"))
                .collect::<Vec<_>>()
                .join("\n");
            bail!("초기화가 더 진행되지 않습니다.\n{detail}");
        }
        pending = blocked;
    }

    let _ = sqlx::query("SET session_replication_role = 'origin'")
        .execute(&mut *connection)
        .await;

    Ok(deleted)
}

/// `--reset` 을 로컬 데이터베이스에서만 허용한다.
///
/// 이 도구는 추가 불가 로그의 트리거를 잠깐 끄고 데이터를 지운다. 로컬에서는 편의지만
/// 공용 환경에서는 사고다. 주소로 판단하는 것이 완벽하지는 않아도, 실수로 스테이징을
/// 가리킨 채 `--reset` 을 누르는 가장 흔한 사고는 여기서 멈춘다.
fn ensure_reset_is_local(database_url: &str) -> anyhow::Result<()> {
    if std::env::var("COUPON_SEED_ALLOW_RESET").is_ok_and(|value| value == "1") {
        return Ok(());
    }

    let host = database_url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.rsplit('@').next())
        .map(|hostport| hostport.split(':').next().unwrap_or_default())
        .unwrap_or_default()
        .to_owned();

    let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
        || host.starts_with("192.168.")
        || host.starts_with("10.")
        || host.starts_with("172.");

    if local {
        Ok(())
    } else {
        bail!(
            "--reset 은 로컬 데이터베이스에서만 씁니다 (지금 대상: {host}). \
             정말 필요하면 COUPON_SEED_ALLOW_RESET=1 을 붙이십시오."
        )
    }
}

/// `$1` 시드 사용자, `$2` 시드 상점, `$3` 그 밖의 시드 리소스 id.
const RESET_STATEMENTS: &[(&str, &str)] = &[
    (
        "job_attempts",
        "DELETE FROM coupon.job_attempts WHERE job_id IN
           (SELECT id FROM coupon.job_registry
            WHERE store_id = ANY($2) OR requested_by_user_id = ANY($1) OR resource_id = ANY($3))",
    ),
    (
        "notification_delivery_callbacks",
        "DELETE FROM coupon.notification_delivery_callbacks WHERE delivery_id IN
           (SELECT d.id FROM coupon.notification_deliveries d
            JOIN coupon.notifications n ON n.id = d.notification_id
            WHERE n.user_id = ANY($1) OR n.store_id = ANY($2))",
    ),
    (
        "notification_deliveries",
        "DELETE FROM coupon.notification_deliveries WHERE notification_id IN
           (SELECT id FROM coupon.notifications WHERE user_id = ANY($1) OR store_id = ANY($2))",
    ),
    (
        "notifications",
        "DELETE FROM coupon.notifications WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "coupon_status_events",
        "DELETE FROM coupon.coupon_status_events
         WHERE actor_user_id = ANY($1) OR coupon_id IN
           (SELECT id FROM coupon.coupon_instances WHERE user_id = ANY($1) OR store_id = ANY($2))",
    ),
    (
        "stamp_ledger",
        "DELETE FROM coupon.stamp_ledger
         WHERE actor_user_id = ANY($1) OR lot_id IN
           (SELECT id FROM coupon.stamp_lots WHERE user_id = ANY($1) OR store_id = ANY($2))",
    ),
    (
        "redemption_transactions",
        "DELETE FROM coupon.redemption_transactions WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "redemption_reservations",
        "DELETE FROM coupon.redemption_reservations
         WHERE user_id = ANY($1) OR owner_user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "issuance_deduplications",
        "DELETE FROM coupon.issuance_deduplications
         WHERE user_id = ANY($1) OR campaign_id = ANY($3)",
    ),
    (
        "campaign_audience_members",
        "DELETE FROM coupon.campaign_audience_members
         WHERE user_id = ANY($1) OR campaign_id = ANY($3)",
    ),
    (
        "coupon_instances",
        "DELETE FROM coupon.coupon_instances WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "stamp_lots",
        "DELETE FROM coupon.stamp_lots WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "stamp_transactions",
        "DELETE FROM coupon.stamp_transactions WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "qr_nonces",
        "DELETE FROM coupon.qr_nonces WHERE user_id = ANY($1)",
    ),
    (
        "campaign_counters",
        "DELETE FROM coupon.campaign_counters WHERE campaign_id = ANY($3)",
    ),
    (
        "campaigns",
        "DELETE FROM coupon.campaigns WHERE store_id = ANY($2)",
    ),
    (
        "loyalty_reward_definitions",
        "DELETE FROM coupon.loyalty_reward_definitions WHERE policy_id = ANY($3)",
    ),
    (
        "loyalty_policies",
        "DELETE FROM coupon.loyalty_policies WHERE store_id = ANY($2)",
    ),
    (
        "catalog_items",
        "DELETE FROM coupon.catalog_items WHERE store_id = ANY($2)",
    ),
    (
        "catalog_categories",
        "DELETE FROM coupon.catalog_categories WHERE store_id = ANY($2)",
    ),
    (
        "store_customers",
        "DELETE FROM coupon.store_customers WHERE store_id = ANY($2) OR user_id = ANY($1)",
    ),
    (
        "favorite_stores",
        "DELETE FROM coupon.favorite_stores WHERE store_id = ANY($2) OR user_id = ANY($1)",
    ),
    (
        "analytics_daily_store",
        "DELETE FROM coupon.analytics_daily_store WHERE store_id = ANY($2)",
    ),
    (
        "store_reviews",
        "DELETE FROM coupon.store_reviews WHERE store_id = ANY($2)",
    ),
    (
        "store_business_profiles",
        "DELETE FROM coupon.store_business_profiles WHERE store_id = ANY($2)",
    ),
    (
        "admin_case_notes",
        "DELETE FROM coupon.admin_case_notes
         WHERE author_user_id = ANY($1)
            OR case_id IN (SELECT id FROM coupon.admin_cases WHERE subject_user_id = ANY($1))",
    ),
    (
        "admin_adjustments",
        "DELETE FROM coupon.admin_adjustments
         WHERE requested_by_user_id = ANY($1)
            OR case_id IN (SELECT id FROM coupon.admin_cases WHERE subject_user_id = ANY($1))",
    ),
    (
        "admin_cases",
        "DELETE FROM coupon.admin_cases WHERE subject_user_id = ANY($1)",
    ),
    (
        "user_sanctions",
        "DELETE FROM coupon.user_sanctions
         WHERE subject_user_id = ANY($1) OR requested_by_user_id = ANY($1)",
    ),
    (
        "user_session_revocations",
        "DELETE FROM coupon.user_session_revocations
         WHERE subject_user_id = ANY($1) OR requested_by_user_id = ANY($1)",
    ),
    (
        "deletion_ledger",
        "DELETE FROM coupon.deletion_ledger WHERE subject_user_id = ANY($1)",
    ),
    (
        "push_subscriptions",
        "DELETE FROM coupon.push_subscriptions WHERE user_id = ANY($1)",
    ),
    (
        "notification_preferences",
        "DELETE FROM coupon.notification_preferences WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "consent_events",
        "DELETE FROM coupon.consent_events WHERE user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "auth_identities",
        "DELETE FROM coupon.auth_identities WHERE user_id = ANY($1)",
    ),
    (
        "idempotency_requests",
        "DELETE FROM coupon.idempotency_requests WHERE actor_user_id = ANY($1)",
    ),
    (
        "audit_logs",
        "DELETE FROM coupon.audit_logs WHERE actor_user_id = ANY($1) OR store_id = ANY($2)",
    ),
    (
        "outbox_events",
        "DELETE FROM coupon.outbox_events WHERE aggregate_id = ANY($3)",
    ),
    (
        "job_registry",
        "DELETE FROM coupon.job_registry
         WHERE store_id = ANY($2) OR requested_by_user_id = ANY($1) OR resource_id = ANY($3)",
    ),
    (
        "user_roles",
        "DELETE FROM coupon.user_roles WHERE user_id = ANY($1) OR granted_by_user_id = ANY($1)",
    ),
    (
        "stores",
        "DELETE FROM coupon.stores WHERE owner_user_id = ANY($1) OR id = ANY($2)",
    ),
    ("users", "DELETE FROM coupon.users WHERE id = ANY($1)"),
];

// ---------------------------------------------------------------------------
// 보고
// ---------------------------------------------------------------------------

/// 사람이 실기기 앞에서 그대로 쓰는 표. 이게 이 도구의 산출물이다.
fn print_report(
    options: &Options,
    emulator: Option<&Emulator>,
    accounts: &Accounts,
    store: &Store,
    policy: &Policy,
    campaigns: &[Campaign],
    wallet: &Wallet,
) {
    let mut out = String::new();

    let _ = writeln!(out, "\n=== 계정 ===");
    let rows: Vec<[String; 4]> = ACCOUNTS
        .iter()
        .map(|spec| {
            let account = account(accounts, spec.key);
            let roles = if account.roles.is_empty() {
                match spec.key {
                    "owner" => "STORE_OWNER (상점 생성으로 자동)".to_owned(),
                    _ => "CONSUMER".to_owned(),
                }
            } else {
                account.roles.join(", ")
            };
            [
                account.display_name.clone(),
                account.email.clone(),
                account.uid.clone(),
                roles,
            ]
        })
        .collect();
    let _ = write!(
        out,
        "{}",
        table(&["이름", "이메일", "Firebase UID", "역할"], &rows)
    );

    match emulator {
        Some(emulator) => {
            let _ = writeln!(
                out,
                "\n비밀번호는 전부 {SEED_PASSWORD} 입니다. 앱에서 위 이메일로 로그인하십시오."
            );
            let _ = writeln!(
                out,
                "앱 Firebase 설정: emulator {} / projectId {}",
                emulator.host, emulator.project_id
            );
        }
        None => {
            let _ = writeln!(
                out,
                "\ndev bypass 모드입니다. 위 UID 를 X-Dev-Firebase-Uid 헤더로 보내십시오."
            );
            let _ = writeln!(
                out,
                "실기기에서 실제 로그인을 하려면 COUPON_FIREBASE_AUTH_EMULATOR_HOST 를 설정하고 다시 도십시오."
            );
        }
    }

    let _ = writeln!(out, "\n=== 상점·정책 ===");
    let _ = write!(
        out,
        "{}",
        table(
            &["항목", "값"],
            &[
                ["상점 slug".to_owned(), store.slug.clone()],
                ["상점 id".to_owned(), store.id.to_string()],
                ["상점 상태".to_owned(), store.status.clone()],
                ["도장 정책 id".to_owned(), policy.id.to_string()],
                ["도장 목표".to_owned(), format!("{}회", policy.target)],
                [
                    "단골 보유 도장".to_owned(),
                    format!("{VETERAN_STAMPS}개 (목표 미만 — 리워드 아직 없음)"),
                ],
            ],
        )
    );

    let _ = writeln!(out, "\n=== 캠페인 ===");
    let campaign_rows: Vec<[String; 4]> = campaigns
        .iter()
        .map(|campaign| {
            [
                campaign.name.clone(),
                campaign.issue_mode.to_owned(),
                campaign.status.clone(),
                campaign.id.to_string(),
            ]
        })
        .collect();
    let _ = write!(
        out,
        "{}",
        table(&["이름", "발급 방식", "상태", "id"], &campaign_rows)
    );

    let _ = writeln!(out, "\n=== 단골 소비자 지갑 ===");
    let _ = write!(
        out,
        "{}",
        table(
            &["상태", "장수"],
            &[
                ["사용 가능".to_owned(), wallet.available.to_string()],
                ["만료 임박(3일 내)".to_owned(), wallet.expiring.to_string()],
                ["사용 완료".to_owned(), wallet.used.to_string()],
            ],
        )
    );

    if !wallet.direct_issued {
        let _ = writeln!(
            out,
            "\n주의: 직접 지급 캠페인이 아직 한 장도 발급하지 않았습니다. 발급은 coupon-worker 가\n\
             합니다 — 워커를 띄운 뒤 이 도구를 다시 도십시오(멱등합니다)."
        );
    }

    let _ = writeln!(out, "\nAPI: {}", options.api_url);
    print!("{out}");
}

/// 고정폭 표. 한글은 두 칸을 차지하므로 문자 수가 아니라 표시 폭으로 맞춘다.
fn table<const N: usize>(headers: &[&str; N], rows: &[[String; N]]) -> String {
    let mut widths = headers.map(display_width);
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(cell));
        }
    }

    let mut out = String::new();
    let line = |out: &mut String, cells: &[String; N]| {
        for (index, cell) in cells.iter().enumerate() {
            let padding = widths[index] - display_width(cell);
            let _ = write!(out, "  {cell}{}", " ".repeat(padding));
        }
        out.push('\n');
    };

    line(
        &mut out,
        &std::array::from_fn(|index| headers[index].to_owned()),
    );
    line(
        &mut out,
        &std::array::from_fn(|index| "─".repeat(widths[index])),
    );
    for row in rows {
        line(&mut out, row);
    }
    out
}

/// 터미널에서 차지하는 칸 수. 한글·이모지는 두 칸, 나머지는 한 칸으로 센다.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| match character {
            // 한글, 한자, 전각 기호, 이모지의 대략적인 범위.
            '\u{1100}'..='\u{115F}'
            | '\u{2E80}'..='\u{A4CF}'
            | '\u{AC00}'..='\u{D7A3}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FE30}'..='\u{FE6F}'
            | '\u{FF00}'..='\u{FF60}'
            | '\u{FFE0}'..='\u{FFE6}'
            | '\u{1F300}'..='\u{1FAFF}' => 2,
            _ => 1,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_seed_identity_is_inside_the_reset_scope() {
        // `--reset` 은 firebase_uid LIKE 'seed-%' 로 범위를 잡는다. 여기서 벗어난 계정이
        // 하나라도 생기면 초기화가 그 계정을 남기고, 다음 실행이 유령 상태를 만난다.
        for spec in ACCOUNTS {
            let uid = format!("{SEED_PREFIX}{}", spec.suffix);
            assert!(uid.starts_with(SEED_PREFIX), "{uid}");
        }
        assert!(STORE_SLUG.starts_with(SEED_PREFIX));
    }

    #[test]
    fn the_reset_plan_ends_at_users() {
        // 사용자 삭제가 마지막이어야 나머지 문장들이 사용자 id 로 범위를 잡을 수 있다.
        let last = RESET_STATEMENTS.last().expect("statements");
        assert_eq!(last.0, "users");
    }

    #[test]
    fn the_expiring_campaign_really_is_about_to_expire() {
        // 만료 임박 배지는 COUPON_COUPON_EXPIRING_LEAD_DAYS(기본 3일) 안쪽일 때만 뜬다.
        let expiring = CAMPAIGNS
            .iter()
            .find(|spec| spec.key == "expiring")
            .expect("만료 임박 캠페인");
        assert!(expiring.usable_days < 3, "{}", expiring.usable_days);

        let available = CAMPAIGNS
            .iter()
            .find(|spec| spec.key == "first-come")
            .expect("선착순 캠페인");
        assert!(available.usable_days > 3);
    }

    #[test]
    fn the_veteran_stays_below_the_reward_goal() {
        // 목표를 넘기면 리워드가 발급되어 "도장을 모으는 중" 화면을 볼 수 없게 된다.
        const { assert!(VETERAN_STAMPS < STAMP_GOAL) };
    }

    #[test]
    fn reset_refuses_a_database_that_is_not_ours_to_wipe() {
        // 추가 불가 로그의 트리거를 잠깐 끄는 동작이다. 원격 주소에서는 멈춰야 한다.
        assert!(
            ensure_reset_is_local("postgres://coupon:pw@db.staging.internal:5432/coupon").is_err()
        );
        assert!(ensure_reset_is_local("postgres://coupon:pw@localhost:55432/coupon").is_ok());
        assert!(ensure_reset_is_local("postgres://coupon:pw@127.0.0.1:55432/coupon").is_ok());
        assert!(ensure_reset_is_local("postgres://coupon:pw@192.168.150.185:55432/coupon").is_ok());
    }

    #[test]
    fn korean_columns_line_up() {
        let rendered = table(
            &["이름", "값"],
            &[["김단골".to_owned(), "seed".to_owned()]],
        );
        let widths: Vec<usize> = rendered
            .lines()
            .map(|line| display_width(line.split_whitespace().next().unwrap_or_default()))
            .collect();
        assert!(widths.iter().all(|width| *width > 0), "{rendered}");
    }
}
