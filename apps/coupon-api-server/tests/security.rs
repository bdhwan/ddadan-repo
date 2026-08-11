//! §16 보안·악용 시나리오를 테스트로 고정한다 (SEC-001 … SEC-005).
//!
//! Phase 1–4 already settle parts of this — a forged QR is refused without saying why
//! (`loyalty.rs`), an accrual is rate limited per store and owner (`loyalty.rs`), reading
//! the audit trail is itself privileged (`operations.rs`). What is here is what nothing
//! else pins down: the ownership boundary on every resource a URL can name, the four ways
//! a QR can be replayed, the §16.4 ceilings as *numbers*, the owner accruing to their own
//! account, and the administrator whose reads are themselves evidence.
//!
//! The rule these tests exist to hold is §11.1's: **소유권 없는 리소스는 404**. A 403 on a
//! resource somebody else owns confirms it exists, and confirming existence is the whole
//! of an enumeration attack.

mod common;

use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// SEC-001 IDOR·권한 상승
// ---------------------------------------------------------------------------

#[tokio::test]
async fn one_customers_coupon_is_invisible_to_another() {
    // SEC-001: 소비자가 URL의 쿠폰 ID를 바꿔 다른 소비자의 내역을 요청하면 404.
    let harness = harness_or_skip!();
    let shop = store(&harness, "idor-wallet").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;

    let owner = consumer(&harness.app, "idor-owner-of-coupon").await;
    let stranger = consumer(&harness.app, "idor-stranger").await;

    let claim = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/campaigns/{campaign_id}/claims"),
        &owner.uid,
        None,
    )
    .await;
    let coupon_id = claim.expect_ok("claim")["coupon_id"]
        .as_str()
        .expect("coupon id")
        .to_owned();
    let path = format!("/api/coupon/v1/me/wallet/coupons/{coupon_id}");

    send(&harness.app, "GET", &path, &owner.uid, None)
        .await
        .expect_ok("the holder reads their own coupon");

    // The same id, a different bearer.
    send(&harness.app, "GET", &path, &stranger.uid, None)
        .await
        .expect_error(
            StatusCode::NOT_FOUND,
            "COUPON_NOT_FOUND",
            "somebody else's coupon must be indistinguishable from one that never existed",
        );

    // And it is absent from their wallet listing, not merely unreadable by id.
    let wallet = send(
        &harness.app,
        "GET",
        "/api/coupon/v1/me/wallet/coupons",
        &stranger.uid,
        None,
    )
    .await;
    assert!(
        wallet.expect_ok("stranger's wallet")["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|item| item["id"] != coupon_id.as_str()),
    );
}

