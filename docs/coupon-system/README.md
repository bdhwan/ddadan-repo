# 쿠폰 시스템 개발 셋업 가이드

DDADAN 상점별 쿠폰 발급 시스템의 로컬 개발 환경 셋업 방법을 설명한다.

## 문서 목록

| 문서 | 내용 |
|---|---|
| [product-spec.md](./product-spec.md) | 기획서 (상세 구현 기준안) |
| [scenarios.md](./scenarios.md) | 시나리오 명세 |
| [api-endpoints.md](./api-endpoints.md) | 엔드포인트 목록 (구현 진행 체크리스트) |
| [acceptance-checklist.md](./acceptance-checklist.md) | MVP 인수 시나리오 체크리스트 |

## 모노레포 구조

쿠폰 시스템은 같은 모노레포 안의 독립 애플리케이션·패키지로 추가한다. 기획서 §5.1 의 권장 구조는 다음과 같다.

```text
apps/
  coupon-api-server/          # Rust API와 worker 바이너리
  coupon-consumer-app/        # Angular 소비자 PWA
  coupon-store-app/           # Angular 점주 Web/PWA
  coupon-system-admin-app/    # Angular 운영 관리자 Web
libs/
  coupon-client-core/         # 인증, API transport, 오류, telemetry
  coupon-contracts/           # OpenAPI 생성 TypeScript DTO
  coupon-ui/                  # 디자인 토큰과 공통 컴포넌트
  coupon-domain/              # 화면용 순수 도메인 formatter/validator
docs/coupon-system/
```

- 기존 `apps/ddadan-admin-app`, `apps/ddadan-client-app`의 명칭과 책임을 재사용하지 않는다.
- 루트 npm workspace에 `libs/*`를 추가하고 앱은 Angular 21 standalone component를 사용한다.
- Rust workspace는 `apps/coupon-api-server/Cargo.toml`을 루트 Rust workspace가 참조하도록 구성한다.

## 포트 표

| 항목 | 값 |
|---|---|
| PostgreSQL | 호스트 포트 `55432`, DB `coupon`, user `coupon`, password `coupon` |
| Redis | 호스트 포트 `56379` |
| coupon API 서버 | `7810` |
| 소비자 앱 | `4310` |
| 점주 앱 | `4320` |
| 운영 관리자 앱 | `4330` |

compose 파일: `apps/coupon-api-server/docker-compose.dev.yml`

## 로컬 개발 시작 순서

1. Postgres/Redis 컨테이너 기동
   ```bash
   ./scripts/coupon/db-up.sh
   ```
2. DB 마이그레이션
   ```bash
   sqlx migrate run
   ```
   > `sqlx-cli` 는 `COUPON_` 접두사를 모른다. `DATABASE_URL` 을 따로 export 해야 한다.
3. 쿠폰 API 서버 실행
   ```bash
   cargo run --bin coupon-api
   ```
4. 각 Angular 앱 dev server 실행 (`coupon-consumer-app` / `coupon-store-app` / `coupon-system-admin-app`)

환경변수는 저장소 루트의 `.env.coupon.example` 를 참고해 `.env.coupon` 을 만들어 로드한다.

## 테스트 실행

DB 통합 테스트는 `COUPON_TEST_DATABASE_URL` 이 없으면 조용히 skip 되면서 통과처럼 보인다. 반드시 다음처럼 돌린다:

```sh
COUPON_TEST_DATABASE_URL=postgres://coupon:coupon_dev_password@localhost:55432/coupon cargo test --workspace
```

## 주의: 기존 사이니지 제품과 분리

쿠폰 시스템 개발 중 기존 사이니지 제품(`apps/ddadan-*`, 루트 `docker-compose.yml`, 포트 7800/4200/7300)은 **건드리지 않는다**.

- 루트 `docker-compose.yml` 의 사이니지 컨테이너는 그대로 둔다.
- 쿠폰용 컨테이너는 별도 compose 파일(`apps/coupon-api-server/docker-compose.dev.yml`)로 관리한다.
- 쿠폰 시스템의 포트는 사이니지 포트와 겹치지 않게 할당되어 있다.
