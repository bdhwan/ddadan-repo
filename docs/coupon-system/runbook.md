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
sqlx migrate run

# 3) API
cargo run --bin coupon-api

# 4) Angular 앱 dev server
# coupon-consumer-app / coupon-store-app / coupon-system-admin-app
```

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
