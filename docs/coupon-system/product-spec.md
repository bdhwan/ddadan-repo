# DDADAN 상점별 쿠폰 발급 시스템 상세 기획서

> 문서 상태: 구현 기준안  
> 대상 출시: 대한민국 운영 가능 MVP  
> 최종 갱신: 2026-08-10  
> 관련 문서: [전체 시나리오 명세](./scenarios.md)

## 1. 제품 정의

DDADAN 쿠폰은 개인 카페, 베이커리, 미용실 같은 단일 소규모 상점이 별도 POS 구축 없이 고객 방문 도장과 할인 쿠폰을 운영하는 웹 서비스다. 소비자는 하나의 계정과 안전한 고유 식별 수단으로 여러 상점의 도장과 쿠폰을 한 지갑에서 관리한다.

제품의 핵심 가치는 다음과 같다.

- 종이 쿠폰을 잃어버리거나 상점마다 별도 앱을 설치하는 불편 제거
- 소규모 상점도 수량·기간·품목 조건을 가진 실용적인 프로모션 운영
- 현장에서 점주가 소비자의 회전형 QR을 스캔하여 빠르게 적립·사용 승인
- 발급·적립·사용·취소를 불변 원장으로 기록하여 중복·부정 사용과 민원 대응
- 앱 내 알림, Web Push, 카카오 알림톡을 통한 실시간 혜택 안내

## 2. 목표, 비목표, 성공지표

### 2.1 MVP 목표

1. 점주가 가입부터 상점 승인, 도장 정책 게시까지 도움 없이 완료한다.
2. 현장 적립·사용의 95%가 QR 화면 진입 후 30초 안에 완료된다.
3. 동시 요청이나 재시도로 쿠폰·도장·수량이 중복 반영되지 않는다.
4. 소비자가 여러 상점의 혜택과 만료 조건을 오해 없이 확인한다.
5. 운영자가 사용자 문의의 전체 사건 이력을 거래 ID 하나로 추적한다.

### 2.2 MVP 비목표

- 결제 승인, 매출 정산, 전자영수증 발급
- POS 거래 진위의 자동 검증
- 현금성 포인트와 선불충전금
- 다지점·프랜차이즈 통합 캠페인
- 직원 초대와 세부 근무 권한
- 쿠폰 거래·선물·양도
- 인터넷이 끊긴 상태의 적립·사용
- 서비스 이용료 결제와 상점 구독 관리

### 2.3 출시 후 핵심 지표

| 분류 | 지표 | 정의 |
|---|---|---|
| 활성화 | 상점 활성화율 | 승인 상점 중 7일 내 도장 정책을 게시한 비율 |
| 소비자 | 첫 적립 완료율 | 가입 후 14일 내 첫 도장을 받은 회원 비율 |
| 반복사용 | 30일 재방문율 | 첫 적립 후 30일 내 같은 상점에서 재적립한 비율 |
| 혜택 | 리워드 사용률 | 발급된 도장 리워드 중 유효기간 내 사용된 비율 |
| 캠페인 | 할인 쿠폰 사용률 | 정상 발급된 할인 쿠폰 중 사용된 비율 |
| 현장성능 | 스캔 처리시간 | QR 해석부터 승인 응답까지 p50/p95 |
| 신뢰성 | 중복 반영 건수 | 동일 논리 거래가 둘 이상 원장에 반영된 건수, 목표 0 |
| 운영 | 민원 해결시간 | 생성부터 종결까지 중앙값 및 p90 |
| 메시지 | 채널 전달률 | 채널별 성공/영구실패/재시도 비율 |

지표는 상점 간 비교를 유도하는 순위가 아니라 자기 상점의 추세를 보여주는 데 사용한다.

## 3. 사용자와 운영 모델

### 3.1 소비자

- 기술 숙련도가 낮아도 모바일 브라우저에서 사용할 수 있어야 한다.
- 같은 회원이 점주이면서 다른 상점의 소비자가 될 수 있다.
- 소비자의 이메일·전화번호·카카오 식별자는 상점에 공개하지 않는다.
- 여러 상점의 보유 혜택을 서비스 단위 지갑에서 통합 관리한다.

### 3.2 점주

- MVP에서 회원 1명은 상점 1개만 소유할 수 있다.
- 직원 계정이 없으므로 적립·사용 승인과 캠페인 관리는 모두 점주가 수행한다.
- 모바일에서는 스캐너와 현장 승인을, 데스크톱에서는 정책·캠페인·통계를 주로 사용한다.
- 점주도 소비자 프로필을 갖지만 자기 상점에서의 거래는 위험 분석에 표시한다.

### 3.3 시스템 관리자

| 역할 | 조회 | 변경 |
|---|---|---|
| `SUPPORT` | 회원·상점·거래·민원 제한 조회 | 민원 메모, 고객 안내 |
| `OPERATIONS` | 캠페인·큐·알림·통계 | 상점 검수, 작업 재처리, 제한적 보정 요청 |
| `SECURITY` | 위험 신호·감사·인증 사건 | 세션 폐기, 임시 정지, 조사 보존 |
| `SUPER_ADMIN` | 전체 | 영구 제재, 원장 보정 승인, 권한 관리 |

고위험 작업은 최근 MFA 재인증, 사유, 사건 티켓을 요구한다. 요청자와 승인자가 달라야 하는 작업은 원장 대량 보정, 이미 발급된 쿠폰 대량 회수, 영구 제재다.

## 4. 제품 원칙

1. **서버 판정 우선**: 클라이언트 시간·표시 잔액·역할을 신뢰하지 않는다.
2. **원장 우선**: 잔액이나 상태를 직접 덮어써 과거를 없애지 않는다.
3. **한 번만 반영**: 모든 변경 API와 비동기 작업은 멱등해야 한다.
4. **고객 권리 보존**: 정책 변경은 이미 발급된 쿠폰·도장에 불리하게 소급하지 않는다.
5. **최소 개인정보**: 상점은 고객 관리에 필요한 가명 정보와 자기 상점의 거래만 본다.
6. **명확한 현장 결과**: 성공·실패·불확실 상태를 색상만이 아니라 문구·거래 ID·시각으로 표시한다.
7. **메시지는 부가 채널**: 외부 알림 실패가 지갑의 혜택을 취소하지 않는다.

## 5. 시스템 범위와 구성

기존 NestJS/Angular 사이니지 제품은 변경하지 않는다. 쿠폰 제품은 같은 모노레포 안의 독립 애플리케이션·패키지로 추가한다.

```text
[Consumer Angular PWA] ─┐
[Store Angular Web/PWA] ├── HTTPS ── [Rust Axum API] ── [PostgreSQL]
[System Admin Angular] ─┘                  │     │
                                          │     └── [Redis / Apalis workers]
                                          ├── Firebase Auth / FCM
                                          └── Kakao OIDC / Alimtalk provider
```

### 5.1 권장 모노레포 구조

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

## 6. 정보 구조와 화면 명세

### 6.1 공통 인증 화면

| 경로 | 화면 | 필수 기능 |
|---|---|---|
| `/login` | 로그인 | 이메일/비밀번호, 카카오 로그인, 비밀번호 찾기, 오류 일반화 |
| `/signup` | 이메일 가입 | 이메일, 비밀번호, 이름, 약관, 인증 메일 안내 |
| `/auth/kakao/callback` | 카카오 콜백 | 진행 상태, 재시도, 안전한 오류 복귀 |
| `/verify-email` | 이메일 인증 | 재발송 제한, 인증 완료 재조회 |
| `/terms` | 약관 동의 | 필수·선택 분리, 버전·전문 링크 |
| `/account/security` | 보안 | 연결 로그인 수단, 세션 폐기, 비밀번호 변경 |
| `/account/notifications` | 알림 설정 | 목적·상점·채널별 동의와 권한 상태 |
| `/account/withdraw` | 탈퇴 | 영향 요약, 재인증, 완료/보존 안내 |

인증 오류는 `이메일 미가입`과 `비밀번호 불일치`를 구분하지 않는다. 카카오 오류는 취소, 일시 장애, 보안 검증 실패만 사용자 행동 가능성에 맞춰 구분한다.

### 6.2 소비자 웹앱

#### 하단 내비게이션

- 홈
- 지갑
- 내 QR
- 알림
- 내 정보

#### 홈 `/`