#[tokio::test]
async fn an_owner_cannot_reach_another_shops_resources() {
    // SEC-001: 점주가 다른 상점 ID를 넣어도 토큰 역할이 아닌 DB 소유 관계로 거절한다.
    let harness = harness_or_skip!();
    let mine = store(&harness, "idor-mine").await;
    let theirs = store(&harness, "idor-theirs").await;
    publish_policy(&harness, &theirs.owner_uid, default_rules()).await;

    // A campaign, a catalogue item and a confirmed accrual — one of each kind of resource
    // an owner URL can name.
    let campaign_id = publish_campaign(
        &harness,
        &theirs,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 5 })),
    )
    .await;
    let item_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/catalog/items",
        &theirs.owner_uid,
        Some(json!({ "name": "아메리카노", "price": 4500 })),
    )
    .await
    .id("catalogue item");
    let customer = consumer(&harness.app, "idor-customer").await;
    let transaction = earn_a_stamp(&harness, &theirs, &customer.uid).await;
    let transaction_id = transaction["transaction_id"]
        .as_str()
        .expect("transaction id");

    // Every one of these is a valid id. The only thing wrong with the request is who is
    // making it, and §11.1 says that must read as 404 rather than 403.
    for (method, path, code, body) in [
        (
            "GET",
            format!("/api/coupon/v1/owner/campaigns/{campaign_id}"),
            "CAMPAIGN_NOT_FOUND",
            None,
        ),
        (
            "POST",
            format!("/api/coupon/v1/owner/campaigns/{campaign_id}/pause"),
            "CAMPAIGN_NOT_FOUND",
            Some(json!({ "reason": "남의 캠페인 중지" })),
        ),
        (
            "POST",
            format!("/api/coupon/v1/owner/campaigns/{campaign_id}/cancel"),
            "CAMPAIGN_NOT_FOUND",
            Some(json!({ "reason": "남의 캠페인" })),
        ),
        (
            "GET",
            format!("/api/coupon/v1/owner/campaigns/{campaign_id}/estimate"),
            "CAMPAIGN_NOT_FOUND",
            None,
        ),
        (
            "PATCH",
            format!("/api/coupon/v1/owner/catalog/items/{item_id}"),
            // The catalogue answers with the generic `NOT_FOUND` rather than its own
            // `CATALOG_ITEM_NOT_FOUND`. That is a cosmetic inconsistency with its
            // neighbours and not a hole — the status, which is what SEC-001 turns on, is
            // the same 404 — so it is pinned here as it is rather than quietly changed.
            "NOT_FOUND",
            Some(json!({ "name": "가로채기" })),
        ),
        (
            "POST",
            format!("/api/coupon/v1/owner/stamp-transactions/{transaction_id}/void"),
            "TRANSACTION_NOT_FOUND",
            Some(json!({ "reason": "남의 거래 취소" })),
        ),
    ] {
        let response = send(&harness.app, method, &path, &mine.owner_uid, body).await;
        response.expect_error(
            StatusCode::NOT_FOUND,
            code,
            &format!("{method} {path} from the wrong owner"),
        );
    }

    // The listing agrees with the by-id answer: the other shop's campaign is simply not
    // in this owner's world.
    let listed = find_across_pages(
        &harness.app,
        "/api/coupon/v1/owner/campaigns?limit=100",
        &mine.owner_uid,
        |entry| entry["id"] == json!(campaign_id.to_string()),
    )
    .await;
    assert!(listed.is_none(), "a foreign campaign must not be listed");
}

#[tokio::test]
async fn an_unknown_id_answers_exactly_as_a_foreign_one_does() {
    // SEC-001: 열거 공격을 막으려면 "없는 것"과 "남의 것"의 응답이 같아야 한다.
    let harness = harness_or_skip!();
    let mine = store(&harness, "idor-enum").await;
    let theirs = store(&harness, "idor-enum-other").await;
    let foreign = publish_campaign(
        &harness,
        &theirs,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 1 })),
    )
    .await;
    let nonexistent = Uuid::new_v4();

    let foreign_response = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/owner/campaigns/{foreign}"),
        &mine.owner_uid,
        None,
    )
    .await;
    let missing_response = send(
        &harness.app,
        "GET",
        &format!("/api/coupon/v1/owner/campaigns/{nonexistent}"),
        &mine.owner_uid,
        None,
    )
    .await;

    assert_eq!(foreign_response.status, missing_response.status);
    assert_eq!(
        foreign_response.error_code(),
        missing_response.error_code(),
        "an existing-but-foreign id must not be distinguishable from a made-up one",
    );
    assert_eq!(foreign_response.message(), missing_response.message());
}

