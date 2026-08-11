//! 쿠폰 인스턴스와 소비자 지갑 조회 (§10.2 `wallet`, §6.2, WALLET-002).
//!
//! Owns `coupon_instances` and `coupon_status_events`. Phase 2 fills in the read side:
//! the stamp boards and the reward coupons a completed board produced. `loyalty` creates
//! reward instances (STAMP-004 requires it to happen in the accrual's own transaction);
//! everything after issuance is this module's.
//!
//! Two rules from §6.2 shape the responses:
//!
//! * **Expiry is judged now, not by whatever a batch last wrote.** A coupon whose
//!   `expires_at` has passed reads as expired even if the sweep has not run (§18.1).
//! * **Spent and expired benefits are not hidden.** They are listed with the reason, so a
//!   customer can see what happened rather than watch a card disappear.

pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::pagination::{Cursor, Page, PageQuery};
use crate::loyalty::stamps::{LotBalance, build_board};

pub use routes::wallet_router;

/// `coupon.coupon_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CouponStatus {
    Pending,
    Available,
    Reserved,
    Used,
    Expired,
    Revoked,
    Voided,
    IssueFailed,
}

impl CouponStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            CouponStatus::Pending => "PENDING",
            CouponStatus::Available => "AVAILABLE",
            CouponStatus::Reserved => "RESERVED",
            CouponStatus::Used => "USED",
            CouponStatus::Expired => "EXPIRED",
            CouponStatus::Revoked => "REVOKED",
            CouponStatus::Voided => "VOIDED",
            CouponStatus::IssueFailed => "ISSUE_FAILED",
        }
    }

    /// Unknown values read as `ISSUE_FAILED`: a status this build does not understand
    /// must not be presented as spendable.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "PENDING" => CouponStatus::Pending,
            "AVAILABLE" => CouponStatus::Available,
            "RESERVED" => CouponStatus::Reserved,
            "USED" => CouponStatus::Used,
            "EXPIRED" => CouponStatus::Expired,
            "REVOKED" => CouponStatus::Revoked,
            "VOIDED" => CouponStatus::Voided,
            _ => CouponStatus::IssueFailed,
        }
    }

    /// Whether the coupon is still in play, ignoring the clock.
    pub fn is_open(self) -> bool {
        matches!(
            self,
            CouponStatus::Pending | CouponStatus::Available | CouponStatus::Reserved
        )
    }

    /// Whether this transition is one the system is allowed to make (§12.6-8).
    ///
    /// Written as a table rather than scattered `if`s so the forbidden combinations —
    /// resurrecting a used coupon, expiring a revoked one — are visible in one place.
    pub fn can_transition_to(self, next: CouponStatus) -> bool {
        use CouponStatus::*;
        match (self, next) {
            (from, to) if from == to => false,
            (Pending, Available | IssueFailed) => true,
            (Available, Reserved | Used | Expired | Revoked | Voided) => true,
            // A reservation either completes, lapses back, or is taken away.
            (Reserved, Available | Used | Expired | Revoked | Voided) => true,
            // REDEEM-004: a use the owner undoes inside their ten minutes either restores
            // the coupon — the campaign is still valid and it has not expired — or settles
            // it as `VOIDED`. This is the one way out of `USED`, and it exists because the
            // alternative is a customer losing a coupon to an owner's mis-tap.
            (Used, Available | Voided) => true,
            // Everything else is terminal. An expired or revoked coupon does not come back.
            _ => false,
        }
    }
}

/// One store's board in the wallet (§8.1: `N/10`, 가장 이른 도장 만료, 리워드 내용).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WalletStampBoard {
    pub store_id: Uuid,
    pub store_name: String,
    pub policy_id: Option<Uuid>,
    pub available: i64,
    pub target: i64,
    pub remaining_to_goal: i64,
    pub earliest_expiry: Option<DateTime<Utc>>,
    pub expiring_within_seven_days: i64,
    pub reward_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WalletStampsResponse {
    pub boards: Vec<WalletStampBoard>,
    pub total_available: i64,
    /// Server time the balances were judged against, so a client can show staleness.
    pub as_of: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WalletCoupon {
    pub id: Uuid,
    pub store_id: Uuid,
    pub store_name: String,
    pub source_type: String,
    /// What the row says.
    pub status: CouponStatus,
    /// What it means right now — `EXPIRED` once the clock has passed `expires_at`, even
    /// if the sweep has not caught up (§18.1).
    pub effective_status: CouponStatus,
    pub title: String,
    pub description: String,
    pub benefit_type: String,
    pub usable_from: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub issued_at: Option<DateTime<Utc>>,
    pub used_at: Option<DateTime<Utc>>,
    pub expired_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    /// Why it was taken away. Shown rather than hidden (§6.2).
    pub revocation_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub version: i64,
}

/// One state change, for the "what happened to my coupon" history.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CouponStatusEvent {
    pub from_status: Option<CouponStatus>,
    pub to_status: CouponStatus,
    pub reason_code: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WalletCouponDetail {
    #[serde(flatten)]
    pub coupon: WalletCoupon,
    /// The conditions frozen at issuance. A later policy edit does not change them
    /// (§8.5).
    pub condition_snapshot: serde_json::Value,
    pub history: Vec<CouponStatusEvent>,
}

/// `?status=` on the coupon list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WalletFilter {
    /// Spendable right now.
    Available,
    /// Used, expired or taken back — the `사용·만료 내역` tab (§6.2).
    History,
    /// The default: the wallet never hides a benefit, it only groups it (§6.2).
    #[default]
    All,
}

