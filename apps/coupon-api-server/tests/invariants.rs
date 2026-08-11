//! §12.6 주요 불변식 10개를 **데이터베이스에** 대고 하나씩 확인한다.
//!
//! Every other suite reaches an invariant through the API, which proves the handler is
//! careful. This one goes around the handler entirely and writes the violation in raw SQL,
//! because §12.6 is not a claim about handlers — it is a claim about the schema. A rule the
//! application enforces is one refactor away from being unenforced; a rule the database
//! enforces survives the refactor, the migration script, the admin with psql open and the
//! restore that replays somebody's ad-hoc fix.
//!
//! Where the schema does *not* enforce a rule the test says so plainly rather than quietly
//! testing the handler instead: knowing which of the ten live in application code is the
//! point of the exercise. Each such case still asserts the application guard, so the rule
//! is covered either way — just not at the same depth.
//!
//! Attempts that would succeed are rolled back; the schema is the subject here, not the
//! data.

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Raw-SQL helpers
// ---------------------------------------------------------------------------

/// The SQLSTATE of a database error, or `""` for anything else.
fn sqlstate(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(db) => db.code().unwrap_or_default().into_owned(),
        _ => String::new(),
    }
}

const UNIQUE_VIOLATION: &str = "23505";
const CHECK_VIOLATION: &str = "23514";

/// Copy one row of `table`, apply `set`, and try to insert the copy. Returns the refusal.
///
/// Duplicating a row through a temporary table rather than writing out twenty columns by
/// hand keeps these tests about the constraint under test instead of about the schema's
/// current shape — a new NOT NULL column must not break an invariant test.
///
/// The whole thing runs inside a transaction that is never committed, so a constraint that
/// turns out *not* to hold leaves nothing behind either.
async fn duplicating_a_row(
    pool: &PgPool,
    table: &str,
    id: Uuid,
    set: &[&str],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await.expect("begin");

    sqlx::query(&format!(
        "CREATE TEMP TABLE row_clone ON COMMIT DROP AS SELECT * FROM {table} WHERE id = $1"
    ))
    .bind(id)
    .execute(&mut *tx)
    .await
    .expect("copy the row");

    let copied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM row_clone")
        .fetch_one(&mut *tx)
        .await
        .expect("count the copy");
    assert_eq!(copied, 1, "{table}: nothing was copied, so nothing is proven");

    let mut assignments = vec!["id = public.gen_random_uuid()".to_owned()];
    assignments.extend(set.iter().map(|fragment| (*fragment).to_owned()));
    sqlx::query(&format!("UPDATE row_clone SET {}", assignments.join(", ")))
        .execute(&mut *tx)
        .await
        .expect("re-key the copy");

    sqlx::query(&format!("INSERT INTO {table} SELECT * FROM row_clone"))
        .execute(&mut *tx)
        .await
        .map(|_| ())
    // `tx` is dropped here, rolling back whichever way it went.
}

// ---------------------------------------------------------------------------
// 1. stores.owner_user_id 는 활성 상점 기준 유일하다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_1_one_live_store_per_owner() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv1").await;

    let refused = duplicating_a_row(
        &harness.pool,
        "coupon.stores",
        shop.id,
        &["slug = 'inv1-' || substr(public.gen_random_uuid()::text, 1, 20)"],
    )
    .await
    .expect_err("uq_stores_active_owner must refuse a second live store");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);

    // The index is partial on `status <> 'CLOSED'`, which is what makes 폐점 후 재개업
    // possible (STORE-004). The rule is "one *live* store", not "one store ever".
    duplicating_a_row(
        &harness.pool,
        "coupon.stores",
        shop.id,
        &[
            "slug = 'inv1-' || substr(public.gen_random_uuid()::text, 1, 20)",
            "status = 'CLOSED'",
            "closed_at = clock_timestamp()",
        ],
    )
    .await
    .expect("a closed store does not occupy the owner's slot");

    // …and the API agrees, rather than the two guards disagreeing.
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/store",
        &shop.owner_uid,
        Some(json!({
            "name": "두 번째 가게",
            "slug": format!("inv1b-{}", Uuid::new_v4().simple()),
        })),
    )
    .await
    .expect_error(
        StatusCode::CONFLICT,
        "STORE_ALREADY_EXISTS",
        "§12.6-1 through the front door",
    );
}