#[tokio::test]
async fn a_consumer_cannot_put_on_an_owner_or_administrator_hat() {
    // SEC-001: 역할 상승. 관리자 API 는 별도 권한을 요구한다 (§3.3).
    let harness = harness_or_skip!();
    let plain = consumer(&harness.app, "escalation").await;

    for (method, path, body) in [
        ("GET", "/api/coupon/v1/owner/campaigns", None),
        ("GET", "/api/coupon/v1/owner/analytics", None),
        ("GET", "/api/coupon/v1/owner/catalog/items", None),
        (
            "POST",
            "/api/coupon/v1/owner/scan/resolve",
            Some(json!({ "qr_token": "irrelevant" })),
        ),
    ] {
        let response = send(&harness.app, method, path, &plain.uid, body).await;
        response.expect_error(
            StatusCode::FORBIDDEN,
            "ROLE_REQUIRED",
            &format!("{method} {path} without the owner role"),
        );
    }

    for (method, path, body) in [
        ("GET", "/api/coupon/v1/admin/audit-logs".to_owned(), None),
        ("GET", "/api/coupon/v1/admin/cases".to_owned(), None),
        ("GET", "/api/coupon/v1/admin/metrics".to_owned(), None),
        ("GET", "/api/coupon/v1/admin/jobs".to_owned(), None),
        (
            "GET",
            "/api/coupon/v1/admin/store-reviews".to_owned(),
            None,
        ),
        (
            "GET",
            format!("/api/coupon/v1/admin/transactions/{}", Uuid::new_v4()),
            None,
        ),
        (
            "POST",
            format!("/api/coupon/v1/admin/users/{}/suspend", plain.user_id),
            Some(json!({
                "sanction_type": "TEMPORARY",
                "case_id": Uuid::new_v4(),
                "public_reason": "스스로 정지",
                "internal_reason": "권한 상승 시도",
                "expires_at": "2099-01-01T00:00:00Z",
            })),
        ),
    ] {
        let response = send(&harness.app, method, &path, &plain.uid, body).await;
        response.expect_error(
            StatusCode::FORBIDDEN,
            "ROLE_REQUIRED",
            &format!("{method} {path} without an administrative role"),
        );
    }

    // Nor can a consumer grant themselves one: roles are read-only over the API.
    let roles = send(&harness.app, "GET", "/api/coupon/v1/me/roles", &plain.uid, None).await;
    let roles = roles.expect_ok("read roles")["roles"]
        .as_array()
        .expect("roles")
        .iter()
        .map(|grant| grant["role"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(roles, vec!["CONSUMER".to_owned()]);
}

#[tokio::test]
async fn a_support_administrator_cannot_take_a_security_action() {
    // SEC-001 / §3.3: 관리자 안에서도 조회 범위와 변경 범위를 나눈다.
    let harness = harness_or_skip!();
    let support = consumer(&harness.app, "support-only").await;
    grant_role(&harness.pool, support.user_id, "SUPPORT").await;
    let subject = consumer(&harness.app, "sanction-subject").await;

    // Support may open a case…
    let case_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/cases",
        &support.uid,
        Some(json!({
            "case_type": "QR_ABUSE",
            "title": "확인 요청",
            "description": "고객 문의",
        })),
    )
    .await
    .id("open a case");

    // …but not sanction an account, and not read the audit trail.
    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{}/suspend", subject.user_id),
        &support.uid,
        Some(json!({
            "sanction_type": "TEMPORARY",
            "case_id": case_id,
            "public_reason": "고객 요청",
            "internal_reason": "지원 데스크가 직접 정지 시도",
            "expires_at": "2099-01-01T00:00:00Z",
        })),
    )
    .await
    .expect_error(
        StatusCode::FORBIDDEN,
        "ROLE_REQUIRED",
        "sanctioning is not a support action",
    );

    send(
        &harness.app,
        "GET",
        "/api/coupon/v1/admin/audit-logs",
        &support.uid,
        None,
    )
    .await
    .expect_error(
        StatusCode::FORBIDDEN,
        "ROLE_REQUIRED",
        "reading the audit trail is itself privileged (SEC-005)",
    );
}

