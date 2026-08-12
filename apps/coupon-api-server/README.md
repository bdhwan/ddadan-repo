# coupon-api-server

DDADAN 상점별 쿠폰·도장 시스템의 Rust 서버. 기획서
[`docs/coupon-system/product-spec.md`](../../docs/coupon-system/product-spec.md) 의
Phase 1 서버측을 구현한다.

> 이 스택은 기존 디지털 사이니지 제품과 완전히 분리되어 있다. 포트·볼륨·compose 파일이
> 모두 별도이며, 루트 `docker-compose.yml` 은 건드리지 않는다.

## 빠른 시작

```bash
# 1) 개발용 PostgreSQL 16 / Redis 7 기동 (호스트 포트 55432 / 56379)
docker compose -f docker-compose.dev.yml up -d

# 2) 마이그레이션 적용 — 애플리케이션은 절대 스스로 스키마를 바꾸지 않는다 (§10.3)
export DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon
cargo sqlx migrate run          # cargo install sqlx-cli --no-default-features --features rustls,postgres

# 3) 서버 기동
cp .env.example .env
set -a && . ./.env && set +a
cargo run --bin coupon-api

# 4) 확인
curl -s localhost:7810/api/coupon/v1/health/live
curl -s localhost:7810/api/coupon/v1/health/ready
```

Swagger UI 는 <http://localhost:7810/api/coupon/v1/docs> 에서 볼 수 있다.

## 바이너리

| 바이너리 | 역할 |
|---|---|
| `coupon-api` | HTTP API. `--dump-openapi [경로]` 로 스펙만 출력하고 종료 |
| `coupon-worker` | 비동기 작업 실행기. **Phase 4 골격** — 기동·설정·로깅만 갖추고 큐는 아직 소비하지 않는다 |
| `coupon-seed` | 인수 시나리오(§20)·실기기 검증(§19.5)용 시드 데이터. 멱등하며 `--reset` 지원 |

## Firebase Auth emulator

기획서 §20.1 은 `local` 환경을 emulator 로 규정하고, §19.3 은 emulator 로 이메일·토큰
흐름을 확인하라고 요구한다.

```bash
./scripts/auth-emulator.sh up            # 0.0.0.0:9099 에 뜬다 (UI 4410)
export COUPON_FIREBASE_AUTH_EMULATOR_HOST=192.168.150.185:9099
export COUPON_FIREBASE_PROJECT_ID=ddadan-dev
cargo run --bin coupon-api
```

emulator 토큰은 `alg: none` 에 서명이 비어 있다는 점만 다르고 클레임은 실제 Firebase 와
같다. 그래서 서버는 **서명 단계만** 갈라지고 `iss`/`aud`/`exp`/`auth_time` 검사는 같은
코드를 탄다(`src/auth/firebase.rs`). production 에서 이 설정이 켜져 있으면
`COUPON_AUTH_DEV_BYPASS` 와 마찬가지로 **부팅을 거부**한다.

`COUPON_AUTH_DEV_BYPASS` 는 그대로 남아 있다. 둘은 별개 경로이고, 프로세스 안에서 도는
기존 통합 테스트는 계속 bypass 를 쓴다.

## 시드 데이터

승인된 ACTIVE 상점 하나를 얻으려면 계정 생성 → 상점 초안 → 사업자 정보 → 검수 제출 →
관리자 승인까지 서로 다른 권한으로 다섯 번을 호출해야 한다. `coupon-seed` 가 그걸
**실제 API 로** 해 준다(`UPDATE stores SET status='ACTIVE'` 같은 지름길을 쓰지 않는다).

```bash
export COUPON_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon
# emulator 를 쓰면 표에 찍히는 이메일/비밀번호로 실기기에서 그대로 로그인할 수 있다
export COUPON_FIREBASE_AUTH_EMULATOR_HOST=192.168.150.185:9099
cargo run --bin coupon-seed -- --api-url http://192.168.150.185:7810

cargo run --bin coupon-seed -- --reset      # 시드 데이터만 지우고 다시
```