// ---------------------------------------------------------------------------
// 2. 상점당 활성 도장 정책은 최대 1개다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_2_one_active_loyalty_policy_per_store() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv2").await;
    let policy_id = publish_policy(&harness, &shop.owner_uid, default_rules()).await;

    let refused = duplicating_a_row(
        &harness.pool,
        "coupon.loyalty_policies",
        policy_id,
        &["version_no = version_no + 1000"],
    )
    .await
    .expect_err("uq_loyalty_policies_active_store must refuse a second ACTIVE policy");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);

    // An ended version is not an active one, so policy history is unaffected (STAMP-008).
    duplicating_a_row(
        &harness.pool,
        "coupon.loyalty_policies",
        policy_id,
        &["version_no = version_no + 1000", "status = 'ENDED'"],
    )
    .await
    .expect("an ended version does not occupy the store's active slot");
}

// ---------------------------------------------------------------------------
// 3. 도장 lot 의 누적 소비량은 원 적립량을 초과할 수 없다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_3_a_lot_is_never_consumed_past_what_it_earned() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv3").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "inv3-customer").await;
    earn_a_stamp(&harness, &shop, &customer.uid).await;

    let (lot_id, capacity): (Uuid, i16) = sqlx::query_as(
        "SELECT id, original_quantity FROM coupon.stamp_lots
         WHERE store_id = $1 AND user_id = $2 ORDER BY earned_at DESC LIMIT 1",
    )
    .bind(shop.id)
    .bind(customer.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("the accrual made a lot");

    // Both directions are guarded, because both are ways to invent stamps: spending more
    // than the lot held, and topping the lot up after the fact.
    for (label, event_type, delta) in [
        ("spending more than it holds", "EXPIRE", -(capacity + 1)),
        ("topping it up afterwards", "EARN", capacity + 1),
    ] {
        let mut tx = harness.pool.begin().await.expect("begin");
        let refused = sqlx::query(
            "INSERT INTO coupon.stamp_ledger
                 (lot_id, event_type, quantity_delta, actor_type, reason_code, occurred_at)
             VALUES ($1, $2::text::coupon.stamp_ledger_event_type, $3,
                     'SYSTEM'::coupon.actor_type, 'INVARIANT_PROBE', clock_timestamp())",
        )
        .bind(lot_id)
        .bind(event_type)
        .bind(delta)
        .execute(&mut *tx)
        .await
        .expect_err(&format!("§12.6-3 must refuse {label}"));

        assert_eq!(sqlstate(&refused), CHECK_VIOLATION, "{label}");
    }
}

// ---------------------------------------------------------------------------
// 4. 캠페인 발급 수량은 총수량과 일일수량을 넘을 수 없다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_4_issuance_never_exceeds_the_total_or_the_daily_quantity() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv4").await;

    // ---- 총수량: a CHECK on the campaign row itself. ----
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 2 })),
    )
    .await;

    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query(
        "UPDATE coupon.campaigns SET global_issued_count = total_quantity + 1 WHERE id = $1",
    )
    .bind(campaign_id)
    .execute(&mut *tx)
    .await
    .expect_err("ck_campaign_global_counts must refuse an over-issued campaign");
    assert_eq!(sqlstate(&refused), CHECK_VIOLATION);
    drop(tx);

    // ---- 일일수량: enforced by §13.2's locked counter, not by a constraint. ----
    //
    // `campaign_counters` has no CHECK against `per_business_day_quantity`, so the daily
    // ceiling lives in application code — inside the transaction that locks the counter
    // row, which is where §13.2 puts it. Worth knowing: a bad migration or a manual UPDATE
    // could push a day's count past the ceiling and the database would accept it.
    let daily_capped = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 50 })),
    )
    .await;
    sqlx::query("UPDATE coupon.campaigns SET per_business_day_quantity = 1 WHERE id = $1")
        .bind(daily_capped)
        .execute(&harness.pool)
        .await
        .expect("set a daily ceiling");

    let first = consumer(&harness.app, "inv4-first").await;
    let second = consumer(&harness.app, "inv4-second").await;
    let path = format!("/api/coupon/v1/campaigns/{daily_capped}/claims");

    send(&harness.app, "POST", &path, &first.uid, None)
        .await
        .expect_ok("the day's one coupon");
    let refused = send(&harness.app, "POST", &path, &second.uid, None).await;
    assert!(
        !refused.status.is_success(),
        "the daily ceiling has to hold somewhere: {}",
        refused.json,
    );

    let issued_today: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(issued_count), 0)::bigint FROM coupon.campaign_counters
         WHERE campaign_id = $1",
    )
    .bind(daily_capped)
    .fetch_one(&harness.pool)
    .await
    .expect("counters");
    assert_eq!(issued_today, 1);
}

