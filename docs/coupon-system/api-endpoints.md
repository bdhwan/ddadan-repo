# 쿠폰 시스템 엔드포인트 목록 (구현 체크리스트)

> **이 파일의 기준은 `apps/coupon-api-server/openapi.json` 이다.**
> `구현 상태` 의 유일한 근거는 openapi `paths` 다. openapi에 있으면 `구현완료`, 없으면 `미구현`이다. openapi에 없는 경로를 `구현완료`로 표시하지 않는다.
> 원본 경로 목록: `product-spec.md` §11.2 ~ §11.5 (일부 운영·헬스·콜백 경로는 openapi에만 있어 아래에 추가했다).
> 대조 시각: 작업 마무리 직전 openapi 재확인.

## 공통 규약

- Base path: `/api/coupon/v1`
- 변경 요청에는 `Idempotency-Key`(UUID) 가 **필수**다.
- 식별자는 UUID, 시각은 RFC 3339 UTC, 금액은 원 단위 정수.
- 상세 규약은 기획서 §11.1 을 참고한다.

## 헬스체크 (§18.2)

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `GET` | `/health/live` | 프로세스 event loop만 확인 | 구현완료 |
| `GET` | `/health/ready` | PostgreSQL 연결, migration version, 필수 설정 확인 | 구현완료 |

## 11.2 인증·회원 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `GET` | `/auth/kakao/authorize` | state/PKCE를 만들고 authorize URL 반환 | 구현완료 |
| `GET` | `/auth/kakao/callback` | OIDC 검증 후 일회용 교환 코드 발급 | 구현완료 |
| `POST` | `/auth/kakao/exchange` | 교환 코드로 Firebase Custom Token 반환 | 구현완료 |
| `POST` | `/webhooks/kakao/unlink` | 카카오 연결 해제 멱등 처리 | 구현완료 |
| `POST` | `/users/bootstrap` | Firebase 가입 후 내부 회원/프로필 생성 | 구현완료 |
| `GET/PATCH` | `/me` | 내 프로필 조회·수정 | 구현완료 |
| `GET` | `/me/roles` | 활성 역할과 상점 참조 | 구현완료 |
| `POST/DELETE` | `/me/auth-links/kakao` | 로그인 수단 연결·해제 | 구현완료 |
| `GET/POST` | `/me/consents` | 동의 조회·변경 | 구현완료 |
| `POST` | `/me/withdrawal` | 재인증 후 탈퇴 요청 | 미구현 |

## 11.3 소비자 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `GET` | `/public/stores` | 공개 상점 검색 | 미구현 |
| `GET` | `/public/stores/{slug}` | 상점·공개 정책·캠페인 | 미구현 |
| `PUT/DELETE` | `/me/favorite-stores/{store_id}` | 관심 등록/해제 | 미구현 |
| `GET` | `/me/wallet/coupons` | 쿠폰 목록과 상태 필터 | 구현완료 |
| `GET` | `/me/wallet/coupons/{coupon_id}` | 조건 스냅샷 포함 상세 | 구현완료 |
| `GET` | `/me/wallet/stamps` | 상점별 가용·만료 예정 도장 | 구현완료 |
| `POST` | `/me/qr-tokens` | 60초 회전형 QR와 보조 코드 발급 | 구현완료 |
| `POST` | `/campaigns/{campaign_id}/claims` | 선착순 쿠폰 받기 | 구현완료 |
| `GET/PATCH` | `/me/notifications` | 앱 내 알림 조회·읽음 | 구현완료 |
| `GET/POST` | `/me/push-subscriptions` | FCM Web Push 토큰 목록·등록 | 구현완료 |
| `DELETE` | `/me/push-subscriptions/{subscription_id}` | FCM Web Push 토큰 해제 | 구현완료 |
| `POST` | `/notifications/callbacks/{provider}` | 알림 provider callback (서명·replay 검증) | 구현완료 |

선착순 받기 응답은 성공 시 쿠폰 ID를, 중복 요청이면 기존 쿠폰 ID를, 소진이면 `CAMPAIGN_SOLD_OUT`을 반환한다.

## 11.4 점주 API