pub struct WalletService {
    _marker: Arc<()>,
}

impl Default for WalletService {
    fn default() -> Self {
        Self::new()
    }
}

impl WalletService {
    pub fn new() -> Self {
        Self {
            _marker: Arc::new(()),
        }
    }

    /// `GET /me/wallet/stamps` — one board per store the customer holds stamps in.
    pub async fn stamps(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<WalletStampsResponse> {
        let now = crate::qr::database_now(pool).await?;

        let rows = sqlx::query!(
            r#"
            SELECT
                b.store_id AS "store_id!",
                s.name AS store_name,
                b.lot_id AS "lot_id!",
                b.balance AS "balance!",
                b.expires_at AS "expires_at!"
            FROM coupon.stamp_lot_balances b
            JOIN coupon.stores s ON s.id = b.store_id
            WHERE b.user_id = $1 AND b.balance > 0 AND b.expires_at > $2
            ORDER BY b.store_id, b.expires_at
            "#,
            user_id,
            now,
        )
        .fetch_all(pool)
        .await?;

        if rows.is_empty() {
            return Ok(WalletStampsResponse {
                boards: Vec::new(),
                total_available: 0,
                as_of: now,
            });
        }

        let store_ids: Vec<Uuid> = {
            let mut ids: Vec<Uuid> = rows.iter().map(|row| row.store_id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };

        // The target a customer is working towards is the store's *current* policy. Where
        // a store has none any more (STAMP-008: 종료 후에도 기존 도장은 조회된다), fall
        // back to the policy the stamps were earned under so the board still reads `N/M`
        // rather than breaking.
        let active = sqlx::query!(
            r#"
            SELECT store_id, id, target_stamp_count, rule_snapshot
            FROM coupon.loyalty_policies
            WHERE store_id = ANY($1) AND status = 'ACTIVE'
            "#,
            &store_ids,
        )
        .fetch_all(pool)
        .await?;

        let fallback = sqlx::query!(
            r#"
            SELECT DISTINCT ON (l.store_id)
                l.store_id, p.id, p.target_stamp_count, p.rule_snapshot
            FROM coupon.stamp_lots l
            JOIN coupon.loyalty_policies p ON p.id = l.policy_id
            WHERE l.user_id = $1 AND l.store_id = ANY($2)
            ORDER BY l.store_id, l.earned_at DESC
            "#,
            user_id,
            &store_ids,
        )
        .fetch_all(pool)
        .await?;

        let mut boards = Vec::new();
        for store_id in store_ids {
            let store_rows: Vec<&_> = rows.iter().filter(|row| row.store_id == store_id).collect();
            let Some(first) = store_rows.first() else {
                continue;
            };

            let policy = active
                .iter()
                .find(|row| row.store_id == store_id)
                .map(|row| (row.id, row.target_stamp_count, &row.rule_snapshot))
                .or_else(|| {
                    fallback
                        .iter()
                        .find(|row| row.store_id == store_id)
                        .map(|row| (row.id, row.target_stamp_count, &row.rule_snapshot))
                });

            let lots: Vec<LotBalance> = store_rows
                .iter()
                .map(|row| LotBalance {
                    lot_id: row.lot_id,
                    balance: row.balance,
                    expires_at: row.expires_at,
                })
                .collect();

            let target = policy.map_or(10, |(_, target, _)| i64::from(target));
            let board = build_board(&lots, target, now);

            boards.push(WalletStampBoard {
                store_id,
                store_name: first.store_name.clone(),
                policy_id: policy.map(|(id, _, _)| id),
                available: board.available,
                target: board.target,
                remaining_to_goal: board.remaining_to_goal,
                earliest_expiry: board.earliest_expiry,
                expiring_within_seven_days: board.expiring_within_seven_days,
                reward_title: policy.and_then(|(_, _, snapshot)| {
                    snapshot
                        .get("reward")
                        .and_then(|reward| reward.get("title"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|title| !title.trim().is_empty())
                        .map(str::to_owned)
                }),
            });
        }

        // Soonest expiry first: that is the one the customer needs to act on (§6.2).
        boards.sort_by_key(|board| (board.earliest_expiry, board.store_id));

        Ok(WalletStampsResponse {
            total_available: boards.iter().map(|board| board.available).sum(),
            boards,
            as_of: now,
        })
    }

    /// `GET /me/wallet/coupons`.
    pub async fn coupons(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        store_id: Option<Uuid>,
        filter: WalletFilter,
        page: &PageQuery,
    ) -> ApiResult<Page<WalletCoupon>> {
        let now = crate::qr::database_now(pool).await?;
        let cursor = page.cursor()?;

        let rows = sqlx::query!(
            r#"
            SELECT
                c.id,
                c.store_id,
                s.name AS store_name,
                c.source_type::text AS "source_type!",
                c.status::text AS "status!",
                c.title,
                c.description,
                c.benefit_type::text AS "benefit_type!",
                c.usable_from,
                c.expires_at,
                c.issued_at,
                c.used_at,
                c.expired_at,
                c.revoked_at,
                c.revocation_reason,
                c.created_at,
                c.version
            FROM coupon.coupon_instances c
            JOIN coupon.stores s ON s.id = c.store_id
            WHERE c.user_id = $1
              AND ($2::uuid IS NULL OR c.store_id = $2)
              -- AVAILABLE is a live judgement: still marked available *and* not past its
              -- expiry. HISTORY is its complement, so the two tabs together are the whole
              -- wallet with nothing hidden (§6.2).
              AND (
                    $3 = 'ALL'
                 OR ($3 = 'AVAILABLE' AND c.status = 'AVAILABLE' AND c.expires_at > $4)
                 OR ($3 = 'HISTORY' AND (c.status <> 'AVAILABLE' OR c.expires_at <= $4))
              )
              AND ($5::timestamptz IS NULL OR (c.created_at, c.id) < ($5, $6))
            ORDER BY c.created_at DESC, c.id DESC
            LIMIT $7
            "#,
            user_id,
            store_id,
            match filter {
                WalletFilter::Available => "AVAILABLE",
                WalletFilter::History => "HISTORY",
                WalletFilter::All => "ALL",
            },
            now,
            cursor.as_ref().map(|cursor| cursor.created_at),
            cursor.as_ref().map(|cursor| cursor.id).unwrap_or_else(Uuid::nil),
            page.fetch_limit(),
        )
        .fetch_all(pool)
        .await?;

        let coupons: Vec<WalletCoupon> = rows
            .into_iter()
            .map(|row| {
                let status = CouponStatus::from_db(&row.status);
                WalletCoupon {
                    id: row.id,
                    store_id: row.store_id,
                    store_name: row.store_name,
                    source_type: row.source_type,
                    status,
                    effective_status: effective_status(status, row.expires_at, now),
                    title: row.title,
                    description: row.description,
                    benefit_type: row.benefit_type,
                    usable_from: row.usable_from,
                    expires_at: row.expires_at,
                    issued_at: row.issued_at,
                    used_at: row.used_at,
                    expired_at: row.expired_at,
                    revoked_at: row.revoked_at,
                    revocation_reason: row.revocation_reason,
                    created_at: row.created_at,
                    version: row.version,
                }
            })
            .collect();

        Ok(Page::from_rows(coupons, page.limit(), |coupon| {
            Cursor::new(coupon.created_at, coupon.id)
        }))
    }

    /// `GET /me/wallet/coupons/:id`, with the frozen conditions and the full history.
    pub async fn coupon_detail(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        coupon_id: Uuid,
    ) -> ApiResult<WalletCouponDetail> {
        let now = crate::qr::database_now(pool).await?;

        let row = sqlx::query!(
            r#"
            SELECT
                c.id,
                c.store_id,
                s.name AS store_name,
                c.source_type::text AS "source_type!",
                c.status::text AS "status!",
                c.title,
                c.description,
                c.benefit_type::text AS "benefit_type!",
                c.usable_from,
                c.expires_at,
                c.issued_at,
                c.used_at,
                c.expired_at,
                c.revoked_at,
                c.revocation_reason,
                c.condition_snapshot,
                c.created_at,
                c.version
            FROM coupon.coupon_instances c
            JOIN coupon.stores s ON s.id = c.store_id
            WHERE c.id = $1 AND c.user_id = $2
            "#,
            coupon_id,
            user_id,
        )
        .fetch_optional(pool)
        .await?
        // Not owned by the caller is indistinguishable from absent, on purpose (SEC-001).
        .ok_or_else(|| ApiError::new(ErrorCode::CouponNotFound))?;

        let history = sqlx::query!(
            r#"
            SELECT
                from_status::text AS "from_status?",
                to_status::text AS "to_status!",
                reason_code,
                occurred_at
            FROM coupon.coupon_status_events
            WHERE coupon_id = $1
            ORDER BY occurred_at, id
            "#,
            coupon_id,
        )
        .fetch_all(pool)
        .await?;

        let status = CouponStatus::from_db(&row.status);

        Ok(WalletCouponDetail {
            coupon: WalletCoupon {
                id: row.id,
                store_id: row.store_id,
                store_name: row.store_name,
                source_type: row.source_type,
                status,
                effective_status: effective_status(status, row.expires_at, now),
                title: row.title,
                description: row.description,
                benefit_type: row.benefit_type,
                usable_from: row.usable_from,
                expires_at: row.expires_at,
                issued_at: row.issued_at,
                used_at: row.used_at,
                expired_at: row.expired_at,
                revoked_at: row.revoked_at,
                revocation_reason: row.revocation_reason,
                created_at: row.created_at,
                version: row.version,
            },
            condition_snapshot: row.condition_snapshot,
            history: history
                .into_iter()
                .map(|event| CouponStatusEvent {
                    from_status: event.from_status.as_deref().map(CouponStatus::from_db),
                    to_status: CouponStatus::from_db(&event.to_status),
                    reason_code: event.reason_code,
                    occurred_at: event.occurred_at,
                })
                .collect(),
        })
    }

    /// Move coupons past their expiry into `EXPIRED` (§18.1 housekeeping).
    ///
    /// The reads above already treat them as expired, so this only tidies state — it can
    /// run late without anyone being able to spend something they should not.
    pub async fn expire_due_coupons(
        &self,
        pool: &PgPool,
        now: DateTime<Utc>,
        batch: i64,
    ) -> ApiResult<u64> {
        let due: Vec<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.coupon_instances
            WHERE status = 'AVAILABLE' AND expires_at <= $1
            ORDER BY expires_at
            LIMIT $2
            "#,
            now,
            batch,
        )
        .fetch_all(pool)
        .await?;

        let mut expired = 0u64;
        for coupon_id in due {
            let mut tx = pool.begin().await?;

            let changed = sqlx::query!(
                r#"
                UPDATE coupon.coupon_instances
                SET status = 'EXPIRED', expired_at = $2
                WHERE id = $1 AND status = 'AVAILABLE' AND expires_at <= $2
                "#,
                coupon_id,
                now,
            )
            .execute(&mut *tx)
            .await?;

            if changed.rows_affected() == 1 {
                sqlx::query!(
                    r#"
                    INSERT INTO coupon.coupon_status_events
                        (coupon_id, from_status, to_status, actor_type, reason_code, occurred_at)
                    VALUES ($1, 'AVAILABLE', 'EXPIRED', 'SYSTEM', 'COUPON_EXPIRED', $2)
                    "#,
                    coupon_id,
                    now,
                )
                .execute(&mut *tx)
                .await?;
                expired += 1;
            }

            tx.commit().await?;
        }

        Ok(expired)
    }
}

/// What a coupon means right now, as opposed to what its row last recorded.
pub fn effective_status(
    status: CouponStatus,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> CouponStatus {
    // §5.2: `[start, end)` — the coupon is unusable from `expires_at` onwards.
    if status.is_open() && now >= expires_at {
        CouponStatus::Expired
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn an_available_coupon_past_its_expiry_reads_as_expired() {
        // §18.1: 만료 상태 반영은 배치를 기다리지 않는다.
        assert_eq!(
            effective_status(CouponStatus::Available, at(1_000), at(1_001)),
            CouponStatus::Expired
        );
        assert_eq!(
            effective_status(CouponStatus::Available, at(1_000), at(999)),
            CouponStatus::Available
        );
    }

    #[test]
    fn the_expiry_boundary_excludes_the_instant_itself() {
        assert_eq!(
            effective_status(CouponStatus::Available, at(1_000), at(1_000)),
            CouponStatus::Expired,
            "사용 종료 시각은 미포함 (§8.5)"
        );
    }

    #[test]
    fn a_settled_coupon_is_not_relabelled_by_the_clock() {
        for status in [
            CouponStatus::Used,
            CouponStatus::Revoked,
            CouponStatus::Voided,
            CouponStatus::IssueFailed,
        ] {
            assert_eq!(
                effective_status(status, at(1_000), at(2_000)),
                status,
                "{status:?} must keep its own record"
            );
        }
    }

    #[test]
    fn statuses_round_trip_and_unknown_values_fail_closed() {
        for status in [
            CouponStatus::Pending,
            CouponStatus::Available,
            CouponStatus::Reserved,
            CouponStatus::Used,
            CouponStatus::Expired,
            CouponStatus::Revoked,
            CouponStatus::Voided,
            CouponStatus::IssueFailed,
        ] {
            assert_eq!(CouponStatus::from_db(status.as_db()), status);
        }
        assert_eq!(
            CouponStatus::from_db("SOMETHING_NEW"),
            CouponStatus::IssueFailed,
            "an unknown status must never read as spendable"
        );
    }

    #[test]
    fn issuance_reaches_availability_and_failure_only() {
        assert!(CouponStatus::Pending.can_transition_to(CouponStatus::Available));
        assert!(CouponStatus::Pending.can_transition_to(CouponStatus::IssueFailed));
        assert!(!CouponStatus::Pending.can_transition_to(CouponStatus::Used));
    }

    #[test]
    fn an_available_coupon_may_be_reserved_used_expired_revoked_or_voided() {
        for next in [
            CouponStatus::Reserved,
            CouponStatus::Used,
            CouponStatus::Expired,
            CouponStatus::Revoked,
            CouponStatus::Voided,
        ] {
            assert!(
                CouponStatus::Available.can_transition_to(next),
                "AVAILABLE → {next:?} must be allowed"
            );
        }
        assert!(!CouponStatus::Available.can_transition_to(CouponStatus::Pending));
    }

    #[test]
    fn a_reservation_can_lapse_back_to_available() {
        // §13.3: an expired reservation returns the coupon rather than consuming it.
        assert!(CouponStatus::Reserved.can_transition_to(CouponStatus::Available));
        assert!(CouponStatus::Reserved.can_transition_to(CouponStatus::Used));
    }

    #[test]
    fn terminal_statuses_are_terminal() {
        // The combinations that would let a spent benefit be spent twice.
        for from in [
            CouponStatus::Expired,
            CouponStatus::Revoked,
            CouponStatus::Voided,
            CouponStatus::IssueFailed,
        ] {
            for to in [
                CouponStatus::Available,
                CouponStatus::Reserved,
                CouponStatus::Used,
                CouponStatus::Pending,
            ] {
                assert!(
                    !from.can_transition_to(to),
                    "{from:?} → {to:?} must be forbidden"
                );
            }
        }
    }

    #[test]
    fn a_used_coupon_moves_only_where_redeem_004_allows() {
        // The owner's ten-minute undo either restores the coupon or settles it as voided.
        assert!(CouponStatus::Used.can_transition_to(CouponStatus::Available));
        assert!(CouponStatus::Used.can_transition_to(CouponStatus::Voided));

        // But it is never spent again, and never goes back to being held.
        assert!(!CouponStatus::Used.can_transition_to(CouponStatus::Used));
        assert!(!CouponStatus::Used.can_transition_to(CouponStatus::Reserved));
        assert!(!CouponStatus::Used.can_transition_to(CouponStatus::Pending));
        assert!(!CouponStatus::Used.can_transition_to(CouponStatus::Expired));
    }

    #[test]
    fn a_transition_to_the_same_status_is_not_a_transition() {
        // Mirrors `ck_coupon_status_changed` in the schema.
        for status in [CouponStatus::Available, CouponStatus::Used] {
            assert!(!status.can_transition_to(status));
        }
    }

    #[test]
    fn only_open_statuses_are_subject_to_expiry() {
        assert!(CouponStatus::Pending.is_open());
        assert!(CouponStatus::Available.is_open());
        assert!(CouponStatus::Reserved.is_open());
        assert!(!CouponStatus::Used.is_open());
        assert!(!CouponStatus::Expired.is_open());
    }

    #[test]
    fn the_default_filter_hides_nothing() {
        assert_eq!(WalletFilter::default(), WalletFilter::All);
    }
}