몇 번을 돌려도 같은 상태로 수렴한다. 대상자 직접 지급 캠페인은 `coupon-worker` 가
발급하므로 워커가 떠 있어야 지갑까지 들어온다.

## 인수 시나리오 E2E

`tests/acceptance.rs` 는 `coupon-api` 와 `coupon-worker` 를 **실제 프로세스로** 띄우고
HTTP 로 붙는다. 프로세스 안에서 라우터를 부르는 다른 스위트가 지나가지 않는 층 — 부팅,
실제 토큰 검증, CORS/Origin, 미들웨어 순서, 별개 워커 — 이 여기서 확인된다.
emulator 나 `COUPON_TEST_DATABASE_URL` 이 없으면 눈에 보이게 건너뛴다.

## OpenAPI

세 Angular 앱은 이 파일에서 TypeScript DTO 를 생성한다. 엔드포인트나 DTO 를 바꿨으면
반드시 다시 뽑아 커밋할 것.

```bash
cargo run --bin coupon-api -- --dump-openapi     # -> openapi.json
```

DB 연결 없이 동작한다.

## Phase 1 엔드포인트

Base path 는 `/api/coupon/v1`, JSON 필드는 `snake_case`, ID 는 UUID 문자열,
시각은 RFC 3339 UTC, 금액은 원 단위 정수 + `currency: "KRW"` (§11.1).

| Method | Path | 인증 |
|---|---|---|
| `GET` | `/health/live` · `/health/ready` | 없음 |
| `POST` | `/users/bootstrap` | Firebase 토큰만 (내부 계정은 아직 없어도 됨) |
| `GET` `PATCH` | `/me` | 계정 필요 |
| `GET` | `/me/roles` | 계정 필요 |
| `GET` `POST` | `/me/consents` | 계정 필요 |
| `POST` `GET` `PATCH` | `/owner/store` | 계정 필요 |
| `POST` | `/owner/store/submit-review` | 계정 + **10분 이내 재인증** |

health 는 인프라 프로브 편의를 위해 base path 없이 `/health/live`, `/health/ready` 로도
응답한다.

### 응답 형식

성공:

```json
{ "data": { }, "request_id": "req_...", "transaction_id": "..." }
```

`transaction_id` 는 **변경 성공에만** 포함된다. 실패:

```json
{ "error": { "code": "STORE_NOT_FOUND", "message": "상점을 찾을 수 없습니다.",
             "field_errors": [], "retryable": false, "request_id": "req_..." } }
```

상태 매핑은 `src/error.rs` 의 `ErrorCode::status()` 가 유일한 기준이며 단위 테스트로
고정되어 있다: 400 형식·필드 / 401 인증 / 403 역할·상태·약관·출처 / 404 없음·소유권 없음 /
409 상태·버전·수량 경합 / 422 비즈니스 조건 / 429 속도제한 / 503 일시 장애.

### 변경 요청 규약

- **`Idempotency-Key` (UUID) 필수.** 같은 키 + 같은 body → 저장된 응답을 그대로 재생하고
  `Idempotent-Replay: true` 를 붙인다. 같은 키 + 다른 body → `409 IDEMPOTENCY_KEY_REUSED`
  (§12.6-9). 실패한 시도는 키를 반납하므로 body 를 고쳐 같은 키로 재시도할 수 있다.
  - 예외: `POST /users/bootstrap` 은 `idempotency_requests.actor_user_id` 가 `users` 를
    참조하는데 그 시점엔 회원이 없다. 헤더는 여전히 필수지만 기록은 남기지 않으며,
    `users.firebase_uid` unique 로 이미 멱등하다.
- **동시 수정**은 `If-Match: "3"` 또는 body 의 `version` 으로 검사한다. 둘 다 주면서 값이
  다르면 400. 진 쪽은 `409 VERSION_CONFLICT`.

