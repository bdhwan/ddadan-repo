# MVP 인수 시나리오 체크리스트

> 원본: [scenarios.md §20](./scenarios.md#20-mvp-인수-시나리오)
> 출시 전 이 10개를 자동화 또는 운영 승인 테스트로 모두 통과해야 한다.
> `자동검증` 근거는 `apps/coupon-api-server/tests/` 와 앱 `*.spec.ts` 에 실재하는 테스트 이름만 적는다. 확실하지 않으면 `미검증`이다.
> 테스트 이름은 `grep` 으로 실재를 확인했다.

| 번호 | 시나리오 | 검증 방법 | 상태 |
|---|---|---|---|
| 1 | 소비자가 카카오로 가입하고 여러 상점을 관심 등록한 뒤 한 지갑에서 조회한다. | openapi에 `/auth/kakao/*`, `/me/favorite-stores/*` 없음 | 미검증 |
| 2 | 점주가 상점을 개설하고 승인받아 10회 방문 도장 정책을 활성화한다. | 서버 통합 테스트 | 자동검증(서버): `an_owner_gets_one_store_and_walks_it_to_review` (`tests/api.rs`), `the_review_queue_activates_a_store_on_approval` (`tests/operations.rs`), `an_active_policy_is_replaced_by_a_new_version_not_edited` (`tests/loyalty.rs`) |
| 3 | 점주가 소비자의 회전형 QR을 스캔해 도장을 10번째 적립하고 리워드가 한 번만 발급된다. | 서버 통합 테스트 | 자동검증(서버): `reaching_the_goal_issues_a_reward_in_the_same_transaction`, `one_qr_scanned_a_hundred_times_at_once_produces_one_ledger_entry` (`tests/loyalty.rs`) |
| 4 | 소비자가 리워드를 사용하고 네트워크 응답 유실 뒤 재시도해도 사용은 한 번만 기록된다. | 서버·클라이언트 | 자동검증(서버, 사용 경로): `a_direct_campaign_runs_from_publication_through_to_a_confirmed_use` (`tests/campaigns.rs`). 자동검증(클라이언트, 멱등키 재사용·불확실 재시도 UI): `store-operations.api.spec.ts` — `reuses the explicit idempotency key while confirming an uncertain result`; `scan-state-machine.spec.ts` — `moves submitting to failure and retries an uncertain result without resetting`. 서버가 재시도로 사용을 한 번만 기록한다는 전용 테스트는 없어 그 부분은 미검증 |
| 5 | 점주가 수량 1개의 선착순 할인 캠페인을 열고 동시 요청 중 한 명만 받는다. | 서버·클라이언트 | 자동검증(서버): `a_hundred_simultaneous_claims_on_the_last_coupon_produce_exactly_one` (`tests/campaigns.rs`). 자동검증(클라이언트, 받기 UX): `store-detail-state.spec.ts` — 성공/중복/소진/멱등키 유지 |
| 6 | 만료 시각과 사용 승인이 경합해도 만료된 쿠폰이 사용되지 않는다. | 서버 통합 테스트 | 자동검증(서버): `an_expiring_reservation_and_a_confirmation_settle_on_exactly_one_outcome` (`tests/campaigns.rs`) |
| 7 | 같은 캠페인 발급 작업을 여러 API·워커가 처리해도 동일 작업은 동시에 하나만 실행되고 고객별 발급도 한 번뿐이다. | 서버 통합 테스트 | 자동검증(서버): `an_advisory_lock_blocks_a_second_worker_and_is_released_when_the_first_goes_away`, `concurrent_registrations_of_one_key_still_produce_one_job`, `a_job_interrupted_mid_run_resumes_from_its_checkpoint` (`tests/campaigns.rs`) |
| 8 | FCM·알림톡이 실패해도 쿠폰은 지갑에 존재하고 재시도·실패 상태가 운영 화면에 나타난다. | 서버 통합 테스트 | 자동검증(서버, 지갑 유지·실패 상태): `a_permanent_provider_failure_leaves_the_reward_untouched`, `clearing_the_inbox_leaves_the_ledger_alone`, `every_provider_status_maps_onto_the_15_4_vocabulary` (`tests/notifications.rs`). 운영 관리자 화면에 실패/재시도가 보인다는 클라이언트 검증은 미검증 |
| 9 | 점주가 잘못 적립한 거래를 취소하고 관련 리워드 상태에 맞게 원장과 잔액이 보정된다. | 서버 통합 테스트 | 자동검증(서버): `reversing_an_accrual_takes_back_the_reward_and_restores_the_stamps` (`tests/loyalty.rs`) |
| 10 | 시스템 관리자가 민원 증거를 조회해 보정하고 모든 행위가 변경 불가능한 감사 로그에 남는다. | 서버 통합 테스트 | 자동검증(서버): `an_administrator_can_follow_one_transaction_end_to_end` (`tests/loyalty.rs`), `a_ledger_correction_needs_a_second_administrator_and_runs_as_a_job` (`tests/campaigns.rs`), `a_case_carries_its_resolution_and_its_audit_trail`, `a_tampered_audit_entry_is_reported_as_broken` (`tests/operations.rs`) |

## 실기기 검증

기획서 §19.5. 아래는 사람이 수행한다. 자동화 대상이 아니다.

- [ ] **Chrome Android — 소비자 QR 표시**
  1. Android 실기기에서 Chrome으로 소비자 앱(`coupon-consumer-app`, 로컬이면 `:4310`)에 로그인한다.
  2. 지갑(또는 QR) 화면으로 이동해 회전형 QR과 보조 코드가 표시되는지 확인한다.
  3. 약 60초 대기 후 QR/코드가 갱신되는지 확인한다. 화면이 꺼지지 않게 유지되는지(또는 wake lock/안내)도 본다.
  4. 기대: QR·보조 코드가 보이고, 만료 전에 점주 스캔에 쓸 수 있다.

- [ ] **Safari iOS — 소비자 QR 표시**
  1. iPhone Safari로 같은 소비자 앱 URL에 로그인한다.
  2. 지갑/QR 화면에서 QR·보조 코드 표시를 확인한다.
  3. 백그라운드→포그라운드 복귀 후 만료된 토큰이면 재발급·갱신 UI가 동작하는지 확인한다.
  4. 기대: Chrome Android와 동일하게 표시·갱신되며, iOS Safari 특유의 카메라/PWA 제약으로 화면이 깨지지 않는다.

- [ ] **Chrome Android — 점주 카메라 스캔**
  1. Android Chrome으로 점주 앱(`coupon-store-app`, 로컬이면 `:4320`)에 점주 계정으로 로그인한다.
  2. 스캔 화면에서 카메라 권한을 허용하고, 위 소비자 QR을 비춘다.
  3. 카메라 거부 시 수동 코드 입력 경로로 같은 고객이 resolve되는지 확인한다.
  4. preview → 적립 승인까지 진행해 도장이 반영되는지 확인한다.
  5. 기대: resolve 성공 → preview → confirm 후 원장/지갑에 반영. 권한 거부 시에도 수동 코드로 동일 흐름이 가능하다.