- 만료 7일 이내 혜택, 목표까지 1~2개 남은 도장판, 새로 받은 쿠폰을 우선 노출한다.
- 관심 상점의 공개 캠페인을 보여주되 마케팅 동의가 없으면 개인화 푸시를 보내지 않는다.
- 빈 상태에는 공개 상점/매장 QR 이용 방법을 안내한다.

#### 지갑 `/wallet`

- 탭: `사용 가능`, `도장`, `사용·만료 내역`
- 필터: 상점, 혜택 유형, 7일 이내 만료
- 정렬: 만료 임박 기본, 최근 발급, 상점명
- 카드에는 상태 배지, 혜택, 상점, 최소 주문액, 핵심 품목 제한, 만료를 표시한다.
- 상세에서 전체 사용 조건, 발급 사유, 발급/사용/만료 시각, 문의용 식별번호를 제공한다.
- 만료·회수 내역을 숨기지 않고 사유를 확인할 수 있게 한다.

#### 내 QR `/my-qr`

- QR과 8자리 보조 코드, 60초 남은 시간, 자동 갱신 상태를 표시한다.
- 용도는 적립과 사용 식별이며 결제 QR이 아님을 명시한다.
- 오프라인, 토큰 만료, 약관 미동의, 계정 정지 시 QR 대신 해결 행동을 표시한다.
- 화면 밝기 증가와 wake lock은 브라우저 지원 범위에서만 사용하고 사용 후 원복한다.

#### 상점 상세 `/stores/:slug`

- 상점 소개, 위치, 영업시간, 활성 도장 정책, 공개 선착순 캠페인, 관심 등록을 제공한다.
- 비활성·정지·폐점 상태에 맞는 고지를 보여준다.
- 현재 휴무 여부는 안내값이며 쿠폰 사용 가능성은 개별 조건을 우선한다.

#### 알림 `/notifications`

- 거래, 혜택, 보안, 운영 공지 유형을 구분한다.
- 읽음 처리는 낙관적으로 표시하되 실패 시 재동기화한다.
- 알림을 삭제해도 거래·쿠폰은 삭제되지 않는다.

### 6.3 점주 웹앱

#### 주요 내비게이션

- 오늘
- 스캔
- 도장 정책
- 할인 캠페인
- 고객
- 통계
- 상점 설정

#### 온보딩 `/onboarding/store`

- 단계: 기본 정보 → 사업자 정보 → 영업 설정 → 약관 → 검수 제출
- 각 단계 임시 저장과 이탈 경고를 제공한다.
- 사업자번호, 대표자명 등 민감 정보는 저장 후 마스킹한다.
- 검수 상태와 보완 사유를 타임라인으로 보여준다.

#### 오늘 `/dashboard`

- 오늘 적립·사용·취소 수, 활성 캠페인, 큐/발송 이상 여부를 요약한다.
- 빠른 동작: QR 스캔, 캠페인 일시 중지, 정책 확인
- 통계가 아직 집계되지 않았으면 0으로 오인하지 않도록 `집계 중`을 표시한다.

#### 스캔 `/scan`

상태 머신은 다음과 같다.

`READY → SCANNING → CUSTOMER_RESOLVED → INPUT → REVIEW → SUBMITTING → SUCCESS|FAILURE`

- 첫 진입에서 카메라 권한과 HTTPS 여부를 검사한다.
- QR을 읽으면 중복 프레임 처리를 중지하고 서버 검증 결과를 기다린다.
- 거래 종류를 `도장 적립` 또는 `쿠폰 사용`으로 고른다.
- 주문 금액, 외부 주문번호, 품목을 입력한다.
- 승인 전 소비자 마스킹 정보, 예상 도장/할인, 만료와 제한을 다시 표시한다.
- 성공 화면은 결과, 거래 ID, 처리 시각을 보여주고 명시적으로 `다음 고객`을 누르기 전 자동 초기화하지 않는다.
- 응답 유실 시 새 요청을 만들지 않고 기존 멱등키로 `처리 결과 확인`을 수행한다.

#### 도장 정책 `/loyalty`

- 현재 활성 버전, 다음 예약 버전, 과거 버전을 구분한다.
- 목표 수, 적립 조건, 도장 만료, 리워드 내용을 단계별 편집한다.
- 활성 정책 수정은 새 버전 생성으로 안내한다.
- 게시 전 소비자에게 보일 예시 도장판과 만료 예를 미리 보여준다.

#### 캠페인 `/campaigns`

- 상태, 발급 방식, 발급/사용 기간, 발급/사용 수량을 목록에서 확인한다.
- 작성 단계: 혜택 → 사용 조건 → 대상 → 수량 → 일정 → 알림 → 검토
- 검토 화면에서 최대 할인 노출액, 예상 대상, 예상 알림량, 이미 발급 후 변경 불가 항목을 표시한다.
- 발급 중 진행률은 대상 스냅샷 확정 수와 처리 수로 구분한다.
- 중지·취소·회수는 영향 요약과 확인 문구 입력을 요구한다.

#### 고객 `/customers`

- 자기 상점의 가명 고객 ID, 별명, 첫/최근 방문일, 도장 수, 보유 쿠폰 수만 조회한다.
- 이메일·전화번호·카카오 계정·다른 상점 정보는 제공하지 않는다.
- 세그먼트는 `관심`, `최근 방문`, `도장 N개 이상`, `장기 미방문`으로 제공한다.
- 고객별 수동 쿠폰 지급은 캠페인 `특정 고객` 발급으로 기록한다.

#### 통계 `/analytics`

- 기간별 적립, 리워드, 캠페인, 취소·보정 지표
- 실시간 잠정치와 일 배치 확정치를 구분
- CSV 개인 목록 내보내기는 MVP에서 제외
- 집단 크기가 개인정보 최소 기준 미만이면 세부 구분을 숨김

### 6.4 시스템 관리자 웹앱

| 영역 | 핵심 화면 |
|---|---|
| 운영 현황 | API/DB/Redis/worker/알림 상태, backlog, 오류율 |
| 상점 검수 | 신청 목록, 증빙, 중복 신호, 승인·보완·거절 |
| 회원·상점 | 상태, 역할, 제재, 세션 폐기, 관련 사건 |
| 거래 탐색 | 적립·사용·취소·보정 원장과 상태 타임라인 |
| 캠페인 | 진행 상태, 대상/발급 수, 긴급 중단·회수 |
| 작업 큐 | 작업 키, 시도, 체크포인트, 오류, 재처리 |
| 알림 | 템플릿 버전, 발송·콜백, 영구 실패 |
| 민원 | 분류, 증거, 당사자 메시지, 해결·승인 |
| 감사 | 관리자 조회·변경 로그, 필터, 보존 잠금 |

관리자 목록은 URL에 필터·페이지를 보존하고, 민감정보 원문은 기본 마스킹한다. 고위험 동작은 일반적인 확인 모달이 아니라 작업 영향과 되돌림 가능성을 명시한 재인증 화면을 사용한다.

## 7. 반응형·PWA·접근성 요구사항

- 소비자 앱: 360px 폭 이상 모바일 우선, 데스크톱에서도 최대 콘텐츠 폭 제한
- 점주 앱: 360px 이상 스캔 화면, 768px 이상 관리 화면 2열, 1280px 이상 데이터 테이블
- 시스템 관리자: 1024px 이상 지원, 모바일에서는 읽기 전용 경고
- 소비자와 점주 앱은 PWA manifest와 service worker를 제공한다.
- 캐시는 정적 자산과 읽기 전용 공개 데이터에 한정하고 QR·변경 API 응답은 캐시하지 않는다.
- 카메라는 HTTPS secure context에서 `getUserMedia`로 접근하고 후면 카메라를 우선한다.
- WCAG 2.2 AA를 목표로 키보드 탐색, 포커스, 4.5:1 대비, 44px 터치 영역, 스크린리더 레이블을 제공한다.
- 성공/실패는 색상만으로 구분하지 않고 아이콘, 제목, 설명, 거래 ID를 함께 제공한다.
- 날짜는 `2026. 8. 10. 오후 3:00`, 금액은 `12,000원`처럼 지역화한다.

## 8. 쿠폰·도장 상세 정책

### 8.1 도장 정책

