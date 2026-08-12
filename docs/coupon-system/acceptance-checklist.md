# MVP 인수 시나리오 체크리스트

> 원본: [scenarios.md §20](./scenarios.md#20-mvp-인수-시나리오)
> 출시 전 이 10개를 자동화 또는 운영 승인 테스트로 모두 통과해야 한다.
> `자동검증` 근거는 `apps/coupon-api-server/tests/` 와 앱 `*.spec.ts` 에 실재하는 테스트 이름만 적는다. 확실하지 않으면 `미검증`이다.
> 테스트 이름은 `grep` 으로 실재를 확인했다. 대조 시각: 출시검증 TASK C 문서 작업 시점(커밋 `dda52ef` + 이후 같은 워크트리).

| 번호 | 시나리오 | 검증 방법 | 상태 |
|---|---|---|---|
| 1 | 소비자가 카카오로 가입하고 여러 상점을 관심 등록한 뒤 한 지갑에서 조회한다. | openapi/`api-endpoints.md`: `/auth/kakao/*`, `/me/favorite-stores/*` 미구현. 카카오·관심상점 E2E 테스트 없음 | 미검증 |
| 2 | 점주가 상점을 개설하고 승인받아 10회 방문 도장 정책을 활성화한다. | 서버 통합 테스트 | 자동검증(서버): `an_owner_gets_one_store_and_walks_it_to_review` (`tests/api.rs`), `the_review_queue_activates_a_store_on_approval` (`tests/operations.rs`), `an_active_policy_is_replaced_by_a_new_version_not_edited` (`tests/loyalty.rs`) |
| 3 | 점주가 소비자의 회전형 QR을 스캔해 도장을 10번째 적립하고 리워드가 한 번만 발급된다. | 서버 통합 테스트 | 자동검증(서버): `reaching_the_goal_issues_a_reward_in_the_same_transaction`, `one_qr_scanned_a_hundred_times_at_once_produces_one_ledger_entry` (`tests/loyalty.rs`). 보조: `a_scan_resolves_previews_and_confirms_into_one_ledger_entry` (`tests/loyalty.rs`) |
| 4 | 소비자가 리워드를 사용하고 네트워크 응답 유실 뒤 재시도해도 사용은 한 번만 기록된다. | 서버·클라이언트 | 자동검증(서버, 사용 정상 경로): `a_direct_campaign_runs_from_publication_through_to_a_confirmed_use` (`tests/campaigns.rs`). 자동검증(클라이언트, 멱등키 재사용·불확실 재시도 UI): `store-operations.api.spec.ts` — `reuses the explicit idempotency key while confirming an uncertain result`; `scan-state-machine.spec.ts` — `moves submitting to failure and retries an uncertain result without resetting`. 서버가 **사용(redemption) 응답 유실 재시도**로 사용을 한 번만 기록한다는 전용 테스트는 없어 그 부분은 미검증. 적립 멱등 재시도는 `the_same_key_replays_and_a_different_body_conflicts` (`tests/loyalty.rs`) 가 있으나 시나리오 4의 사용 경로가 아니다 |
| 5 | 점주가 수량 1개의 선착순 할인 캠페인을 열고 동시 요청 중 한 명만 받는다. | 서버·클라이언트 | 자동검증(서버): `a_hundred_simultaneous_claims_on_the_last_coupon_produce_exactly_one` (`tests/campaigns.rs`). 자동검증(클라이언트, 받기 UX): `store-detail-state.spec.ts` — `성공하면 낙관적 표시를 유지하고 지갑 쿠폰을 연결한다` / `중복 요청은 서버가 준 기존 쿠폰으로 안내한다` / `소진되면 낙관적 표시를 되돌리고 코드를 보여 준다` / `연타해도 동일 멱등키를 유지한다` |
| 6 | 만료 시각과 사용 승인이 경합해도 만료된 쿠폰이 사용되지 않는다. | 서버 통합 테스트 | 자동검증(서버): `an_expiring_reservation_and_a_confirmation_settle_on_exactly_one_outcome` (`tests/campaigns.rs`) |
| 7 | 같은 캠페인 발급 작업을 여러 API·워커가 처리해도 동일 작업은 동시에 하나만 실행되고 고객별 발급도 한 번뿐이다. | 서버 통합 테스트 | 자동검증(서버): `an_advisory_lock_blocks_a_second_worker_and_is_released_when_the_first_goes_away`, `concurrent_registrations_of_one_key_still_produce_one_job`, `a_job_interrupted_mid_run_resumes_from_its_checkpoint` (`tests/campaigns.rs`) |
| 8 | FCM·알림톡이 실패해도 쿠폰은 지갑에 존재하고 재시도·실패 상태가 운영 화면에 나타난다. | 서버 통합 테스트 | 자동검증(서버, 지갑 유지·실패 상태): `a_permanent_provider_failure_leaves_the_reward_untouched`, `clearing_the_inbox_leaves_the_ledger_alone`, `every_provider_status_maps_onto_the_15_4_vocabulary` (`tests/notifications.rs`). 운영 관리자 화면에 실패/재시도가 보인다는 **클라이언트 테스트는 없음** (`admin-notification-operations.component.ts` UI는 있으나 `*.spec.ts` 없음) → 그 부분은 미검증 |
| 9 | 점주가 잘못 적립한 거래를 취소하고 관련 리워드 상태에 맞게 원장과 잔액이 보정된다. | 서버 통합 테스트 | 자동검증(서버): `reversing_an_accrual_takes_back_the_reward_and_restores_the_stamps` (`tests/loyalty.rs`) |
| 10 | 시스템 관리자가 민원 증거를 조회해 보정하고 모든 행위가 변경 불가능한 감사 로그에 남는다. | 서버 통합 테스트 | 자동검증(서버): `an_administrator_can_follow_one_transaction_end_to_end` (`tests/loyalty.rs`), `a_ledger_correction_needs_a_second_administrator_and_runs_as_a_job` (`tests/campaigns.rs`), `a_case_carries_its_resolution_and_its_audit_trail`, `a_tampered_audit_entry_is_reported_as_broken` (`tests/operations.rs`) |

브라우저 E2E(Playwright 등)로 §20 10개를 한 번에 도는 스위트는 이 워크트리에 없다(`apps/coupon-api-server/tests/` 재확인: 신규 파일 없음. `api.rs` 의 `a_page_size_is_clamped_rather_than_refused` 등은 §20 시나리오가 아님). 다른 에이전트가 추가하면 이 표의 「검증 방법」에 테스트 이름을 보탠다. 이름이 `grep` 으로 안 나오면 `미검증`을 유지한다.

## 실기기 검증

기획서 §19.5. 자동화 대상이 아니다. **사람이** [device-test-guide.md](./device-test-guide.md) 를 손에 들고 수행한다.

절차·URL·계정·인증서·증적 칸·실패 보고는 그 문서에만 둔다. 여기서는 **어떤 항목을 실기기에서 봐야 하는지**만 체크한다.

| 체크 | 가이드 | 실기기에서 확인할 것 | 근거 |
|---|---|---|---|
| [ ] | [D-01](./device-test-guide.md) | Android Chrome — 소비자 QR, 60초 타이머, 30초 자동 갱신, 8자리 보조 코드 | §6.2, WALLET-003 |
| [ ] | [D-02](./device-test-guide.md) | iOS Safari — 소비자 QR, 백그라운드→복귀 후 갱신 또는 만료 UI | §6.2, WALLET-004 |
| [ ] | [D-03](./device-test-guide.md) | Android Chrome — 점주 카메라 권한 허용, 후면 카메라, QR 스캔 resolve | §7, §6.3 |
| [ ] | [D-04](./device-test-guide.md) | 카메라 권한 거부 후 8자리 수동 입력으로 동일 고객 resolve | STORE-005 |
| [ ] | [D-05](./device-test-guide.md) | 오프라인 전환 시 QR 대신 「인터넷 연결이 필요해요」와 해결 버튼 | §6.2 |
| [ ] | [D-06](./device-test-guide.md) | 승인 응답 유실 후 **`처리 결과 확인`** (새 요청 없음, 같은 멱등키) | REDEEM-006, §6.3 |
| [ ] | [D-07](./device-test-guide.md) | 두 기기/탭 QR이 서로 즉시 무효화되지 않음 | WALLET-004 |
| [ ] | [D-08](./device-test-guide.md) | QR 화면 wake lock·흰 배경이 떠나면 원복 | §6.2 |
| [ ] | [D-09](./device-test-guide.md) | 360px 및 실기기에서 소비자·점주 주요 본문 가로 스크롤 없음 | §7 |
| [ ] | [D-10](./device-test-guide.md) | 성공/실패가 아이콘·제목·설명·거래 ID로 구분 | §7, §6.3 |

시드 계정·HTTPS 인증서 절차가 비어 있으면 가이드 §0 대로 `미정`을 남기고, 빈칸을 추측으로 채우지 않는다.