// ---------------------------------------------------------------------------
// 5. 소비자별 인스턴스 수는 개인 한도를 넘을 수 없다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_5_a_customer_never_holds_more_than_their_personal_limit() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv5").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 50 })),
    )
    .await;
    let customer = consumer(&harness.app, "inv5-customer").await;

    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &customer.uid,
        None,
    )
    .await;
    let coupon_id = Uuid::parse_str(
        claim.expect_ok("claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    // The database owns the *ordinal*: no two instances of one campaign share a customer
    // and a position, so a retry can never become a second coupon.
    let refused = duplicating_a_row(&harness.pool, "coupon.coupon_instances", coupon_id, &[])
        .await
        .expect_err("uq_coupon_campaign_user_ordinal must refuse a duplicate ordinal");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);

    // The *ceiling* — `per_user_quantity` — is application code: the schema would accept a
    // second instance at ordinal 2 without complaint, and only §13.2's dedup row and limit
    // check stop it. So the ceiling is asserted through the API.
    duplicating_a_row(
        &harness.pool,
        "coupon.coupon_instances",
        coupon_id,
        &["issuance_ordinal = issuance_ordinal + 1"],
    )
    .await
    .expect("the schema does not know what the personal limit is");

    let repeat = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &customer.uid,
        None,
    )
    .await;
    assert_eq!(
        repeat.expect_ok("a repeat claim")["already_claimed"],
        true,
        "a second press returns the coupon they have rather than a second one",
    );

    let held: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1 AND user_id = $2",
    )
    .bind(campaign_id)
    .bind(customer.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(held, 1);
}

// ---------------------------------------------------------------------------
// 6. 쿠폰당 활성 예약은 최대 1개, 성공 사용은 최대 1개다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_6_one_live_reservation_and_one_successful_use_per_coupon() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv6").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    let customer = consumer(&harness.app, "inv6-customer").await;

    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &customer.uid,
        None,
    )
    .await;
    let coupon_id = Uuid::parse_str(
        claim.expect_ok("claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    let (token, _) = issue_qr(&harness.app, &customer.uid).await;
    let reservation = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &shop.owner_uid,
        Some(json!({
            "qr_token": token,
            "coupon_id": coupon_id,
            "owner_session_id": "inv6-till",
            "order": order(12_000),
        })),
    )
    .await;
    let reservation_id = Uuid::parse_str(
        reservation.expect_ok("reserve")["reservation_id"]
            .as_str()
            .expect("reservation id"),
    )
    .expect("uuid");

    let refused = duplicating_a_row(
        &harness.pool,
        "coupon.redemption_reservations",
        reservation_id,
        &[
            "owner_session_id = owner_session_id || '-clone'",
            "idempotency_key = public.gen_random_uuid()",
        ],
    )
    .await
    .expect_err("uq_redemption_reservations_active_coupon must refuse a second hold");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);

    let confirmed = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/redemptions/{reservation_id}/confirm"),
        &shop.owner_uid,
        Some(json!({ "owner_session_id": "inv6-till", "order": order(12_000) })),
    )
    .await;
    confirmed.expect_ok("confirm");

    // A second coupon, taken through the same flow, so there is a *real* confirmed
    // redemption to point at the first coupon. `redemption_transactions.reservation_id` is
    // NOT NULL and unique, so the probe has to move an existing row rather than invent one:
    // moving it is also the more honest test, because it is what a bad correction script
    // would actually do.
    let other = consumer(&harness.app, "inv6-other").await;
    let other_claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &other.uid,
        None,
    )
    .await;
    let other_coupon = Uuid::parse_str(
        other_claim.expect_ok("second claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    let (other_token, _) = issue_qr(&harness.app, &other.uid).await;
    let other_reservation = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/redemptions/preview",
        &shop.owner_uid,
        Some(json!({
            "qr_token": other_token,
            "coupon_id": other_coupon,
            "owner_session_id": "inv6-till-2",
            "order": order(12_000),
        })),
    )
    .await;
    let other_reservation_id = Uuid::parse_str(
        other_reservation.expect_ok("reserve the second coupon")["reservation_id"]
            .as_str()
            .expect("reservation id"),
    )
    .expect("uuid");

    let other_confirmed = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/owner/redemptions/{other_reservation_id}/confirm"),
        &shop.owner_uid,
        Some(json!({ "owner_session_id": "inv6-till-2", "order": order(12_000) })),
    )
    .await;
    let other_redemption = Uuid::parse_str(
        other_confirmed.expect_ok("confirm the second coupon")["redemption_id"]
            .as_str()
            .expect("redemption id"),
    )
    .expect("uuid");

    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query(
        "UPDATE coupon.redemption_transactions SET coupon_id = $2 WHERE id = $1",
    )
    .bind(other_redemption)
    .bind(coupon_id)
    .execute(&mut *tx)
    .await
    .expect_err("uq_redemption_transactions_confirmed_coupon must refuse a second use");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);
}