| 정책 | 기본값 | 허용 범위/설명 |
|---|---:|---|
| 목표 도장 | 10 | 2~100 |
| 주문당 적립 | 1 | 1~10, 초기 UI는 1 고정 추천 |
| 최소 주문액 | 0원 | 원 단위 정수 |
| 영업일당 횟수 | 1 | 1~20 또는 무제한 |
| 도장 유효기간 | 180일 | 1~730일 |
| 리워드 유효기간 | 30일 | 1~365일 |
| 중복 경고 구간 | 5분 | 1~60분 |
| 활성 정책 수 | 1 | 상점당 기본 정책 최대 1개 |

- 개별 도장은 `earned_at`과 `expires_at`을 가진다.
- 목표 달성 시 먼저 만료되는 가용 도장부터 목표 수만큼 소비한다.
- 도장 소비와 리워드 발급은 하나의 논리 거래로 묶는다.
- 정책 새 버전은 미래 적립에만 적용한다.
- 도장판에는 `N/10`, 가장 이른 도장 만료, 리워드 내용을 보여준다.

### 8.2 할인 유형

#### 정액 할인

- 필수: 할인액
- 선택: 최소 주문액, 품목 제한
- 실제 할인액은 대상 주문액을 넘지 않는다.

#### 정률 할인

- 필수: 1~100% 할인율, 최대 할인액
- 대상 금액 × 할인율에서 1원 미만을 버리고 최대 할인액을 적용한다.

#### 무료 품목

- 필수: 하나 이상의 품목 SKU 또는 상점 내부 품목 ID
- 여러 대상 품목을 주문했다면 기본적으로 가장 낮은 단가 1개를 무료 처리한다.
- 수량 2개 이상 구매 조건은 후속 기능이며 MVP에서는 최소 주문액으로만 보완한다.

### 8.3 품목 관리

- 점주는 품목명, 내부 SKU(선택), 카테고리, 활성 상태를 관리한다.
- 가격은 쿠폰 예상 할인 계산을 위해 거래 시 입력하며 상품 마스터 가격은 참고값이다.
- 비활성 품목은 새 정책에서 선택할 수 없지만 기존 쿠폰 조건 스냅샷에는 남는다.
- 대상과 제외가 겹치면 제외가 우선한다.
- 주문 품목을 입력하지 않은 경우 품목 제한 쿠폰을 승인할 수 없다.

### 8.4 발급 수량

- `total_quantity`: 전체 발급 가능 수량
- `per_user_quantity`: 회원별 누적 발급 가능 수량
- `per_business_day_quantity`: 상점 영업일별 발급 가능 수량
- `issued_count`는 `PENDING`, `AVAILABLE`, `RESERVED`, `USED`, `EXPIRED`를 포함하고 `ISSUE_FAILED`는 제외한다.
- 회수 쿠폰이 수량을 복원하는지는 캠페인에 고정된 `restore_quantity_on_revoke` 값으로 결정하며 기본값은 `false`다.
- 총수량 무제한은 운영상 상한을 가진 별도 표현이며 DB 정수 최대값을 사용하지 않는다.

### 8.5 기간과 상태 변경

- 발급 기간과 사용 기간을 분리한다.
- 절대 사용 종료와 `발급 후 N일`을 함께 설정하면 더 이른 시각을 적용한다.
- 발급된 쿠폰에는 계산 완료된 `usable_from`, `expires_at`과 조건 스냅샷을 저장한다.
- 캠페인 수정이 인스턴스 조건을 소급 변경하지 않는다.
- 사용 종료 시각은 미포함이므로 정확히 그 시각부터 사용 불가다.

### 8.6 중복·취소

- MVP는 주문당 혜택 1개만 사용한다.
- 적립과 쿠폰 사용은 같은 주문에서 가능하되 점주가 각각 승인하며 동일 `external_order_ref`로 연결할 수 있다.
- 적립 취소 24시간, 사용 취소 10분을 점주 셀프서비스 한도로 둔다.
- 한도 이후 처리는 관리자 민원·보정 흐름을 사용한다.

## 9. 인증·계정 설계

### 9.1 Firebase Auth 책임

- 이메일/비밀번호 생성·검증, 이메일 인증, 비밀번호 재설정
- Firebase ID Token과 refresh session
- Web Push용 FCM 토큰과 프로젝트 연결
- 계정 비활성화·토큰 폐기

Firebase UID는 외부 인증 식별자이며 도메인 FK로 직접 사용하지 않는다. 회원마다 canonical `users.firebase_uid` 하나를 두고 내부 `users.id` UUID와 매핑한다. 이메일 가입자는 Firebase가 만든 UID를 쓰고, 카카오로 처음 가입한 회원은 서버가 충돌 불가능한 UID를 만든다.

### 9.2 카카오 로그인

1. 브라우저가 Rust API의 `/v1/auth/kakao/authorize`를 호출한다.
2. 서버가 PKCE verifier를 암호화된 HttpOnly SameSite=Lax 임시 쿠키 또는 단기 서버 저장소에 보관하고 authorize URL을 반환한다.
3. 콜백에서 code, state, nonce를 검증한다.
4. 카카오 OIDC discovery/JWKS를 캐시하되 `kid` 미일치 시 한 번 갱신한다.
5. `iss=https://kauth.kakao.com`, audience, nonce, exp를 검증한다.
6. `(provider, provider_subject)`로 내부 사용자를 조회하거나 canonical Firebase UID를 가진 새 내부 사용자를 생성한다.
7. 기존 계정에 카카오를 연결한 경우를 포함하여 항상 해당 회원의 canonical Firebase UID로, 서비스 계정 RS256 키를 사용해 최대 1시간 유효한 Firebase Custom Token을 만든다.
8. Angular가 `signInWithCustomToken`으로 Firebase 세션을 얻는다.

- 카카오 access/refresh token은 로그인 완료 후 폐기한다. 카카오 사용자 API를 지속 호출해야 하는 기능을 MVP에 두지 않는다.
- 이메일은 매핑 힌트일 뿐 계정 자동 병합 키가 아니다.
- 연결 해제·계정 상태 웹훅은 서명을 검증하고 멱등 처리한다.

### 9.3 API 인증

- 모든 비공개 API는 Firebase ID Token을 검증한다.
- JWKS 공개키 캐시, issuer, audience, exp, auth_time을 확인한다.
- 고위험 API는 `auth_time`이 10분 이내이거나 별도 MFA 확인 토큰이 필요하다.
- 사용자·역할·상점 상태는 매 요청 DB에서 확인한다.
- 관리자 앱은 별도 Firebase tenant 또는 최소한 별도 audience/allowlist와 MFA를 사용한다.

### 9.4 약관과 동의

| 동의 | 필수 | 철회 효과 |
|---|---|---|
| 서비스 이용약관 | 예 | 서비스 이용 중단/탈퇴 안내 |
| 개인정보 수집·이용 | 예 | 서비스 이용 중단/탈퇴 안내 |
| 위치 기반 공개 상점 검색 | 아니오 | 위치 기반 정렬 비활성화 |
| 서비스 거래 Web Push | 아니오 | 앱 내 알림만 유지 |
| 카카오 정보성 알림 | 정책별 | 앱 내 알림만 유지 |
| 전체 마케팅 | 아니오 | 프로모션 외부 발송 중단 |
| 상점별 마케팅 | 아니오 | 해당 상점 대상 발송 제외 |

동의는 문서 버전, 동의/철회, 시각, IP 해시, user agent 분류, 수집 화면을 기록한다.

## 10. Rust 서버 아키텍처

### 10.1 기술 기준

- Rust stable, edition 2024
- Axum: HTTP routing와 middleware
- Tokio: async runtime
- SQLx: compile-time checked query와 PostgreSQL transaction
- PostgreSQL: 모든 도메인 상태와 불변 원장, outbox, job registry
- Redis: Apalis job transport, rate limiting, 짧은 캐시
- Apalis: 비동기 worker 실행
- Serde, validator: DTO 역직렬화와 입력 검증
- tracing + OpenTelemetry: 구조화 로그·trace·metric
- utoipa 또는 동등 도구: OpenAPI 생성

### 10.2 모듈