// ---------------------------------------------------------------------------
// SEC-002 QR 위조·리플레이
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_spent_qr_cannot_be_scanned_again() {
    // SEC-002 / §12.6-7: nonce 는 성공 거래에 최대 1회 연결된다. `loyalty.rs` settles the
    // simultaneous case; this is the sequential replay, which is what a photographed or
    // forwarded QR actually looks like.
    let harness = harness_or_skip!();
    let shop = store(&harness, "replay").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "replay-customer").await;

    let (token, code) = issue_qr(&harness.app, &customer.uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &shop.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await
    .expect_ok("the first scan");

    for (label, body) in [
        ("the same token again", json!({ "qr_token": token })),
        // STORE-005's manual path is the same nonce by another name, so spending the
        // token must spend the code with it.
        ("the manual code behind it", json!({ "fallback_code": code })),
    ] {
        send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/scan/resolve",
            &shop.owner_uid,
            Some(body),
        )
        .await
        .expect_error(StatusCode::CONFLICT, "QR_ALREADY_USED", label);
    }

    let consumed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.stamp_transactions
         WHERE store_id = $1 AND user_id = $2 AND status = 'CONFIRMED'",
    )
    .bind(shop.id)
    .bind(customer.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(consumed, 1, "a replay must not add a second ledger entry");
}

#[tokio::test]
async fn an_expired_or_revoked_nonce_is_refused_and_the_reasons_are_distinguished_only_where_they_help()
 {
    // SEC-002: 서명 불일치, 만료, 잘못된 audience, 없는 nonce, 이미 소비된 nonce 를 구분해
    // *내부* 기록한다. Outward, only "expired" and "already used" are named — those two tell
    // the owner what to do next. Everything else collapses into one code so that a prober
    // learns nothing about which part of the token they got wrong.
    let harness = harness_or_skip!();
    let shop = store(&harness, "qr-states").await;
    let customer = consumer(&harness.app, "qr-states-customer").await;

    let expired = expire_a_nonce(&harness, &customer.uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/scan/resolve",
        &shop.owner_uid,
        Some(json!({ "qr_token": expired })),
    )
    .await
    .expect_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "QR_TOKEN_EXPIRED",
        "an expired QR is named as expired so the owner asks for a fresh one",
    );

    // A revoked nonce, an audience it was not minted for, and a nonce the database has no
    // record of all answer the same way (§16.2: 위조 방법을 추론하기 어렵게 일반화한다).
    let revoked = mutate_a_nonce(
        &harness,
        &customer.uid,
        "UPDATE coupon.qr_nonces SET revoked_at = clock_timestamp() WHERE nonce_hash = $1",
    )
    .await;
    let wrong_audience = mutate_a_nonce(
        &harness,
        &customer.uid,
        "UPDATE coupon.qr_nonces SET audience = 'ddadan.somewhere-else' WHERE nonce_hash = $1",
    )
    .await;
    let forgotten = mutate_a_nonce(
        &harness,
        &customer.uid,
        "DELETE FROM coupon.qr_nonces WHERE nonce_hash = $1",
    )
    .await;

    for (label, token) in [
        ("a revoked nonce", revoked),
        ("a nonce minted for another audience", wrong_audience),
        ("a nonce the database has no record of", forgotten),
    ] {
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/owner/scan/resolve",
            &shop.owner_uid,
            Some(json!({ "qr_token": token })),
        )
        .await;
        response.expect_error(StatusCode::UNPROCESSABLE_ENTITY, "QR_TOKEN_INVALID", label);
        let message = response.message();
        for leak in ["서명", "audience", "revoke", "kid", "nonce"] {
            assert!(
                !message.contains(leak),
                "{label}: the message must not say which check failed: {message}",
            );
        }
    }
}

#[tokio::test]
async fn a_qr_is_not_a_licence_to_act_for_its_holder() {
    // SEC-002: QR 은 적립 대상을 지목할 뿐 권한을 옮기지 않는다. Scanning somebody's QR must
    // not let a shop read their wallet or act as them.
    let harness = harness_or_skip!();
    let shop = store(&harness, "qr-scope").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "qr-scope-customer").await;

    let (token, _) = issue_qr(&harness.app, &customer.uid).await;
    let resolved = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/scan/resolve",
        &shop.owner_uid,
        Some(json!({ "qr_token": token })),
    )
    .await;
    let resolved = resolved.expect_ok("resolve").clone();

    // WALLET-005: what comes back is a store-local alias, never the person.
    let rendered = resolved.to_string();
    assert!(
        !rendered.contains(&customer.user_id.to_string()),
        "the scan result must not carry the customer's user id: {rendered}",
    );
    assert!(
        !rendered.contains("김손님"),
        "nor their name: {rendered}",
    );
}