| Method | Path | 설명 | 구현 상태 |
|---|---|---|---|
| `POST` | `/owner/store` | 상점 초안 생성, 계정당 1개 | 구현완료 |
| `GET/PATCH` | `/owner/store` | 자기 상점 조회·수정 | 구현완료 |
| `POST` | `/owner/store/submit-review` | 검수 제출 | 구현완료 |
| `GET/POST` | `/owner/catalog/categories` | 카테고리 목록·생성 | 구현완료 |
| `PATCH` | `/owner/catalog/categories/{category_id}` | 카테고리 수정 | 구현완료 |
| `GET/POST` | `/owner/catalog/items` | 품목 목록·생성 | 구현완료 |
| `PATCH` | `/owner/catalog/items/{item_id}` | 품목 수정 | 구현완료 |
| `GET/POST` | `/owner/loyalty-policies` | 정책 버전 목록·초안 생성 | 구현완료 |
| `PATCH` | `/owner/loyalty-policies/{policy_id}` | 초안 수정 | 구현완료 |
| `POST` | `/owner/loyalty-policies/{policy_id}/publish` | 즉시/예약 게시 | 구현완료 |
| `POST` | `/owner/scan/resolve` | QR 검증과 가명 고객 조회 | 구현완료 |
| `POST` | `/owner/stamp-transactions/preview` | 적립 조건·예상 결과 검증 | 구현완료 |
| `POST` | `/owner/stamp-transactions` | 최종 적립 승인 | 구현완료 |
| `POST` | `/owner/stamp-transactions/{transaction_id}/void` | 24시간 내 취소 | 구현완료 |
| `GET/POST` | `/owner/campaigns` | 캠페인 목록·초안 생성 | 구현완료 |
| `GET/PATCH` | `/owner/campaigns/{campaign_id}` | 캠페인 조회·초안/허용 필드 수정 | 구현완료 |
| `GET` | `/owner/campaigns/{campaign_id}/estimate` | 대상 규모 추정 | 구현완료 |
| `POST` | `/owner/campaigns/{campaign_id}/publish` | 캠페인 게시·발급 작업 등록 | 구현완료 |
| `POST` | `/owner/campaigns/{campaign_id}/pause` | 신규 발급 중지 | 구현완료 |
| `POST` | `/owner/campaigns/{campaign_id}/resume` | 안전 재개 | 구현완료 |
| `POST` | `/owner/campaigns/{campaign_id}/cancel` | 취소와 회수 정책 지정 | 구현완료 |
| `POST` | `/owner/redemptions/preview` | 조건 검증과 2분 예약 생성 | 구현완료 |
| `POST` | `/owner/redemptions/{reservation_id}/confirm` | 사용 최종 승인 | 구현완료 |
| `POST` | `/owner/redemptions/{reservation_id}/cancel` | 예약 또는 10분 내 사용 취소 | 구현완료 |
| `GET` | `/owner/customers` | 가명 고객과 자기 상점 지표 | 미구현 |
| `GET` | `/owner/analytics` | 잠정/확정 통계 | 구현완료 |

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
| `GET` | `/admin/store-reviews` | 검수 큐 | 구현완료 |
| `POST` | `/admin/store-reviews/{review_id}/decision` | 승인·보완·거절 | 구현완료 |
| `GET` | `/admin/users/{user_id}` | 마스킹된 회원·사건 조회 | 미구현 |
| `POST` | `/admin/users/{user_id}/suspend` | 임시/영구 제재 요청 | 구현완료 |
| `POST` | `/admin/users/{user_id}/revoke-sessions` | Firebase 세션 폐기 | 구현완료 |
| `GET` | `/admin/transactions/{transaction_id}` | 연결 원장과 감사 타임라인 | 구현완료 |
| `POST` | `/admin/adjustments/preview` | 보정 결과 시뮬레이션 | 구현완료 |
| `POST` | `/admin/adjustments` | 승인된 보정 사건 생성 | 구현완료 |
| `POST` | `/admin/campaigns/{campaign_id}/emergency-stop` | 긴급 중단 | 구현완료 |
| `POST` | `/admin/campaigns/{campaign_id}/revoke-job` | 대량 회수 작업 | 구현완료 |
| `GET/POST` | `/admin/cases` | 민원·보안 사건 관리 | 구현완료 |
| `GET/PATCH` | `/admin/cases/{case_id}` | 민원 단건 조회·갱신 | 구현완료 |
| `GET` | `/admin/jobs` | 작업·시도·체크포인트 | 구현완료 |
| `GET` | `/admin/jobs/{job_id}` | 단일 작업 상세 | 구현완료 |
| `POST` | `/admin/jobs/{job_id}/retry` | 사유 포함 재처리 | 구현완료 |
| `GET` | `/admin/audit-logs` | 감사 검색 | 구현완료 |
| `GET` | `/admin/metrics` | 운영 지표 (§18.4 신호) | 구현완료 |
| `GET/POST` | `/admin/privacy/erasures` | 개인정보 파기 요청·실행 | 구현완료 |
| `POST` | `/admin/privacy/erasures/reapply` | deletion ledger 재적용 | 구현완료 |
| `GET` | `/admin/retention-policies` | 보존기간 정책 목록 | 구현완료 |
| `PATCH` | `/admin/retention-policies/{data_category}` | 보존기간 정책 변경 | 구현완료 |