// ---------------------------------------------------------------------------
// 7. QR nonce 는 성공 거래에 최대 1회 연결된다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_7_a_nonce_is_linked_to_one_successful_transaction() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv7").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "inv7-customer").await;

    let transaction = earn_a_stamp(&harness, &shop, &customer.uid).await;
    let transaction_id = Uuid::parse_str(
        transaction["transaction_id"]
            .as_str()
            .expect("transaction id"),
    )
    .expect("uuid");

    let nonce_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM coupon.qr_nonces WHERE consumed_transaction_id = $1",
    )
    .bind(transaction_id)
    .fetch_one(&harness.pool)
    .await
    .expect("the accrual consumed a nonce");

    // Re-consuming: a one-way door, held shut by a trigger rather than by the `WHERE
    // consumed_at IS NULL` in the accrual path, which a refactor could drop.
    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query(
        "UPDATE coupon.qr_nonces
         SET consumed_at = clock_timestamp(), consumed_transaction_id = public.gen_random_uuid()
         WHERE id = $1",
    )
    .bind(nonce_id)
    .execute(&mut *tx)
    .await
    .expect_err("trg_qr_nonces_consume_once must refuse a second consumption");
    assert_eq!(sqlstate(&refused), "55000");
    drop(tx);

    // …and the other direction: one transaction cannot be credited with two nonces, which
    // is the shape a profitable replay would have to take.
    let (second_token, _) = issue_qr(&harness.app, &customer.uid).await;
    assert!(!second_token.is_empty());
    let second_nonce: Uuid = sqlx::query_scalar(
        "SELECT n.id FROM coupon.qr_nonces n
         WHERE n.user_id = $1 AND n.consumed_at IS NULL
         ORDER BY n.issued_at DESC LIMIT 1",
    )
    .bind(customer.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("a fresh nonce");

    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query(
        "UPDATE coupon.qr_nonces
         SET consumed_at = clock_timestamp(), consumed_transaction_id = $2
         WHERE id = $1",
    )
    .bind(second_nonce)
    .bind(transaction_id)
    .execute(&mut *tx)
    .await
    .expect_err("uq_qr_nonces_consumed_transaction must refuse a second nonce for one use");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);
}

// ---------------------------------------------------------------------------
// 8. 상태 이벤트의 이전 상태는 당시 인스턴스 상태와 같아야 한다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_8_a_status_event_agrees_with_the_instance_it_describes() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv8").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    let customer = consumer(&harness.app, "inv8-customer").await;

    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &customer.uid,
        None,
    )
    .await;
    let coupon_id = Uuid::parse_str(
        claim.expect_ok("claim")["coupon_id"]
            .as_str()
            .expect("coupon id"),
    )
    .expect("uuid");

    // The coupon is AVAILABLE. An event claiming it became USED, with nothing having moved
    // it, is refused: the event log cannot say something the instance does not.
    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query(
        "INSERT INTO coupon.coupon_status_events
             (coupon_id, from_status, to_status, actor_type, reason_code, occurred_at)
         VALUES ($1, 'AVAILABLE', 'USED', 'SYSTEM', 'FABRICATED', clock_timestamp())",
    )
    .bind(coupon_id)
    .execute(&mut *tx)
    .await
    .expect_err("trg_coupon_status_events_chain must refuse a fabricated event");
    assert_eq!(sqlstate(&refused), CHECK_VIOLATION);
    drop(tx);

    // The instance's own transitions are a state machine, not a free-form column.
    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query("UPDATE coupon.coupon_instances SET status = 'PENDING' WHERE id = $1")
        .bind(coupon_id)
        .execute(&mut *tx)
        .await
        .expect_err("trg_coupon_instances_transition must refuse AVAILABLE → PENDING");
    assert_eq!(sqlstate(&refused), CHECK_VIOLATION);
    drop(tx);

    // And an event that is not a transition at all is refused by a plain CHECK.
    let mut tx = harness.pool.begin().await.expect("begin");
    let refused = sqlx::query(
        "INSERT INTO coupon.coupon_status_events
             (coupon_id, from_status, to_status, actor_type, reason_code, occurred_at)
         VALUES ($1, 'AVAILABLE', 'AVAILABLE', 'SYSTEM', 'NOOP', clock_timestamp())",
    )
    .bind(coupon_id)
    .execute(&mut *tx)
    .await
    .expect_err("ck_coupon_status_changed must refuse a non-transition");
    assert_eq!(sqlstate(&refused), CHECK_VIOLATION);
}

