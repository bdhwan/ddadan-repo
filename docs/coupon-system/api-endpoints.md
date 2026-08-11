# 쿠폰 시스템 엔드포인트 목록 (구현 체크리스트)

> 이 파일은 구현 진행 상황 체크리스트다. 엔드포인트를 구현하면 상태를 `구현완료` 로 바꾼다.
> 원본: `product-spec.md` §11.2 ~ §11.5

## 공통 규약

- Base path: `/api/coupon/v1`
- 변경 요청에는 `Idempotency-Key`(UUID) 가 **필수**다.
- 식별자는 UUID, 시각은 RFC 3339 UTC, 금액은 원 단위 정수.
- 상세 규약은 기획서 §11.1 을 참고한다.

## 11.2 인증·회원 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `GET` | `/auth/kakao/authorize` | state/PKCE를 만들고 authorize URL 반환 | 미구현 |
| `GET` | `/auth/kakao/callback` | OIDC 검증 후 일회용 교환 코드 발급 | 미구현 |
| `POST` | `/auth/kakao/exchange` | 교환 코드로 Firebase Custom Token 반환 | 미구현 |
| `POST` | `/webhooks/kakao/unlink` | 카카오 연결 해제 멱등 처리 | 미구현 |
| `POST` | `/users/bootstrap` | Firebase 가입 후 내부 회원/프로필 생성 | 미구현 |
| `GET/PATCH` | `/me` | 내 프로필 조회·수정 | 미구현 |
| `GET` | `/me/roles` | 활성 역할과 상점 참조 | 미구현 |
| `POST/DELETE` | `/me/auth-links/kakao` | 로그인 수단 연결·해제 | 미구현 |
| `GET/POST` | `/me/consents` | 동의 조회·변경 | 미구현 |
| `POST` | `/me/withdrawal` | 재인증 후 탈퇴 요청 | 미구현 |

## 11.3 소비자 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `GET` | `/public/stores` | 공개 상점 검색 | 미구현 |
| `GET` | `/public/stores/:slug` | 상점·공개 정책·캠페인 | 미구현 |
| `PUT/DELETE` | `/me/favorite-stores/:store_id` | 관심 등록/해제 | 미구현 |
| `GET` | `/me/wallet/coupons` | 쿠폰 목록과 상태 필터 | 미구현 |
| `GET` | `/me/wallet/coupons/:id` | 조건 스냅샷 포함 상세 | 미구현 |
| `GET` | `/me/wallet/stamps` | 상점별 가용·만료 예정 도장 | 미구현 |
| `POST` | `/me/qr-tokens` | 60초 회전형 QR와 보조 코드 발급 | 미구현 |
| `POST` | `/campaigns/:id/claims` | 선착순 쿠폰 받기 | 미구현 |
| `GET/PATCH` | `/me/notifications` | 앱 내 알림 조회·읽음 | 미구현 |
| `POST/DELETE` | `/me/push-subscriptions` | FCM Web Push 토큰 등록·해제 | 미구현 |

선착순 받기 응답은 성공 시 쿠폰 ID를, 중복 요청이면 기존 쿠폰 ID를, 소진이면 `CAMPAIGN_SOLD_OUT`을 반환한다.

## 11.4 점주 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `POST` | `/owner/store` | 상점 초안 생성, 계정당 1개 | 미구현 |
| `GET/PATCH` | `/owner/store` | 자기 상점 조회·수정 | 미구현 |
| `POST` | `/owner/store/submit-review` | 검수 제출 | 미구현 |
| `GET/POST/PATCH` | `/owner/catalog/items` | 품목 관리 | 미구현 |
| `GET/POST` | `/owner/loyalty-policies` | 정책 버전 목록·초안 생성 | 미구현 |
| `PATCH` | `/owner/loyalty-policies/:id` | 초안 수정 | 미구현 |
| `POST` | `/owner/loyalty-policies/:id/publish` | 즉시/예약 게시 | 미구현 |
| `POST` | `/owner/scan/resolve` | QR 검증과 가명 고객 조회 | 미구현 |
| `POST` | `/owner/stamp-transactions/preview` | 적립 조건·예상 결과 검증 | 미구현 |
| `POST` | `/owner/stamp-transactions` | 최종 적립 승인 | 미구현 |
| `POST` | `/owner/stamp-transactions/:id/void` | 24시간 내 취소 | 미구현 |
| `GET/POST` | `/owner/campaigns` | 캠페인 목록·초안 생성 | 미구현 |
| `PATCH` | `/owner/campaigns/:id` | 초안/허용 필드 수정 | 미구현 |
| `POST` | `/owner/campaigns/:id/publish` | 캠페인 게시·발급 작업 등록 | 미구현 |
| `POST` | `/owner/campaigns/:id/pause` | 신규 발급 중지 | 미구현 |
| `POST` | `/owner/campaigns/:id/resume` | 안전 재개 | 미구현 |
| `POST` | `/owner/campaigns/:id/cancel` | 취소와 회수 정책 지정 | 미구현 |
| `POST` | `/owner/redemptions/preview` | 조건 검증과 2분 예약 생성 | 미구현 |
| `POST` | `/owner/redemptions/:id/confirm` | 사용 최종 승인 | 미구현 |
| `POST` | `/owner/redemptions/:id/cancel` | 예약 또는 10분 내 사용 취소 | 미구현 |
| `GET` | `/owner/customers` | 가명 고객과 자기 상점 지표 | 미구현 |
| `GET` | `/owner/analytics` | 잠정/확정 통계 | 미구현 |

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

## 11.5 시스템 관리자 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `GET` | `/admin/store-reviews` | 검수 큐 | 미구현 |
| `POST` | `/admin/store-reviews/:id/decision` | 승인·보완·거절 | 미구현 |
| `GET` | `/admin/users/:id` | 마스킹된 회원·사건 조회 | 미구현 |
| `POST` | `/admin/users/:id/suspend` | 임시/영구 제재 요청 | 미구현 |
| `POST` | `/admin/users/:id/revoke-sessions` | Firebase 세션 폐기 | 미구현 |
| `GET` | `/admin/transactions/:id` | 연결 원장과 감사 타임라인 | 미구현 |
| `POST` | `/admin/adjustments/preview` | 보정 결과 시뮬레이션 | 미구현 |
| `POST` | `/admin/adjustments` | 승인된 보정 사건 생성 | 미구현 |
| `POST` | `/admin/campaigns/:id/emergency-stop` | 긴급 중단 | 미구현 |
| `POST` | `/admin/campaigns/:id/revoke-job` | 대량 회수 작업 | 미구현 |
| `GET/POST` | `/admin/cases` | 민원·보안 사건 관리 | 미구현 |
| `GET` | `/admin/jobs` | 작업·시도·체크포인트 | 미구현 |
| `POST` | `/admin/jobs/:id/retry` | 사유 포함 재처리 | 미구현 |
| `GET` | `/admin/audit-logs` | 감사 검색 | 미구현 |