| 모듈 | 책임 |
|---|---|
| `auth` | Firebase 검증, Kakao OIDC, custom token, webhook |
| `users` | 프로필, 역할, 계정 상태, 탈퇴 |
| `consents` | 약관·채널·상점별 동의 |
| `stores` | 상점, 검수, 영업 설정, 소유권 |
| `catalog` | 품목·카테고리 |
| `loyalty` | 도장 정책 버전, 적립 원장, 목표 달성 |
| `campaigns` | 캠페인, 대상 스냅샷, 수량 |
| `wallet` | 쿠폰 인스턴스, 소비자 조회 |
| `redemptions` | 예약·사용·취소, 할인 계산 |
| `qr` | nonce, 서명, 소비 처리 |
| `notifications` | 앱 내 알림, FCM, 알림톡, 동의 판정 |
| `admin` | 검수, 제재, 보정, 민원 |
| `jobs` | registry, enqueue, worker, checkpoint, DLQ |
| `audit` | 관리자·고위험 사용자 행위 감사 |
| `analytics` | 집계와 개인정보 보호 임계값 |

모듈 간 호출은 서비스 인터페이스와 도메인 이벤트로 제한한다. 한 모듈이 다른 모듈의 테이블을 임의 갱신하지 않는다.

### 10.3 실행 바이너리

- `coupon-api`: HTTP API, health/readiness, outbox 기록
- `coupon-worker`: 캠페인·알림·만료·집계 작업
- `coupon-scheduler`: 주기 작업을 결정적 키로 등록. 초기에는 worker 프로세스 내 단일 스케줄러 모드로 둘 수 있음
- 마이그레이션은 SQLx migration으로 별도 실행하며 애플리케이션 자동 schema sync를 금지한다.

## 11. API 규약

### 11.1 공통

- Base path: `/api/coupon/v1`
- JSON 필드: `snake_case`
- 식별자: UUID 문자열
- 시각: RFC 3339 UTC, 예: `2026-08-10T06:00:00Z`
- 금액: 원 단위 정수와 `currency="KRW"`
- 목록: cursor pagination, 기본 20개, 최대 100개
- 변경 요청: `Idempotency-Key` UUID 필수
- 동시 수정: `version` 또는 `If-Match`로 optimistic concurrency
- 응답에 `request_id`, 변경 성공에는 `transaction_id`를 포함한다.

#### 오류 형식

```json
{
  "error": {
    "code": "COUPON_NOT_AVAILABLE",
    "message": "현재 사용할 수 없는 쿠폰입니다.",
    "field_errors": [],
    "retryable": false,
    "request_id": "req_..."
  }
}
```

HTTP 상태 기준:

- `400`: 형식·필드 검증
- `401`: 인증 없음/만료
- `403`: 역할, 상태, 약관, 정책상 금지
- `404`: 없거나 소유권 없는 리소스
- `409`: 상태·버전·수량 경합
- `422`: 형식은 맞지만 비즈니스 조건 불충족
- `429`: 속도 제한
- `503`: 일시 장애, 안전한 재시도 가능 여부 포함

### 11.2 인증·회원 API

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/auth/kakao/authorize` | state/PKCE를 만들고 authorize URL 반환 |
| `GET` | `/auth/kakao/callback` | OIDC 검증 후 일회용 교환 코드 발급 |
| `POST` | `/auth/kakao/exchange` | 교환 코드로 Firebase Custom Token 반환 |
| `POST` | `/webhooks/kakao/unlink` | 카카오 연결 해제 멱등 처리 |
| `POST` | `/users/bootstrap` | Firebase 가입 후 내부 회원/프로필 생성 |
| `GET/PATCH` | `/me` | 내 프로필 조회·수정 |
| `GET` | `/me/roles` | 활성 역할과 상점 참조 |
| `POST/DELETE` | `/me/auth-links/kakao` | 로그인 수단 연결·해제 |
| `GET/POST` | `/me/consents` | 동의 조회·변경 |
| `POST` | `/me/withdrawal` | 재인증 후 탈퇴 요청 |

### 11.3 소비자 API

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/public/stores` | 공개 상점 검색 |
| `GET` | `/public/stores/{slug}` | 상점·공개 정책·캠페인 |
| `PUT/DELETE` | `/me/favorite-stores/{store_id}` | 관심 등록/해제 |
| `GET` | `/me/wallet/coupons` | 쿠폰 목록과 상태 필터 |
| `GET` | `/me/wallet/coupons/{coupon_id}` | 조건 스냅샷 포함 상세 |
| `GET` | `/me/wallet/stamps` | 상점별 가용·만료 예정 도장 |
| `POST` | `/me/qr-tokens` | 60초 회전형 QR와 보조 코드 발급 |
| `POST` | `/campaigns/{campaign_id}/claims` | 선착순 쿠폰 받기 |
| `GET/PATCH` | `/me/notifications` | 앱 내 알림 조회·읽음 |
| `POST/DELETE` | `/me/push-subscriptions` | FCM Web Push 토큰 등록·해제 |

선착순 받기 응답은 성공 시 쿠폰 ID를, 중복 요청이면 기존 쿠폰 ID를, 소진이면 `CAMPAIGN_SOLD_OUT`을 반환한다.

### 11.4 점주 API

| Method | Path | 설명 |
|---|---|---|
| `POST` | `/owner/store` | 상점 초안 생성, 계정당 1개 |
| `GET/PATCH` | `/owner/store` | 자기 상점 조회·수정 |
| `POST` | `/owner/store/submit-review` | 검수 제출 |
| `GET/POST/PATCH` | `/owner/catalog/items` | 품목 관리 |
| `GET/POST` | `/owner/loyalty-policies` | 정책 버전 목록·초안 생성 |
| `PATCH` | `/owner/loyalty-policies/{policy_id}` | 초안 수정 |
| `POST` | `/owner/loyalty-policies/{policy_id}/publish` | 즉시/예약 게시 |
| `POST` | `/owner/scan/resolve` | QR 검증과 가명 고객 조회 |
| `POST` | `/owner/stamp-transactions/preview` | 적립 조건·예상 결과 검증 |
| `POST` | `/owner/stamp-transactions` | 최종 적립 승인 |
| `POST` | `/owner/stamp-transactions/{transaction_id}/void` | 24시간 내 취소 |
| `GET/POST` | `/owner/campaigns` | 캠페인 목록·초안 생성 |
| `PATCH` | `/owner/campaigns/{campaign_id}` | 초안/허용 필드 수정 |
| `POST` | `/owner/campaigns/{campaign_id}/publish` | 캠페인 게시·발급 작업 등록 |
| `POST` | `/owner/campaigns/{campaign_id}/pause` | 신규 발급 중지 |
| `POST` | `/owner/campaigns/{campaign_id}/resume` | 안전 재개 |
| `POST` | `/owner/campaigns/{campaign_id}/cancel` | 취소와 회수 정책 지정 |
| `POST` | `/owner/redemptions/preview` | 조건 검증과 2분 예약 생성 |
| `POST` | `/owner/redemptions/{reservation_id}/confirm` | 사용 최종 승인 |
| `POST` | `/owner/redemptions/{reservation_id}/cancel` | 예약 또는 10분 내 사용 취소 |
| `GET` | `/owner/customers` | 가명 고객과 자기 상점 지표 |
| `GET` | `/owner/analytics` | 잠정/확정 통계 |

#### 적립 승인 요청 예시

```json
{
  "qr_token": "opaque-signed-token",
  "preview_id": "fc733982-d11d-4f5f-b814-722075c8b7c2",
  "order": {
    "external_order_ref": "POS-OPTIONAL-20260810-42",
    "gross_amount": 12000,
    "currency": "KRW",
    "items": [
      { "catalog_item_id": "...", "name_snapshot": "아메리카노", "quantity": 2, "unit_price": 6000 }
    ]
  }
}
```

preview는 표시 편의를 위한 것이며 confirm에서 모든 조건을 다시 검증한다.

### 11.5 시스템 관리자 API

| Method | Path | 설명 |
|---|---|---|
| `GET` | `/admin/store-reviews` | 검수 큐 |
| `POST` | `/admin/store-reviews/{review_id}/decision` | 승인·보완·거절 |
| `GET` | `/admin/users/{user_id}` | 마스킹된 회원·사건 조회 |
| `POST` | `/admin/users/{user_id}/suspend` | 임시/영구 제재 요청 |
| `POST` | `/admin/users/{user_id}/revoke-sessions` | Firebase 세션 폐기 |
| `GET` | `/admin/transactions/{transaction_id}` | 연결 원장과 감사 타임라인 |
| `POST` | `/admin/adjustments/preview` | 보정 결과 시뮬레이션 |
| `POST` | `/admin/adjustments` | 승인된 보정 사건 생성 |
| `POST` | `/admin/campaigns/{campaign_id}/emergency-stop` | 긴급 중단 |
| `POST` | `/admin/campaigns/{campaign_id}/revoke-job` | 대량 회수 작업 |
| `GET/POST` | `/admin/cases` | 민원·보안 사건 관리 |
| `GET` | `/admin/jobs` | 작업·시도·체크포인트 |
| `POST` | `/admin/jobs/{job_id}/retry` | 사유 포함 재처리 |
| `GET` | `/admin/audit-logs` | 감사 검색 |

