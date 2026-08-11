# 동시성 결정표

> 원본: [scenarios.md §15](./scenarios.md#15-동시성-결정표)
> `구현 위치` 는 `apps/coupon-api-server` 소스·테스트를 대조한 결과다. 찾지 못한 항목은 `미구현`이다.
> Phase 3(캠페인·사용)·Phase 4(알림·운영) 이후 아래 8행은 모두 서버 통합 테스트로 덮였다.
> 테스트 이름은 `apps/coupon-api-server/tests/` 에서 `grep` 으로 실재를 확인했다.

| 경합 | 승자 결정 | 패자 응답 | 불변식 | 구현 위치 |
|---|---|---|---|---|
| 마지막 선착순 쿠폰 N명 동시 요청 | 캠페인 수량 행 잠금 후 먼저 커밋 | `SOLD_OUT` | 발급 수량 ≤ 총수량 | `tests/campaigns.rs::a_hundred_simultaneous_claims_on_the_last_coupon_produce_exactly_one` |
| 동일 쿠폰 동시 예약 | 쿠폰 행 조건부 갱신 승자 | `COUPON_NOT_AVAILABLE` | 활성 예약 최대 1개 | `tests/campaigns.rs::a_hundred_simultaneous_reservations_of_one_coupon_leave_one_active` |
| 예약 만료와 최종 승인 | 행 잠금 후 서버 시각/버전 검증 | `RESERVATION_EXPIRED` 또는 만료 작업 무효 | 사용·복구 중 하나만 커밋 | `tests/campaigns.rs::an_expiring_reservation_and_a_confirmation_settle_on_exactly_one_outcome` |
| 동일 QR 동시 스캔 | nonce 소비 조건부 갱신 승자 | `QR_ALREADY_USED` | nonce 성공 소비 최대 1회 | `apps/coupon-api-server/src/qr/mod.rs` (`consume`, `lock`, `ensure_unconsumed`), `apps/coupon-api-server/src/loyalty/stamps.rs` (`confirm`); `tests/loyalty.rs::one_qr_scanned_a_hundred_times_at_once_produces_one_ledger_entry` |
| 적립 승인과 일일 만료/경계 | DB 서버 시각과 영업일 키 | 조건 불충족 | 일일 한도 초과 없음 | `apps/coupon-api-server/src/loyalty/stamps.rs` (`assess`), `apps/coupon-api-server/src/stores/business_day.rs`; `tests/loyalty.rs::a_daily_limit_blocks_the_second_accrual_and_names_the_next_business_day` |
| 점주 취소와 소비자 사용 | 쿠폰·원 거래 순서 고정 잠금 | 최신 상태에 맞는 충돌 오류 | 사용과 회수 동시 불가 | `tests/campaigns.rs::a_revocation_and_a_use_of_the_same_coupon_cannot_both_win` |
| 캠페인 중지와 발급 배치 | 상태 버전 확인 후 배치 커밋 | 다음 배치 중단 | 중지 확인 뒤 신규 배치 없음 | `tests/campaigns.rs::pausing_a_campaign_stops_the_issuing_batch_and_resuming_continues_it` |
| 동일 큐 작업 여러 워커 | advisory lock 소유 워커 | 지연 재큐 | 동일 키 동시 실행 1개 | `tests/campaigns.rs::an_advisory_lock_blocks_a_second_worker_and_is_released_when_the_first_goes_away`; 보조: `concurrent_registrations_of_one_key_still_produce_one_job`, `a_job_interrupted_mid_run_resumes_from_its_checkpoint` |

## 잠금 순서

기획서 §13.1:

> 모든 테이블 잠금 순서는 `store → policy/campaign → user/store_customer → coupon/stamp lot → nonce`로 고정해 교착을 줄인다.

적립·취소·예약·발급이 같은 테이블을 건드릴 때 이 순서를 지키면 서로 다른 코드 경로가 반대 순서로 잠그며 생기는 교착을 줄일 수 있다. 구현된 도장 적립(`stamps.rs::confirm`)과 적립 취소(`stamps.rs::void`)는 상점 잠금을 먼저 취한 뒤 하위 행을 잠근다.
