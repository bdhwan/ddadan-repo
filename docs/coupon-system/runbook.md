# 쿠폰 시스템 운영 런북

> 근거: [product-spec.md](./product-spec.md) §18(관측성·운영), §20(배포·마이그레이션·롤아웃).
> 기획서에 없는 절차는 적지 않는다. 근거가 없으면 `미정`이다.

## 헬스체크

기획서 §18.2:

| 경로 | 의미 |
|---|---|
| `/health/live` | 프로세스 event loop만 확인 |
| `/health/ready` | PostgreSQL 연결, migration version, 필수 설정 확인 |

- Redis 장애는 API 전체 readiness 실패가 아니라 기능별 degraded 상태로 노출할 수 있다.
- worker health는 마지막 heartbeat와 queue별 poll 시각으로 판정한다.

## SLO

기획서 §18.1:

| 항목 | 초기 SLO |
|---|---:|
| API 월 가용성 | 99.9% |
| 지갑 조회 p95 | 500ms 이하 |
| 적립/사용 승인 p95 | 800ms 이하, 외부 알림 제외 |
| 선착순 발급 p95 | 800ms 이하 |
| 직접 지급 지갑 반영 | 대상 확정 후 95% 60초 이내 |
| 만료 상태 반영 지연 | 5분 이하, 온라인 판정은 즉시 |
| 중복 논리 거래 | 0건 |

## 핵심 경보

기획서 §18.4:

- 중복 불변식 위반 또는 unique conflict 급증
- 적립/사용 오류율과 p95 임계 초과
- campaign backlog, notification backlog, outbox unpublished age
- dead-letter 신규 발생
- PostgreSQL connection/lock wait, replica lag(도입 시)
- Redis 연결 실패와 memory pressure
- Firebase token validation 오류 급증
- FCM/알림톡 provider 실패율·템플릿 거절
- 관리자 고위험 동작과 대량 조회

경보 임계값·알림 채널·on-call 라우팅: 미정

## 배포 순서

기획서 §20.2:

1. 확장 가능한 DB migration 적용
2. worker와 API를 이전/신규 schema 동시 호환 버전으로 배포
3. Angular 앱 배포
4. 새 기능 flag 활성화
5. 비호환 컬럼 정리는 최소 한 릴리스 뒤 별도 migration

부가 주의(§20.2):

- migration은 transaction 가능 여부, lock 시간, rollback/forward-fix를 사전 검토한다.
- 대규모 인덱스는 production에서 concurrent 생성한다.
- 캠페인 발급 중 worker 롤링 배포가 발생해도 job registry/checkpoint로 중복 없이 이어져야 한다.

## 백업·복구

기획서 §18.5:

- PostgreSQL PITR 가능한 지속 백업과 일일 복구 검증
- 알림·이미지 object storage의 versioning과 lifecycle
- Redis는 재생 가능한 전달 계층으로 취급하고 PostgreSQL outbox/job registry에서 복구
- RPO 5분 이내, RTO 60분을 초기 목표로 하며 실제 복구 훈련으로 검증

복원 후 순서(§18.5):

1. outbox 재발행
2. 만료 따라잡기
3. deletion ledger 재적용

구체 복원 명령·스토리지 대상·훈련 주기: 미정

## 로컬 개발 명령 모음

셋업 상세는 [README.md](./README.md)를 따른다.

```bash
# 1) Postgres/Redis
./scripts/coupon/db-up.sh

# 2) migration (`sqlx-cli` 는 COUPON_ 접두사를 모르므로 DATABASE_URL 을 export)
#    역할명 coupon 과 스키마가 겹치지 않게 search_path 를 고정한다.
cd apps/coupon-api-server
PGOPTIONS='-c search_path=public' sqlx migrate run
cd ../..

# 3) API
cargo run --bin coupon-api

# 4) Angular 앱 dev server
# coupon-consumer-app / coupon-store-app / coupon-system-admin-app
```

## 실기기 검증용 LAN·HTTPS 기동

사람이 휴대폰으로 수행하는 절차·표·실패 보고는 [device-test-guide.md](./device-test-guide.md) 가 원본이다. 여기서는 개발 머신에서 무엇을 띄울지만 적는다.

전제: 휴대폰과 개발 머신이 같은 Wi-Fi. 개발 머신 LAN IP 는 `192.168.150.185`.

1. 위 「로컬 개발 명령 모음」 1–3으로 DB·마이그레이션·API 를 올린다. API 는 `COUPON_BIND_ADDR=0.0.0.0:7810` 이라 LAN 에서 `http://192.168.150.185:7810/api/coupon/v1/health/live` 로 확인한다.
2. Angular 는 localhost 기본이라 휴대폰이 붙지 않는다. `--host 0.0.0.0` 을 붙인다.
   ```bash
   npm run start --workspace coupon-consumer-app -- --host 0.0.0.0   # :4310
   npm run start --workspace coupon-store-app -- --host 0.0.0.0      # :4320
   npm run start --workspace coupon-system-admin-app -- --host 0.0.0.0  # :4330, 필요할 때만
   ```