### 11.6 실시간 갱신

- MVP 기본은 REST와 짧은 polling이다.
- 소비자 지갑·알림은 앱 활성 상태에서 30초, 점주 발급 진행은 5초 polling한다.
- 브라우저 visibility가 hidden이면 polling을 중단하거나 최대 5분으로 낮춘다.
- 향후 SSE를 추가할 수 있도록 각 리소스에 증가하는 `version`과 `updated_at`을 제공한다.
- 실시간이라는 표현은 발급 DB 커밋 후 소비자 지갑 조회에 수 초 내 반영되는 것을 뜻하며 외부 메시지 즉시 도착을 보장하지 않는다.

## 12. PostgreSQL 데이터 모델

### 12.1 공통 규칙

- PK는 PostgreSQL 16에서 기본 제공되는 `gen_random_uuid()` UUID를 사용한다. UUIDv7 도입은 PostgreSQL/라이브러리 지원과 인덱스 효과를 검증한 후 별도 마이그레이션으로 진행한다.
- 모든 테이블에 필요한 경우 `created_at`, `updated_at`, `version`을 둔다.
- 업무 데이터는 물리 삭제 대신 상태와 삭제 시각을 사용한다.
- 원장·감사 테이블은 애플리케이션 UPDATE/DELETE 권한을 제거한다.
- 금액은 `BIGINT`, 비율은 basis point 또는 검증된 정수 percent로 저장한다.
- 조건 스냅샷은 정규화 FK와 변경 불가능한 JSONB를 함께 사용하되 JSON schema version을 저장한다.

### 12.2 회원·상점

| 테이블 | 핵심 컬럼 | 제약/인덱스 |
|---|---|---|
| `users` | id, firebase_uid, status, display_name, primary_email_enc, consumer_key | firebase_uid unique, consumer_key unique |
| `auth_identities` | user_id, provider, provider_subject, status | `(provider, provider_subject)` unique |
| `user_roles` | user_id, role, status | 활성 `(user_id, role)` unique |
| `terms_documents` | type, version, content_hash, effective_at | `(type, version)` unique |
| `consent_events` | user_id, scope, action, document_version, occurred_at | append-only |
| `notification_preferences` | user_id, store_id nullable, purpose, channel, enabled | 범위별 unique |
| `stores` | owner_user_id, status, name, slug, timezone, business_day_cutoff | owner_user_id unique, slug unique |
| `store_business_profiles` | store_id, registration_no_enc/hash, representative_enc, review_status | registration hash index |
| `store_reviews` | store_id, submission_snapshot, status, reviewer_id, reason | 상태·제출시각 index |
| `store_customers` | store_id, user_id, alias, first_seen_at, last_seen_at | `(store_id,user_id)` unique |
| `favorite_stores` | store_id, user_id, status | 활성 unique |

### 12.3 카탈로그·도장

| 테이블 | 핵심 컬럼 | 설명 |
|---|---|---|
| `catalog_categories` | store_id, name, status | 상점 내부 분류 |
| `catalog_items` | store_id, category_id, sku, name, status | 품목 제한 판정 |
| `loyalty_policies` | store_id, version_no, status, schedule, rule_snapshot | 활성 버전 직접 수정 금지 |
| `loyalty_reward_definitions` | policy_id, benefit_type, benefit_snapshot, validity | 목표 리워드 정의 |
| `stamp_lots` | store_id, user_id, policy_id, earned_at, expires_at, original_quantity | 적립 묶음, 잔량은 원장 파생 |
| `stamp_ledger` | transaction_id, lot_id, event_type, quantity_delta, reason | append-only, 합계 불변식 |
| `stamp_transactions` | store_id, user_id, policy_id, business_day, order_snapshot, status | 멱등키·외부 주문 참조 |

도장 가용 수는 `stamp_ledger.quantity_delta` 합으로 계산하며 읽기 성능을 위해 projection을 둘 수 있다. projection은 원장에서 재구축 가능해야 한다.

### 12.4 캠페인·쿠폰·사용

| 테이블 | 핵심 컬럼 | 제약/인덱스 |
|---|---|---|
| `campaigns` | store_id, version, status, issue_mode, audience_snapshot, quantity limits, schedules | `(store_id,id,version)` |
| `campaign_audience_members` | campaign_id, user_id, snapshot_reason, status | `(campaign_id,user_id)` unique |
| `campaign_counters` | campaign_id, business_day, reserved, issued | 행 잠금/조건부 UPDATE |
| `coupon_instances` | store_id, campaign_id nullable, reward_definition_id nullable, user_id, status, usable_from, expires_at, condition_snapshot | 발급 source 둘 중 하나 필수 |
| `coupon_status_events` | coupon_id, event_type, from_status, to_status, actor, reason | append-only |
| `redemption_reservations` | coupon_id, store_id, user_id, owner_session_id, expires_at, status, order_snapshot | 쿠폰당 활성 예약 unique |
| `redemption_transactions` | coupon_id, reservation_id, order_snapshot, discount_amount, status | 쿠폰당 성공 사용 unique |
| `issuance_deduplications` | campaign_id, user_id, ordinal | `(campaign_id,user_id,ordinal)` unique |

`coupon_instances.condition_snapshot`에는 발급 시점의 혜택, 최소 금액, 품목, 중복, 요일·시간, 문구와 schema version을 저장한다.

### 12.5 QR·알림·운영

| 테이블 | 핵심 컬럼 | 설명 |
|---|---|---|
| `qr_nonces` | nonce_hash, user_id, audience, expires_at, consumed_at, transaction_id | 원문 nonce 저장 금지 |
| `notifications` | user_id, event_id, type, title, body, read_at | 앱 내 알림 원본 |
| `notification_deliveries` | notification_id, channel, template_version, status, attempt_count, provider_ref | 발송 고유키 |
| `push_subscriptions` | user_id, token_enc/hash, browser, status, last_seen_at | 사용자·토큰 hash unique |
| `outbox_events` | aggregate, event_type, payload, published_at | 도메인 커밋과 같은 트랜잭션 |
| `job_registry` | unique_key, job_type, status, generation, checkpoint, attempts | 활성 unique key 제약 |
| `admin_cases` | type, status, subject refs, assignee, resolution | 민원·보안 사건 |
| `admin_adjustments` | case_id, preview_snapshot, approval, execution_job | 이중 승인 가능 |
| `audit_logs` | actor, action, resource, before_hash, after_hash, reason, request_id | append-only·변조 탐지 |

### 12.6 주요 불변식

1. `stores.owner_user_id`는 활성 상점 기준 유일하다.
2. 상점당 활성 도장 정책은 최대 1개다.
3. 도장 lot의 누적 소비량은 원 적립량을 초과할 수 없다.
4. 캠페인 발급 수량은 총수량과 일일수량을 넘을 수 없다.
5. 소비자별 인스턴스 수는 개인 한도를 넘을 수 없다.
6. 쿠폰당 활성 예약은 최대 1개, 성공 사용은 최대 1개다.
7. QR nonce는 성공 거래에 최대 1회 연결된다.
8. 상태 이벤트의 이전 상태는 당시 인스턴스 상태와 같아야 한다.
9. 동일 멱등키와 actor의 요청 본문 hash가 다르면 재사용 오류다.
10. 동일 job unique key의 `QUEUED/RUNNING/RETRY_WAIT` 작업은 최대 1개다.

## 13. 핵심 트랜잭션 설계

### 13.1 도장 적립

1. 멱등 요청 행을 삽입하거나 기존 결과를 조회한다.
2. QR nonce를 `SELECT ... FOR UPDATE`로 잠그고 유효성을 확인한다.
3. 상점, 사용자, 활성 정책, 영업일 사용량을 잠근다.
4. 금액·품목·일일 한도를 검증한다.
5. `stamp_transactions`, `stamp_lots`, `stamp_ledger(EARN)`을 생성한다.
6. 가용 lot을 만료순으로 확인해 목표 달성 횟수를 계산한다.
7. 각 목표에 `CONSUME_FOR_REWARD`를 기록하고 같은 트랜잭션에서 `coupon_instances(AVAILABLE)`를 만든다. 쿠폰 생성 실패 시 적립 전체를 롤백한다.
8. nonce를 소비하고 outbox·감사 사건을 기록한다.
9. 결과를 멱등 응답에 저장하고 커밋한다.