## 인증

Firebase ID Token 을 `Authorization: Bearer <token>` 으로 보낸다. 서버는 Google
securetoken X.509 인증서를 캐시해 서명을 검증하고 `iss` / `aud` / `exp` / `auth_time` 을
확인한다. **역할·계정 상태·상점 상태는 토큰 claim 이 아니라 매 요청 DB에서 읽는다** (§9.3).

로컬 개발에서는 `COUPON_AUTH_DEV_BYPASS=1` 을 켜고 실제 토큰 대신 헤더를 쓸 수 있다:

```bash
curl -s localhost:7810/api/coupon/v1/me -H 'X-Dev-Firebase-Uid: dev-user-1'
# 재인증 가드 테스트용으로 sign-in 을 인위적으로 늙힐 수 있다
curl -s ... -H 'X-Dev-Auth-Age-Secs: 9000'
```

`COUPON_ENV=production` 에서 이 플래그가 켜져 있으면 **부팅 자체가 실패한다.**

카카오 OIDC 는 Phase 1 범위 밖이다 (`src/auth/kakao.rs` 에 인터페이스와 TODO 만 있다).

## 개발

```bash
cargo build --workspace
cargo test --workspace                     # DB 없이 도는 단위 테스트

# 통합 테스트는 마이그레이션된 DB 가 있을 때만 돈다 (없으면 조용히 skip)
COUPON_TEST_DATABASE_URL=$DATABASE_URL cargo test --workspace
```

### SQLx 오프라인 데이터

컴파일 타임 쿼리 검증 결과가 `.sqlx/` 에 있어 DB 없이도 빌드된다. 워크스페이스 루트
`.cargo/config.toml` 이 `SQLX_OFFLINE=true` 를 기본값으로 둔다.

**쿼리를 추가·수정했으면 반드시 다시 생성해 커밋할 것:**

```bash
DATABASE_URL=... cargo sqlx prepare -- --all-targets
```

라이브 DB 로 검증하며 작업하려면 `SQLX_OFFLINE=false` 를 export 한다.

## 모듈 구조

`src/` 는 기획서 §10.2 의 모듈 이름을 그대로 쓴다. 모듈끼리는 각 `mod.rs` 가 공개하는
서비스 구조체로만 호출하고, **한 모듈이 다른 모듈의 테이블을 직접 갱신하지 않는다.**
예: `consents` 는 `notification_preferences` 를 직접 쓰지 않고
`NotificationPreferenceService::apply` 를 호출하고, `stores` 는 `user_roles` 대신
`UserService::grant_role` 을 호출한다.

| 경로 | 상태 |
|---|---|
| `config` `telemetry` `error` `state` `db` `crypto` `openapi` | 구현 완료 |
| `http/{router, response, pagination, concurrency, health}` | 구현 완료 |
| `http/middleware/{request_id, auth, origin, idempotency}` | 구현 완료 |
| `auth` `users` `consents` `stores` | Phase 1 범위 구현 |
| `notifications` | 동의 → 발송 설정 투영만 구현, 나머지는 Phase 4 |
| `catalog` `loyalty` `campaigns` `wallet` `redemptions` `qr` `admin` `jobs` `audit` `analytics` | 자리만 확보 (TODO 주석에 해당 phase 와 불변식 기록) |

## 관측성

구조화 JSON 로그. 요청마다 `request_id`, 인증 후 `actor_id`, 상점 범위에 `store_id`,
변경에 `transaction_id` 가 span 필드로 붙는다 (§18.3). 로그에 남길 헤더는 **allowlist**
방식이라 `Authorization`, `Cookie`, QR·provider 헤더는 구조적으로 새어나갈 수 없다
(§16.3, `src/telemetry.rs`).

`/health/ready` 는 PostgreSQL 연결과 적용된 migration version, 필수 설정을 확인한다.
**Redis 장애는 readiness 실패가 아니라 `degraded`** 로만 보고한다 (§18.2).