// ---------------------------------------------------------------------------
// 9. 동일 멱등키와 actor 의 요청 본문 hash 가 다르면 재사용 오류다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_9_one_idempotency_key_per_actor_and_operation() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv9").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "inv9-customer").await;
    earn_a_stamp(&harness, &shop, &customer.uid).await;

    let record_id: Uuid = sqlx::query_scalar(
        "SELECT r.id FROM coupon.idempotency_requests r
         JOIN coupon.users u ON u.id = r.actor_user_id
         WHERE u.firebase_uid = $1
         ORDER BY r.created_at DESC LIMIT 1",
    )
    .bind(&shop.owner_uid)
    .fetch_one(&harness.pool)
    .await
    .expect("the accrual recorded its idempotency key");

    // The *slot* is the database's: one (actor, operation, key) row, ever.
    let refused = duplicating_a_row(&harness.pool, "coupon.idempotency_requests", record_id, &[])
        .await
        .expect_err("uq_idempotency_request must refuse a second record for one key");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);

    // The *comparison* — same key, different body hash — is application code reading that
    // row, and cannot be a constraint: a constraint has nothing to compare against. So the
    // rule itself is asserted through the API.
    let (token, _) = issue_qr(&harness.app, &customer.uid).await;
    let key = Uuid::new_v4();
    send_with_key(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &shop.owner_uid,
        Some(key),
        // A distinct amount: the accrual above used 12,000 and §8.6's near-duplicate
        // warning would refuse this one for that instead of for the key.
        Some(json!({ "qr_token": token, "order": order(33_000) })),
    )
    .await
    .expect_ok("the first request");

    let conflicting = send_with_key(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &shop.owner_uid,
        Some(key),
        Some(json!({ "qr_token": token, "order": order(99_000) })),
    )
    .await;
    conflicting.expect_error(
        StatusCode::CONFLICT,
        "IDEMPOTENCY_KEY_REUSED",
        "§12.6-9: a different body under the same key is a reuse error",
    );
}

// ---------------------------------------------------------------------------
// 10. 동일 job unique key 의 QUEUED/RUNNING/RETRY_WAIT 작업은 최대 1개다
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariant_10_one_active_job_per_unique_key() {
    let harness = harness_or_skip!();
    let shop = store(&harness, "inv10").await;

    // Publishing a campaign registers its audience job, which is a real active job with a
    // real unique key.
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("DIRECT", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;

    let job_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM coupon.job_registry
         WHERE resource_id = $1
           AND status IN ('PENDING_OUTBOX','QUEUED','RUNNING','RETRY_WAIT','PAUSE_REQUESTED','PAUSED')
         ORDER BY created_at LIMIT 1",
    )
    .bind(campaign_id)
    .fetch_one(&harness.pool)
    .await
    .expect("publishing registered a job");

    let refused = duplicating_a_row(
        &harness.pool,
        "coupon.job_registry",
        job_id,
        &["generation = generation + 1000"],
    )
    .await
    .expect_err("uq_job_registry_active_key must refuse a second active job");
    assert_eq!(sqlstate(&refused), UNIQUE_VIOLATION);

    // The index is partial on the active states, which is exactly what lets the *next*
    // generation of the same key be registered once this one has finished (§14.7).
    duplicating_a_row(
        &harness.pool,
        "coupon.job_registry",
        job_id,
        &[
            "generation = generation + 1000",
            "status = 'SUCCEEDED'",
            "finished_at = clock_timestamp()",
        ],
    )
    .await
    .expect("a finished job does not occupy the key");
}