모든 테이블 잠금 순서는 `store → policy/campaign → user/store_customer → coupon/stamp lot → nonce`로 고정해 교착을 줄인다.

### 13.2 선착순 발급

1. 멱등 요청과 캠페인 상태·기간·대상을 확인한다.
2. `(campaign_id, business_day)` counter를 잠근다.
3. 소비자의 기존 발급 ordinal을 확인한다.
4. 총·일일·개인 한도를 모두 통과한 경우 dedup 행과 쿠폰을 생성한다.
5. counter를 증가시키고 outbox를 기록한다.
6. 고유 제약 충돌은 기존 발급 결과 또는 소진 결과로 정상 변환한다.

카운터 캐시나 Redis 값은 수량 판정의 source of truth로 사용하지 않는다.

### 13.3 쿠폰 사용 예약·승인

예약:

1. 쿠폰 행 잠금
2. 소유·상점·`AVAILABLE`·기간·조건 확인
3. 2분 만료 예약 생성, 쿠폰을 `RESERVED`로 전환
4. 예상 할인액과 예약 ID 반환

승인:

1. 예약과 쿠폰을 같은 순서로 잠금
2. 예약 소유 점주 세션, 미만료, 주문 hash를 검증
3. 조건을 재계산
4. 사용 원장과 `USED` 상태 사건을 기록
5. 앱 내 알림 outbox 생성 후 커밋

### 13.4 취소·보정

- 원 거래를 잠그고 취소 가능 기간과 연결 리워드/쿠폰 상태를 검사한다.
- 취소 행과 반대 원장 사건을 추가한다.
- 자동 복원이 안전하지 않으면 `REQUIRES_ADMIN_REVIEW`로 끝내고 민원 사건을 만든다.
- 관리자 보정 preview는 repeatable-read 스냅샷과 만료 시각을 가지며 실행 시 version이 달라졌으면 다시 preview한다.

## 14. 비동기 큐 설계

### 14.1 목적

BullMQ의 job, retry, delayed job, worker concurrency와 유사한 운영 모델을 Rust에서 제공하되, "같은 논리 작업은 전체 클러스터에서 동시에 하나만 실행"하도록 한다.

### 14.2 구성

- Apalis + Redis: 전달, 예약 시각, retry transport
- PostgreSQL `job_registry`: 작업 생명주기, 결정적 중복 방지, 체크포인트, 운영 조회
- PostgreSQL advisory lock: 여러 worker 프로세스 간 동일 key 동시 실행 차단
- PostgreSQL outbox: API 커밋과 enqueue 사이 유실 방지

Redis 단일 lock만으로 도메인 정합성을 보장하지 않는다. lock TTL 만료나 네트워크 분할이 있어도 DB 고유 제약과 멱등 처리로 중복 반영되지 않아야 한다.

### 14.3 작업 키

```text
{job_type}:{tenant_or_store_id}:{resource_id}:{operation_version}
```

예:

- `issue_campaign:store-uuid:campaign-uuid:3`
- `expire_coupons:global:2026-08-10T06:00Z:v1`
- `notify_event:user-uuid:event-uuid:fcm-template-2`
- `revoke_campaign:store-uuid:campaign-uuid:case-uuid`

### 14.4 작업 상태

`PENDING_OUTBOX → QUEUED → RUNNING → SUCCEEDED`

실패 분기:

- `RUNNING → RETRY_WAIT → QUEUED`
- `RUNNING → DEAD_LETTER`
- 운영 중지: `QUEUED/RUNNING → PAUSE_REQUESTED → PAUSED`
- 작업 취소는 도메인 영향이 없거나 보상 작업이 정의된 경우만 `CANCELLED`

### 14.5 단일 실행 알고리즘

1. enqueue 전에 `job_registry`에 unique key와 generation을 삽입한다.
2. 활성 상태 partial unique index 충돌이면 기존 job ID를 반환한다.
3. outbox relay가 Redis에 `job_id`만 게시한다.
4. worker는 job을 받은 뒤 DB에서 최신 상태를 확인한다.
5. unique key의 안정적 64-bit hash로 `pg_try_advisory_lock`을 얻는다.
6. 잠금 실패 시 실패 횟수를 늘리지 않고 짧은 jitter 후 재큐한다.
7. `RUNNING` 전환 후 heartbeat, checkpoint, processed/succeeded/failed count를 갱신한다.
8. 성공/실패 상태를 커밋하고 advisory lock 전용 DB connection을 반환한다.
9. worker crash 시 DB 연결 종료로 lock이 해제되고 visibility timeout 뒤 재시도한다.

도메인 레코드마다 `(campaign_id,user_id,ordinal)` 같은 고유 제약을 추가하여 worker 중복 실행 가능성에 대비한다.

### 14.6 작업별 정책

| 작업 | 동시성 키 | 배치 | retry | 영구 실패 예 |
|---|---|---:|---:|---|
| 캠페인 대상 산정 | campaign version | 1,000명 | 5 | 잘못된 대상 schema |
| 쿠폰 대량 발급 | campaign version | 500명 | 10 | 캠페인 취소 |
| 캠페인 회수 | campaign + case | 500장 | 10 | 승인 철회 |
| 만료 처리 | 시간 shard | 1,000건 | 무제한 지연 재시도 | schema 불일치 |
| 알림 발송 | event+channel+recipient | 1건/제공자 batch | 5 | 수신 거부·템플릿 거절 |
| 일 통계 집계 | store+business day | 상점 1개 | 5 | 삭제된 기준 데이터 |
| 개인정보 파기 | request/case | 사용자 1명 | 10 | 법적 hold 활성 |

### 14.7 재시도

- 기본: 5초부터 2배 지수 증가, 최대 30분, ±20% jitter
- provider 429/Retry-After는 제공자 값을 우선한다.
- validation, authorization, not-found는 재시도하지 않는다.
- DB serialization, timeout, network는 재시도한다.
- 각 시도에 동일 job ID와 새 attempt ID를 기록한다.
- dead-letter 재처리는 원인 해결 확인, 관리자 사유, 새 generation을 요구한다.

## 15. 알림 설계

### 15.1 채널

1. 앱 내 알림: 모든 거래·운영 사건의 기준 기록
2. FCM Web Push: 권한과 활성 subscription이 있는 브라우저
3. 카카오 알림톡: 승인된 정보성 템플릿과 적법한 발송 조건

일반 카카오 친구 메시지 API를 서비스의 대량 자동 알림 수단으로 사용하지 않는다. 알림톡은 공식 대행사/사업자 API를 `AlimtalkProvider` 인터페이스 뒤에 둬 교체 가능하게 한다.

### 15.2 이벤트와 템플릿

| 이벤트 | 필수 데이터 | 기본 우선순위 |
|---|---|---|
| `STAMP_EARNED` | 상점, 적립 수, 잔여 목표, 만료 | 보통 |
| `REWARD_ISSUED` | 혜택, 사용 종료, 조건 링크 | 높음 |
| `COUPON_ISSUED` | 캠페인, 혜택, 종료 | 보통/마케팅 판정 |
| `COUPON_EXPIRING` | 혜택, 남은 기간 | 보통 |
| `COUPON_USED` | 상점, 시각, 할인액, 거래 ID | 높음 |
| `TRANSACTION_VOIDED` | 원 거래, 복원 여부 | 높음 |
| `STORE_SUSPENDED/CLOSED` | 영향과 문의 경로 | 높음 |
| `SECURITY_ALERT` | 사건 시각, 세션 폐기 링크 | 긴급 |

- 템플릿은 code, version, locale, channel, provider template ID, approval status로 버전 관리한다.
- 발송 payload에는 사용자 입력 HTML을 넣지 않고 허용 변수만 escape한다.
- 템플릿 변경은 과거 발송 재현을 위해 기존 버전을 보존한다.

### 15.3 동의와 목적 판정