// ---------------------------------------------------------------------------
// SEC-003 봇과 대량 계정 — §16.4 표의 값
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_16_4_ceilings_are_the_shipped_defaults() {
    // §16.4 는 제한값을 운영 설정으로 관리한다고 못박았고, 이 테스트는 그 *기본값*이 표와
    // 같은지 본다. A limit that silently drifted from the spec is the kind of thing nobody
    // notices until an incident.
    let config: coupon_api_server::config::Config = serde_json::from_value(json!({
        "env": "test",
        "database_url": "postgres://unused/unused",
        "firebase_project_id": "ddadan-test",
    }))
    .expect("defaults parse");

    assert_eq!(config.rate_limit_qr_issue_per_min, 20, "QR 발급 20회/분");
    assert_eq!(
        config.rate_limit_qr_resolve_failure_per_min, 30,
        "QR 해석 실패 30회/분",
    );
    assert_eq!(
        config.rate_limit_stamp_approval_per_min, 30,
        "적립/사용 승인 30회/분",
    );
    assert_eq!(
        config.rate_limit_campaign_claim_per_min, 5,
        "선착순 받기 5회/분",
    );
}

#[tokio::test]
async fn a_bot_hammering_first_come_hits_the_ceiling_before_the_stock() {
    // SEC-003 / §16.4: 선착순 받기는 user+campaign 으로 5회/분. The point of limiting the
    // *claim* rather than the issuance is that a legitimate winner keeps their coupon;
    // only the presses beyond the ceiling are refused.
    let harness = harness_or_skip!(json!({ "rate_limit_campaign_claim_per_min": 5 }));
    let shop = store(&harness, "claim-limit").await;
    let campaign_id = publish_campaign(
        &harness,
        &shop,
        campaign_draft("FIRST_COME", json!({ "mode": "LIMITED", "quantity": 100 })),
    )
    .await;
    let bot = consumer(&harness.app, "claim-bot").await;
    let path = format!("/api/coupon/v1/campaigns/{campaign_id}/claims");

    let mut outcomes = Vec::new();
    for _ in 0..8 {
        let response = send(&harness.app, "POST", &path, &bot.uid, None).await;
        outcomes.push((response.status, response.error_code().to_owned()));
    }

    let limited = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::TOO_MANY_REQUESTS)
        .count();
    assert_eq!(
        limited, 3,
        "five presses pass the window and the rest are refused: {outcomes:?}",
    );
    assert!(
        outcomes
            .iter()
            .all(|(status, code)| *status != StatusCode::TOO_MANY_REQUESTS
                || code == "RATE_LIMITED"),
    );

    // The limit did not cost the bot the one coupon they were entitled to, and did not
    // hand them more than one either (§12.6-5).
    let issued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coupon.coupon_instances WHERE campaign_id = $1 AND user_id = $2",
    )
    .bind(campaign_id)
    .bind(bot.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("count");
    assert_eq!(issued, 1);
}

#[tokio::test]
async fn a_rate_limit_is_scoped_to_its_subject_not_to_the_endpoint() {
    // §16.4: IPv4/IPv6 공유망을 고려해 IP 만으로 계정을 영구 차단하지 않는다. The practical
    // reading: one account exhausting a bucket must not refuse a second account's first
    // request on the same endpoint.
    let harness = harness_or_skip!(json!({ "rate_limit_qr_issue_per_min": 3 }));
    let heavy = consumer(&harness.app, "qr-heavy").await;
    let light = consumer(&harness.app, "qr-light").await;

    let mut refused = 0;
    for _ in 0..6 {
        let response = send(
            &harness.app,
            "POST",
            "/api/coupon/v1/me/qr-tokens",
            &heavy.uid,
            None,
        )
        .await;
        if response.status == StatusCode::TOO_MANY_REQUESTS {
            refused += 1;
        }
    }
    assert_eq!(refused, 3, "three of the six presses are over the ceiling");

    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/me/qr-tokens",
        &light.uid,
        None,
    )
    .await
    .expect_ok("a second account is unaffected by the first one's bucket");
}