3. 접속 URL: 소비자 `http://192.168.150.185:4310` / 점주 `:4320` / 관리자 `:4330`. 앱 dev server 가 `/api/coupon/v1` 을 API 로 프록시한다.
4. HTTPS 기동(인증서 경로, 포트, `ng serve --ssl` 여부, 자체 서명 경고 통과): **미정**. 점주 카메라(`getUserMedia`)는 secure context 가 아니면 「HTTPS 연결이 필요함」을 보여 준다. 확정되면 [device-test-guide.md](./device-test-guide.md) §1.3·§1.5 와 이 절을 함께 고친다.
5. LAN Origin 을 `COUPON_ALLOWED_ORIGINS` 에 넣었는지는 **미정**(예제는 localhost HTTP 만). HTTPS origin 이 정해지면 `.env.coupon` 에 추가하고 API 를 재시작한다.
6. 시드(아래)로 ACTIVE 상점·계정을 만든 뒤 가이드 추적표를 수행한다.
7. Firebase Auth emulator 스크립트 `apps/coupon-api-server/scripts/auth-emulator.sh` 가 보이기 시작했다(다른 에이전트, 작성 시점 미커밋). `./apps/coupon-api-server/scripts/auth-emulator.sh up` → `http://192.168.150.185:9099`. 서버가 이 호스트를 읽는 설정 키는 **미정**(`config.rs`에 없음). 계정 생성은 이 스크립트 범위가 아니다.

## 시드 도구

실기기·운영 승인에 필요한 소비자/점주 계정, ACTIVE 상점, 활성 도장 정책 픽스처를 넣는 도구.

**미정** — 다른 작업에서 추가 중이다. 확정 전까지 계정·비밀번호를 추측해 적지 않는다.

채워질 때 적을 것:

| 칸 | 값 |
|---|---|
| 명령 (저장소 루트 기준) | 미정 |
| 표준출력에 나오는 계정 표 | 미정 — 출력을 [device-test-guide.md](./device-test-guide.md) §1.4 에 옮긴다 |
| 개발 DB(`coupon`)만 건드리는지 | 미정. 테스트 DB(`coupon_test`)를 지우면 안 된다 |
| 반복 실행 시 동작 (재실행·정리) | 미정 |

도구가 생기면 이 절에 명령과 예시 출력 위치를 넣고, 가이드의 `미정 — 시드 도구 출력 참조` 칸을 갱신한다.

## 테스트 DB

통합 테스트가 개발용 DB(`coupon`)를 그대로 쓰면 테스트 픽스처·불완전 payload 가 `job_registry` 등에 남아, 실제 개발 상태와 구분할 수 없다. 그래서 같은 Postgres 컨테이너 안에 **별도 데이터베이스** `coupon_test` 를 두고 테스트만 그쪽으로 보낸다. 컨테이너를 새로 띄우지 않는다.

```bash
# 테스트 DB 생성 + 마이그레이션 (개발용 DB 는 건드리지 않음)
./scripts/coupon/db-test-up.sh

# 테스트 DB 만 drop 후 재생성·마이그레이션 (개발용 DB 는 건드리지 않음)
./scripts/coupon/db-test-reset.sh

# COUPON_TEST_DATABASE_URL 을 coupon_test 로 설정한 뒤 cargo test --workspace
./scripts/coupon/test.sh
```

`COUPON_TEST_DATABASE_URL` 이 없으면 DB 통합 테스트가 조용히 skip 되어 통과처럼 보인다. `test.sh` 가 이 변수를 반드시 설정한다.

### 테스트 DB 누적 (검수 큐 limit=100)

같은 `coupon_test` DB로 통합 테스트를 반복하면 검수 큐에 `PENDING` 이 쌓인다. `the_review_queue_activates_a_store_on_approval` (`tests/operations.rs`) 는 `GET /admin/store-reviews?status=PENDING&limit=100` 으로 방금 만든 건을 찾는데, 누적 PENDING이 100건을 넘으면 첫 페이지 밖으로 밀려 실패한다(커밋 `7a1b682` 알려진 한계).

**언제 reset 하나**

- 위 테스트(또는 검수 큐를 limit=100 으로 훑는 테스트)가 간헐 실패할 때
- 테스트 DB를 오래 돌려 상태가 불명확할 때

```bash
./scripts/coupon/db-test-reset.sh
./scripts/coupon/test.sh
```

개발용 DB(`coupon`)는 건드리지 않는다. 이 절은 테스트가 페이지네이션/필터로 고쳐지기 전까지 유효하다.

## DLQ(dead-letter) 대응

기획서 §14.7: dead-letter 재처리는 원인 해결 확인, 관리자 사유, 새 generation 을 요구한다. §18.4 핵심 경보에 dead-letter 신규 발생이 포함된다.

확인 순서:

1. `GET /api/coupon/v1/admin/jobs` 로 dead-letter·실패 작업과 시도·체크포인트를 조회한다. 단건은 `GET /api/coupon/v1/admin/jobs/:id`.
2. payload·오류 원인·generation 을 확인하고, 잘못된 픽스처·스키마 불일치·워커 버그 등 **원인을 먼저 해결**한다.
3. 원인 해결이 확인되면 `POST /api/coupon/v1/admin/jobs/:id/retry` 로 관리자 사유를 포함해 재처리한다(새 generation).
4. 재처리 후에도 dead-letter 가 반복되면 §18.4 경보로 보고하고, 동일 원인 재시도만 반복하지 않는다.

로컬에서 테스트 잔여 행이 dead-letter 로 보이는 경우: 개발용 DB 에 테스트가 섞인 것일 수 있다. `./scripts/coupon/test.sh` 경로(테스트 DB)를 쓰고, 개발용 DB 는 수동 정리하거나 필요 시 `db-reset.sh`(전체 볼륨 삭제, 확인 입력 필요)를 검토한다.

## 기능 플래그

기획서 §20.4:

- 카카오 로그인
- FCM Web Push
- 카카오 알림톡
- 선착순 캠페인
- 대상자 대량 직접 지급
- 관리자 대량 회수

플래그 비활성화는 신규 진입만 막고 이미 생성된 도메인 상태를 손상하지 않는다.

플래그 저장소·토글 절차·환경별 기본값: 미정