- 거래 완료·계정 보안은 정보성 서비스 알림이다.
- 신규 할인 캠페인, 장기 미방문 유도는 마케팅이다.
- 만료 임박은 혜택 안내 성격과 관계 법령·제공자 심사 결과에 따라 출시 전 최종 분류한다.
- 상점별 마케팅 동의와 전체 채널 동의를 모두 통과해야 해당 채널로 발송한다.
- 철회가 enqueue 이후 발생해도 실제 provider 호출 직전에 동의를 다시 확인한다.

### 15.4 전달 결과

- `PENDING`, `SENDING`, `DELIVERED`, `FAILED_RETRYABLE`, `FAILED_PERMANENT`, `SUPPRESSED`
- provider callback은 서명과 provider reference를 검증하고 멱등 처리한다.
- `DELIVERED`는 제공자 수락/전달 정의에 따르며 사용자가 읽었다는 뜻으로 표시하지 않는다.
- 영구 실패는 지갑 혜택 상태에 영향을 주지 않는다.

## 16. 보안 설계

### 16.1 위협 경계

- 인터넷 브라우저는 신뢰하지 않는다.
- Firebase 토큰이 유효해도 도메인 권한은 신뢰하지 않는다.
- Redis job payload와 provider callback은 중복·지연·위조 가능성을 전제한다.
- 점주가 입력한 주문 금액·품목은 증빙이 아니라 주장 데이터다.
- 시스템 관리자도 최소 권한과 감사 대상이다.

### 16.2 QR

- QR payload는 버전, nonce, 불투명 subject, audience, issued_at, expires_at만 포함한다.
- Ed25519 또는 ES256 비대칭 서명을 사용하고 key ID를 포함한다.
- 원문 consumer key와 개인정보를 넣지 않는다.
- nonce는 128bit 이상의 CSPRNG 값이며 DB에는 hash만 저장한다.
- 60초 만료, 성공 사용 1회, 서버 시각 검증을 적용한다.
- 수동 8자리 코드는 nonce에서 안전하게 파생하지 않고 별도 무작위 값과 hash를 사용한다.

### 16.3 웹 보안

- 모든 환경 HTTPS, HSTS, secure cookie
- Angular는 CSP nonce 기반으로 inline script를 제한하고 Trusted Types 적용을 검토한다.
- API CORS는 세 앱의 정확한 origin allowlist만 허용한다.
- OIDC 임시 쿠키는 HttpOnly, Secure, SameSite=Lax, 짧은 TTL
- state-changing API는 Bearer token 외 origin 검증과 필요 시 CSRF 방어를 적용한다.
- 파일 업로드는 MIME·magic byte·크기 검사, 재인코딩, 악성 검사, 공개/비공개 bucket 분리
- 로그에서 Authorization, token, QR, provider payload, 민감 개인정보를 제거한다.

### 16.4 속도 제한

| 대상 | 초기 제한 | 키 |
|---|---:|---|
| 로그인/가입 시작 | 10회/10분 | IP+브라우저 신호 |
| 카카오 callback 실패 | 20회/10분 | IP+state prefix |
| QR 발급 | 20회/분 | user |
| QR 해석 실패 | 30회/분 | owner+IP |
| 적립/사용 승인 | 30회/분 | store+owner |
| 선착순 받기 | 5회/분 | user+campaign, IP 보조 |
| 관리자 검색 | 120회/분 | admin |

제한값은 운영 설정으로 관리한다. IPv4/IPv6 공유망을 고려해 IP만으로 계정을 영구 차단하지 않는다.

### 16.5 비밀과 암호화

- Firebase service account private key, 카카오 client secret, 알림톡 API key는 secret manager에서 주입한다.
- 저장소와 Docker image, 일반 환경 예제에 실제 비밀을 넣지 않는다.
- 이메일·전화번호·사업자번호는 envelope encryption, 검색이 필요한 값은 별도 keyed hash를 사용한다.
- 키 회전 버전을 암호문에 저장하고 온라인 재암호화 작업을 지원한다.

## 17. 개인정보·법무 준비

출시 전 대한민국 개인정보·전자적 광고·쿠폰 표시 관련 법률 검토를 수행한다. 이 문서는 법률 자문을 대체하지 않는다.

### 17.1 최소 수집

- 소비자 필수: 인증 식별자, 표시 이름, 약관 동의
- 소비자 선택: 이메일/연락처, 위치, 외부 알림 동의
- 점주 필수: 대표자·사업자 정보, 상점 연락처, 운영 정보
- 거래: 가명 소비자, 상점, 시각, 조건, 주문 스냅샷, 처리 주체

### 17.2 공개와 위탁

- 개인정보 처리방침에 Firebase/Google, Kakao, 알림톡 사업자, hosting·monitoring 사업자의 처리 역할과 국외 이전 여부를 명시한다.
- 상점은 플랫폼의 고객 개인정보를 자유롭게 내려받는 독립 CRM이 아니라 자기 상점 내 가명 관계만 이용한다.
- 마케팅 발신 주체, 수신 거부 방법, 야간 발송 동의 요건을 채널 템플릿에 반영한다.

### 17.3 보존·파기

- 프로필, 동의, 거래, 감사, 민원, 보안 로그별 보존기간을 설정 테이블로 관리한다.
- 법적 보존 또는 분쟁 hold가 없으면 만료 후 파기 작업을 큐에 등록한다.
- 탈퇴자는 거래 원장의 user FK를 가명 tombstone으로 치환할 수 있게 설계한다.
- 백업 복원 후 이미 파기된 사용자가 살아나지 않도록 deletion ledger를 재적용한다.

## 18. 관측성·운영

### 18.1 서비스 수준 목표

| 항목 | 초기 SLO |
|---|---:|
| API 월 가용성 | 99.9% |
| 지갑 조회 p95 | 500ms 이하 |
| 적립/사용 승인 p95 | 800ms 이하, 외부 알림 제외 |
| 선착순 발급 p95 | 800ms 이하 |
| 직접 지급 지갑 반영 | 대상 확정 후 95% 60초 이내 |
| 만료 상태 반영 지연 | 5분 이하, 온라인 판정은 즉시 |
| 중복 논리 거래 | 0건 |

### 18.2 Health endpoint

- `/health/live`: 프로세스 event loop만 확인
- `/health/ready`: PostgreSQL 연결, migration version, 필수 설정 확인
- Redis 장애는 API 전체 readiness 실패가 아니라 기능별 degraded 상태로 노출할 수 있다.
- worker health는 마지막 heartbeat와 queue별 poll 시각으로 판정한다.

### 18.3 로그·trace

- 모든 요청에 `request_id`, 인증 후 `actor_id`, 상점 범위에 `store_id`, 변경에 `transaction_id`
- API → outbox → job → provider delivery에 같은 `correlation_id`
- 구조화 JSON 로그, 개인정보 필드 allowlist
- 느린 쿼리와 lock wait, serialization retry 횟수 수집

### 18.4 핵심 경보

- 중복 불변식 위반 또는 unique conflict 급증
- 적립/사용 오류율과 p95 임계 초과
- campaign backlog, notification backlog, outbox unpublished age
- dead-letter 신규 발생
- PostgreSQL connection/lock wait, replica lag(도입 시)
- Redis 연결 실패와 memory pressure
- Firebase token validation 오류 급증
- FCM/알림톡 provider 실패율·템플릿 거절
- 관리자 고위험 동작과 대량 조회

### 18.5 백업·복구

- PostgreSQL PITR 가능한 지속 백업과 일일 복구 검증
- 알림·이미지 object storage의 versioning과 lifecycle
- Redis는 재생 가능한 전달 계층으로 취급하고 PostgreSQL outbox/job registry에서 복구
- RPO 5분 이내, RTO 60분을 초기 목표로 하며 실제 복구 훈련으로 검증
- 복원 후 outbox 재발행, 만료 따라잡기, deletion ledger 재적용 순서를 runbook에 명시

## 19. 테스트 전략

### 19.1 단위 테스트

- 기간 경계 `[start,end)`와 상점 타임존/영업일 계산
- 정액·정률·무료 품목 할인과 1원 미만 버림
- 도장 목표 여러 번 달성, 만료순 소비, 취소 복원
- 캠페인 대상·개인·일일·총수량 판정
- 쿠폰 상태 전이의 허용/금지 조합
- 알림 목적·동의·야간 발송 판정
- job unique key 생성과 retry 분류

### 19.2 DB 통합 테스트