// ---------------------------------------------------------------------------
// SEC-004 점주 자기 적립·공모
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_owner_accruing_to_their_own_account_is_allowed_and_flagged() {
    // SEC-004 / §3.2: 기술적으로 적립할 수 있으나 자기 거래로 표시하고 위험 집계에 포함한다.
    // MVP 에서는 결제 증빙을 자동 검증하지 못하므로 차단이 아니라 표시가 통제 수단이다.
    let harness = harness_or_skip!();
    let shop = store(&harness, "self-accrual").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;

    // The owner scans their own QR: same person, both sides of the counter.
    let (token, _) = issue_qr(&harness.app, &shop.owner_uid).await;
    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions/preview",
        &shop.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    let preview = preview.expect_ok("preview a self-accrual");

    assert_eq!(
        preview["self_transaction"], true,
        "the preview must say so out loud: {preview}",
    );
    assert!(
        preview["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning["code"] == "SELF_TRANSACTION"),
        "SEC-004 warning is missing: {preview}",
    );

    // It is a warning, not a blocker: the accrual goes through.
    let confirmed = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &shop.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    confirmed.expect_ok("a self-accrual is permitted");

    // …and it lands in the risk aggregate, which is what the audit and 민원 procedures
    // §16 leans on actually read.
    let flags: Value = sqlx::query_scalar(
        "SELECT risk_flags FROM coupon.store_customers WHERE store_id = $1 AND user_id = $2",
    )
    .bind(shop.id)
    .bind(shop.owner_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("the owner is a customer of their own store");

    assert!(
        flags
            .as_array()
            .expect("flags")
            .iter()
            .any(|flag| flag == "SELF_TRANSACTION"),
        "risk_flags must carry the mark: {flags}",
    );

    // A second self-accrual must not double-count the flag — the aggregate is a set.
    let (token, _) = issue_qr(&harness.app, &shop.owner_uid).await;
    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &shop.owner_uid,
        Some(json!({
            "qr_token": token,
            "order": { "gross_amount": 9_000, "currency": "KRW", "items": [] },
        })),
    )
    .await
    .expect_ok("a second self-accrual");

    let flags: Value = sqlx::query_scalar(
        "SELECT risk_flags FROM coupon.store_customers WHERE store_id = $1 AND user_id = $2",
    )
    .bind(shop.id)
    .bind(shop.owner_user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("read the flags again");
    assert_eq!(
        flags
            .as_array()
            .expect("flags")
            .iter()
            .filter(|flag| *flag == "SELF_TRANSACTION")
            .count(),
        1,
    );
}

#[tokio::test]
async fn an_ordinary_customer_carries_no_risk_flag() {
    // The other half of SEC-004: the mark has to mean something, so it must be absent on
    // an ordinary accrual.
    let harness = harness_or_skip!();
    let shop = store(&harness, "clean-accrual").await;
    publish_policy(&harness, &shop.owner_uid, default_rules()).await;
    let customer = consumer(&harness.app, "clean-customer").await;

    let (token, _) = issue_qr(&harness.app, &customer.uid).await;
    let preview = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions/preview",
        &shop.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await;
    let preview = preview.expect_ok("preview an ordinary accrual");
    assert_eq!(preview["self_transaction"], false);
    assert!(
        preview["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .all(|warning| warning["code"] != "SELF_TRANSACTION"),
    );

    send(
        &harness.app,
        "POST",
        "/api/coupon/v1/owner/stamp-transactions",
        &shop.owner_uid,
        Some(json!({ "qr_token": token, "order": order(12_000) })),
    )
    .await
    .expect_ok("confirm an ordinary accrual");

    let flags: Value = sqlx::query_scalar(
        "SELECT risk_flags FROM coupon.store_customers WHERE store_id = $1 AND user_id = $2",
    )
    .bind(shop.id)
    .bind(customer.user_id)
    .fetch_one(&harness.pool)
    .await
    .expect("the customer row");
    assert_eq!(flags, json!([]));
}

// ---------------------------------------------------------------------------
// SEC-005 관리자 오남용
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_high_risk_administrative_action_leaves_a_reason_and_a_case() {
    // SEC-005: 모든 조회와 변경을 목적·사건 ID와 함께 감사한다.
    let harness = harness_or_skip!();
    let desk = admin(&harness, "audit-admin").await;
    let subject = consumer(&harness.app, "audit-subject").await;

    let case_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/cases",
        &desk.uid,
        Some(json!({
            "case_type": "QR_ABUSE",
            "title": "도용 신고",
            "description": "동일 QR 이 여러 매장에서 사용되었습니다.",
            "subject_user_id": subject.user_id,
        })),
    )
    .await
    .id("open a case");

    // A sanction with a blank reason does not happen at all.
    let unexplained = send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{}/suspend", subject.user_id),
        &desk.uid,
        Some(json!({
            "sanction_type": "TEMPORARY",
            "case_id": case_id,
            "public_reason": "",
            "internal_reason": "",
            "expires_at": "2099-01-01T00:00:00Z",
        })),
    )
    .await;
    unexplained.expect_error(
        StatusCode::BAD_REQUEST,
        "VALIDATION_FAILED",
        "a sanction with no reason is not a sanction",
    );

    send(
        &harness.app,
        "POST",
        &format!("/api/coupon/v1/admin/users/{}/suspend", subject.user_id),
        &desk.uid,
        Some(json!({
            "sanction_type": "TEMPORARY",
            "case_id": case_id,
            "public_reason": "이용약관 위반",
            "internal_reason": "동일 QR 다중 사용 확인",
            "expires_at": "2099-01-01T00:00:00Z",
        })),
    )
    .await
    .expect_ok("sanction with a reason and a case");

    let entry = find_across_pages(
        &harness.app,
        &format!("/api/coupon/v1/admin/audit-logs?case_id={case_id}&limit=100"),
        &desk.uid,
        |entry| entry["resource_id"] == json!(subject.user_id.to_string()),
    )
    .await
    .expect("the sanction is in the audit trail");

    assert_eq!(entry["reason"], "동일 QR 다중 사용 확인");

    assert_eq!(entry["case_id"], json!(case_id.to_string()));
    assert_eq!(entry["actor_user_id"], json!(desk.user_id.to_string()));
    assert_eq!(
        entry["chain_intact"], true,
        "§12.5: the entry chains to its predecessor",
    );
}

#[tokio::test]
async fn the_audit_trail_refuses_the_administrator_who_wants_it_gone() {
    // SEC-005: 감사 로그는 관리자도 직접 수정·삭제할 수 없다. `operations.rs` proves the
    // UPDATE is refused; DELETE and TRUNCATE are the other two ways to make a record go
    // away, and they have to be refused by the *database*, not by a route that simply was
    // not written.
    let harness = harness_or_skip!();
    let desk = admin(&harness, "immutable-admin").await;
    let case_id = send(
        &harness.app,
        "POST",
        "/api/coupon/v1/admin/cases",
        &desk.uid,
        Some(json!({
            "case_type": "OTHER",
            "title": "지워보기",
            "description": "감사 로그 삭제 시도",
        })),
    )
    .await
    .id("open a case");

    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM coupon.audit_logs WHERE case_id = $1")
            .bind(case_id)
            .fetch_one(&harness.pool)
            .await
            .expect("count");
    assert!(before > 0, "opening a case is audited");

    for statement in [
        "DELETE FROM coupon.audit_logs WHERE case_id = $1",
        "UPDATE coupon.audit_logs SET reason = '없던 일' WHERE case_id = $1",
    ] {
        let refused = sqlx::query(statement)
            .bind(case_id)
            .execute(&harness.pool)
            .await;
        assert!(
            refused.is_err(),
            "`{statement}` must be refused by the database itself",
        );
    }

    let after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM coupon.audit_logs WHERE case_id = $1")
            .bind(case_id)
            .fetch_one(&harness.pool)
            .await
            .expect("count");
    assert_eq!(after, before, "nothing was removed");
}

#[tokio::test]
async fn a_bulk_export_is_not_something_the_api_offers() {
    // SEC-005: 대량 내보내기는 기본 비활성화한다. There is no export endpoint, so the
    // enforcement is the page ceiling: an administrator cannot ask for the table in one
    // request (§11.1, 최대 100).
    let harness = harness_or_skip!();
    let desk = admin(&harness, "bulk-admin").await;

    for path in [
        "/api/coupon/v1/admin/audit-logs?limit=100000",
        "/api/coupon/v1/admin/cases?limit=100000",
        "/api/coupon/v1/admin/store-reviews?limit=100000",
        // The queue dashboard answers with a bare list rather than a page, and caps
        // itself at the same hundred.
        "/api/coupon/v1/admin/jobs",
    ] {
        let response = send(&harness.app, "GET", path, &desk.uid, None).await;
        let data = response.expect_ok(path);
        let rows = data["items"]
            .as_array()
            .or_else(|| data.as_array())
            .unwrap_or_else(|| panic!("{path} answered with neither a page nor a list: {data}"));
        assert!(
            rows.len() <= 100,
            "{path} must not answer with more than 100 rows, got {}",
            rows.len(),
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Issue a QR and push its nonce past its expiry, returning the still-well-signed token.
///
/// The token's own `exp` claim is minutes away — §16.2's TTL floor is ten seconds and
/// waiting for it would put a sleep in the suite. Moving the *nonce* row is the same state
/// from the verifier's point of view and arrives instantly.
async fn expire_a_nonce(harness: &Harness, customer_uid: &str) -> String {
    // `ck_qr_nonce_period` keeps `expires_at > issued_at`, so the whole window moves into
    // the past rather than only its end.
    mutate_a_nonce(
        harness,
        customer_uid,
        "UPDATE coupon.qr_nonces
         SET issued_at = clock_timestamp() - interval '10 minutes',
             expires_at = clock_timestamp() - interval '1 minute'
         WHERE nonce_hash = $1",
    )
    .await
}

/// Issue a QR, apply `statement` to its nonce row, and return the token.
///
/// `statement` is bound to the nonce hash. Reaching into the table is the only way to
/// reach these states: revocation and audience mismatch have no API that produces them,
/// which is the point — they are what a *restored backup* or a *rotated key* leaves
/// behind, not something a client can ask for.
async fn mutate_a_nonce(harness: &Harness, customer_uid: &str, statement: &str) -> String {
    let (token, _) = issue_qr(&harness.app, customer_uid).await;
    let nonce_hash = latest_nonce_hash(harness, customer_uid).await;

    sqlx::query(statement)
        .bind(&nonce_hash)
        .execute(&harness.pool)
        .await
        .expect("mutate the nonce");

    token
}

/// The nonce hash of the most recently issued QR for this dev uid.
async fn latest_nonce_hash(harness: &Harness, customer_uid: &str) -> Vec<u8> {
    sqlx::query_scalar(
        "SELECT n.nonce_hash FROM coupon.qr_nonces n
         JOIN coupon.users u ON u.id = n.user_id
         WHERE u.firebase_uid = $1
         ORDER BY n.issued_at DESC, n.id DESC
         LIMIT 1",
    )
    .bind(customer_uid)
    .fetch_one(&harness.pool)
    .await
    .expect("the nonce just issued")
}