- 마지막 1장에 100개 동시 claim, 성공 수 정확히 1
- 같은 QR로 적립 100개 동시 승인, 원장 1건
- 같은 쿠폰 100개 동시 예약, 활성 예약 1건
- 예약 만료와 승인 동시 실행, 종결 상태 하나
- 같은 멱등키 같은 body는 같은 응답, 다른 body는 409
- 캠페인 중지와 발급 batch 경합
- 취소와 사용 경합
- worker crash 후 advisory lock 해제와 체크포인트 재개

실제 PostgreSQL과 Redis container를 사용하며 SQLite 대체 테스트로 정합성을 판단하지 않는다.

### 19.3 계약·외부 연동 테스트

- OpenAPI schema에서 Angular client를 생성하고 breaking diff 검사
- Firebase emulator로 이메일, 토큰 폐기, custom token 흐름 검증
- 카카오 OIDC callback의 state, nonce, kid 회전, 취소·오류 fixture
- FCM/알림톡 provider sandbox 또는 contract mock의 2xx, 4xx, 429, 5xx, callback 중복
- webhook 서명 실패와 replay 방지

### 19.4 Angular 테스트

- 공통 auth interceptor의 단일 token refresh와 무한 재시도 방지
- route guard의 약관·역할·상점 상태 처리
- 스캔 상태 머신과 카메라 권한 거부/복귀
- 지갑 empty/loading/stale/error/offline 상태
- 키보드, 스크린리더 이름, 포커스 복귀, 대비 자동 검사
- 360/768/1280px 반응형 visual regression
- 서비스 워커가 QR/API 응답을 캐시하지 않는지 확인

### 19.5 E2E 인수 테스트

[시나리오 명세의 MVP 인수 시나리오](./scenarios.md#20-mvp-인수-시나리오) 10개를 자동화 또는 운영 승인 테스트로 모두 수행한다. 현장용 Chrome Android와 Safari iOS에서 소비자 QR 표시, Chrome Android에서 점주 카메라 스캔을 실제 기기로 검증한다.

## 20. 배포·마이그레이션·롤아웃

### 20.1 환경

- `local`: Firebase emulator와 provider mock
- `development`: 개발용 Firebase/Kakao 앱, 합성 개인정보
- `staging`: production 동등 topology, provider sandbox/허용 번호
- `production`: 별도 프로젝트·키·DB·Redis·도메인

환경 간 사용자·비밀·실제 메시지 대상을 공유하지 않는다.

### 20.2 배포 순서

1. 확장 가능한 DB migration 적용
2. worker와 API를 이전/신규 schema 동시 호환 버전으로 배포
3. Angular 앱 배포
4. 새 기능 flag 활성화
5. 비호환 컬럼 정리는 최소 한 릴리스 뒤 별도 migration

- migration은 transaction 가능 여부, lock 시간, rollback/forward-fix를 사전 검토한다.
- 대규모 인덱스는 production에서 concurrent 생성한다.
- 캠페인 발급 중 worker 롤링 배포가 발생해도 job registry/checkpoint로 중복 없이 이어져야 한다.

### 20.3 단계적 출시

1. 내부 계정·테스트 상점
2. 협력 카페 1곳, 소비자 50명 이하
3. 상점 10곳, 선착순 캠페인 제한적 활성화
4. 알림톡 심사·비용·수신거부 검증 후 채널 활성화
5. SLO·민원·부정사용 지표 충족 시 일반 가입

각 단계는 중복 거래 0, 미해결 심각 민원 0, 복구 훈련 성공을 다음 단계 조건으로 둔다.

### 20.4 기능 플래그

- 카카오 로그인
- FCM Web Push
- 카카오 알림톡
- 선착순 캠페인
- 대상자 대량 직접 지급
- 관리자 대량 회수

플래그 비활성화는 신규 진입만 막고 이미 생성된 도메인 상태를 손상하지 않는다.

## 21. 구현 순서

### Phase 1: 기반

- Rust workspace, PostgreSQL migration, Redis, observability
- Firebase 이메일 인증과 내부 사용자 bootstrap
- Angular 공통 라이브러리, 세 웹앱 shell, 디자인 토큰
- 상점 초안·검수·활성 상태

### Phase 2: 도장 핵심

- 카탈로그와 도장 정책 버전
- 회전형 QR과 점주 스캔
- 적립 원장, 목표 달성, 리워드 지갑
- 관리자 거래 탐색과 보정 preview

### Phase 3: 할인 쿠폰

- 캠페인 작성·검증·게시
- 선착순 claim과 직접 지급 worker
- 사용 예약·승인·취소
- 수량/기간/품목 동시성 테스트

### Phase 4: 알림·운영

- 앱 내 알림, FCM, 알림톡 provider
- queue dashboard, DLQ, 재처리
- 통계, 민원, 제재, 감사
- 개인정보 파기와 백업 복구 runbook

### Phase 5: 출시 검증

- 실제 모바일 카메라·PWA·접근성 검증
- 부하·경합·장애 주입 테스트
- 법률·약관·알림 템플릿 검토
- 협력 상점 단계적 롤아웃

## 22. 후속 확장 설계 지점

- 다지점: `organization → stores`와 조직 범위 캠페인
- 직원: owner invitation, 역할별 스캔/캠페인/통계 권한과 근무 세션
- POS: provider adapter, 서명된 주문 수신, 적립/사용 승인 자동화
- 구독: plan entitlement와 사용량 meter를 별도 billing bounded context로 추가
- 고급 세그먼트: 개인정보 보호 기준을 가진 cohort query builder
- 네이티브 앱: 현재 REST/OpenAPI 계약과 Firebase 계정을 재사용
- SSE/WebSocket: 현재 version/cursor 기반 polling을 점진 교체
- 쿠폰 선물: 양도 가능성, 세금·부정사용·개인정보 법률 검토 후 별도 원장 사건 추가

후속 기능이 들어와도 현재 쿠폰 인스턴스와 도장 원장의 의미를 바꾸지 않고 새 actor, scope, event type으로 확장한다.

## 23. 확정 기본값과 미출시 전 확인사항

### 23.1 확정 기본값

- 대한민국, 한국어, KRW, `Asia/Seoul`
- Angular 21 웹앱 3개와 공통 라이브러리
- 계정당 상점 1개, 점주 1명, 모든 회원의 소비자 프로필 허용
- 방문 도장 주문당 1개, 목표 10개, 개별 만료 180일, 일 1회
- 리워드 발급 후 30일 유효
- QR 60초, 자동 화면 갱신 30초, 사용 예약 2분
- 주문당 사용 혜택 1개
- 점주 취소 한도: 적립 24시간, 사용 10분
- Rust Axum + SQLx + PostgreSQL + Redis/Apalis
- 동일 작업은 job registry, advisory lock, 도메인 멱등 제약의 3중 방어
- 앱 내 알림을 기준 기록으로 하고 FCM/알림톡은 독립 전달 채널

### 23.2 출시 전 외부 확정 필요

- 카카오 앱 비즈니스 설정, OIDC, 연결 해제 webhook과 심사 항목
- 알림톡 공식 대행사, 템플릿 승인, 단가, 발신 채널, 광고성 분류
- Firebase 프로젝트/tenant, 관리자 MFA, App Check 지원 범위
- 개인정보 처리방침, 이용약관, 마케팅·야간 발송 동의 문구
- 거래·감사·민원·백업의 법정 보존기간
- 운영 도메인, 데이터 저장 지역, 비밀 관리자, 백업 대상

외부 확정 항목은 제품의 핵심 상태 모델이나 원장 정합성을 바꾸지 않도록 provider와 설정 경계 안에서 처리한다.

### 23.3 구현 시 확인할 공식 자료

- [카카오 로그인 이해하기](https://developers.kakao.com/docs/ko/kakaologin/common)
- [카카오 로그인 REST API](https://developers.kakao.com/docs/ko/kakaologin/rest-api)
- [Firebase Custom Token 만들기](https://firebase.google.com/docs/auth/admin/create-custom-tokens)
- [카카오톡 메시지 제품 선택 FAQ](https://developers.kakao.com/docs/ko/kakaotalk-message/faq)
- [Apalis Rust 문서](https://docs.rs/apalis/latest/apalis/)

외부 서비스의 심사 요건, API 제한, SDK 버전은 변경될 수 있으므로 구현 시작과 출시 직전에 위 공식 자료를 다시 확인한다.
