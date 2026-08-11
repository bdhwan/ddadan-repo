//! 집계와 개인정보 보호 임계값 (§10.2 `analytics`, §6.3, §19 분석·통계).
//!
//! Owns `analytics_daily_store`.
//!
//! Two decisions shape everything here.
//!
//! **A missing row is not a zero.** §19 requires 실시간 수치와 확정 배치 수치를 구분한다, and
//! the failure mode that costs an owner money is a dashboard that shows `0` for a day the
//! batch has not reached yet. So a day the aggregation has never touched reports
//! [`AggregationState::Pending`] — 집계 중 — and a day it has touched reports whether the
//! business day was over when it did. The provisional figure is still shown, because a
//! shopkeeper wants to know how today is going; it is simply labelled.
//!
//! **Every figure is recomputed from the ledgers.** §19 says 지표는 원장을 기준으로 재산출
//! 가능해야 한다, so the aggregation is an idempotent upsert over the source tables rather
//! than an incrementing counter. Running it twice produces the same row; running it after a
//! correction produces the corrected row. That is also what makes 순수치와 총 발생치 both
//! available — the gross count and its reversal are stored separately and neither is a
//! subtraction anyone has to guess at.

pub mod routes;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::stores::business_day::BusinessCalendar;

pub use routes::owner_analytics_router;

/// How far back a single query may reach. A year of days is 365 rows, which is a chart;
/// five years is a data export, and §17.2 says a store is not an independent CRM.
pub const MAX_RANGE_DAYS: i64 = 366;

/// Whether a day's numbers are settled (§19: 실시간 수치와 확정 배치 수치를 구분한다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AggregationState {
    /// The batch has not reached this day. The counts are absent, not zero.
    Pending,
    /// Aggregated, but the business day was still open — 잠정치.
    Provisional,
    /// The business day had closed when this was computed — 확정치.
    Final,
}

/// One business day for one store.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DailyMetrics {
    pub business_day: NaiveDate,
    pub state: AggregationState,
    /// `null` while [`AggregationState::Pending`], so a client cannot render a zero.
    pub metrics: Option<DailyCounts>,
    /// When the figures were last recomputed.
    pub computed_through: Option<DateTime<Utc>>,
    /// True when the cohort behind this day is below the privacy floor and the breakdown
    /// has been withheld (§19 소규모 집단).
    pub suppressed: bool,
}

/// §6.3's dashboard row. Gross and reversal are separate columns on purpose.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct DailyCounts {
    /// 총 발생치: accruals recorded, before reversals.
    pub stamp_earned_count: i64,
    /// 취소·보정으로 되돌린 수.
    pub stamp_voided_count: i64,
    /// 순수치.
    pub stamp_net_count: i64,
    pub stamp_transaction_count: i64,
    /// Distinct customers seen. Withheld below the privacy floor.
    pub active_customer_count: Option<i64>,
    pub reward_issued_count: i64,
    pub reward_used_count: i64,
    pub campaign_coupon_issued_count: i64,
    pub campaign_coupon_used_count: i64,
    pub campaign_coupon_revoked_count: i64,
    pub coupon_expired_count: i64,
    pub redemption_voided_count: i64,
    pub discount_amount_total: i64,
}

/// The whole answer to `GET /owner/analytics`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AnalyticsResponse {
    pub store_id: Uuid,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Days whose numbers are settled.
    pub finalised_days: i64,
    /// Days the batch has not reached. Non-zero means the totals below are incomplete, and
    /// the client is expected to say so rather than present them as the period's result.
    pub pending_days: i64,
    /// The privacy floor in force (§19).
    pub minimum_cohort_size: i64,
    pub totals: DailyCounts,
    pub days: Vec<DailyMetrics>,
}

/// Reads and rebuilds `analytics_daily_store`.
pub struct AnalyticsService {
    minimum_cohort_size: i64,
}

impl AnalyticsService {
    pub fn new(minimum_cohort_size: i64) -> Self {
        Self {
            minimum_cohort_size,
        }
    }

    pub fn minimum_cohort_size(&self) -> i64 {
        self.minimum_cohort_size
    }

    /// `GET /owner/analytics` (§6.3).
    pub async fn range(
        &self,
        pool: &PgPool,
        store_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> ApiResult<AnalyticsResponse> {
        if to < from {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "조회 종료일이 시작일보다 빠릅니다.",
            ));
        }
        if (to - from).num_days() + 1 > MAX_RANGE_DAYS {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "한 번에 최대 366일까지 조회할 수 있습니다.",
            ));
        }

        let rows = sqlx::query!(
            r#"
            SELECT business_day, stamp_earned_count, stamp_voided_count,
                   stamp_transaction_count, active_customer_count, reward_issued_count,
                   reward_used_count, campaign_coupon_issued_count,
                   campaign_coupon_used_count, campaign_coupon_revoked_count,
                   coupon_expired_count, redemption_voided_count, discount_amount_total,
                   computed_through, is_final
            FROM coupon.analytics_daily_store
            WHERE store_id = $1 AND business_day BETWEEN $2 AND $3
            ORDER BY business_day
            "#,
            store_id,
            from,
            to,
        )
        .fetch_all(pool)
        .await?;

        let mut aggregated: std::collections::BTreeMap<NaiveDate, DailyMetrics> =
            std::collections::BTreeMap::new();

        for row in rows {
            let counts = DailyCounts {
                stamp_earned_count: row.stamp_earned_count,
                stamp_voided_count: row.stamp_voided_count,
                stamp_net_count: row.stamp_earned_count - row.stamp_voided_count,
                stamp_transaction_count: row.stamp_transaction_count,
                active_customer_count: Some(row.active_customer_count),
                reward_issued_count: row.reward_issued_count,
                reward_used_count: row.reward_used_count,
                campaign_coupon_issued_count: row.campaign_coupon_issued_count,
                campaign_coupon_used_count: row.campaign_coupon_used_count,
                campaign_coupon_revoked_count: row.campaign_coupon_revoked_count,
                coupon_expired_count: row.coupon_expired_count,
                redemption_voided_count: row.redemption_voided_count,
                discount_amount_total: row.discount_amount_total,
            };

            let (counts, suppressed) = self.apply_privacy_floor(counts);

            aggregated.insert(
                row.business_day,
                DailyMetrics {
                    business_day: row.business_day,
                    state: if row.is_final {
                        AggregationState::Final
                    } else {
                        AggregationState::Provisional
                    },
                    metrics: Some(counts),
                    computed_through: Some(row.computed_through),
                    suppressed,
                },
            );
        }

        // Fill the gaps explicitly. This is the whole point of the endpoint's shape: a day
        // with no row is 집계 중, and the client must be able to tell that from a quiet day.
        let mut days = Vec::new();
        let mut cursor = from;
        while cursor <= to {
            days.push(aggregated.remove(&cursor).unwrap_or(DailyMetrics {
                business_day: cursor,
                state: AggregationState::Pending,
                metrics: None,
                computed_through: None,
                suppressed: false,
            }));
            cursor = cursor.succ_opt().unwrap_or(cursor);
            if days.len() as i64 > MAX_RANGE_DAYS {
                break;
            }
        }

        let totals = sum(&days);
        let (totals, _) = self.apply_privacy_floor(totals);

        Ok(AnalyticsResponse {
            store_id,
            from,
            to,
            finalised_days: days
                .iter()
                .filter(|day| day.state == AggregationState::Final)
                .count() as i64,
            pending_days: days
                .iter()
                .filter(|day| day.state == AggregationState::Pending)
                .count() as i64,
            minimum_cohort_size: self.minimum_cohort_size,
            totals,
            days,
        })
    }

    /// §19: 소규모 집단의 개인 식별을 막기 위해 세그먼트가 기준 인원 미만이면 상세 분해를
    /// 숨긴다.
    ///
    /// Only the *breakdown by person* is withheld — the distinct-customer figure itself.
    /// The shop's own transaction totals are not personal data about a customer, and hiding
    /// them would make the dashboard useless on a quiet Tuesday without protecting anyone.
    fn apply_privacy_floor(&self, mut counts: DailyCounts) -> (DailyCounts, bool) {
        let cohort = counts.active_customer_count.unwrap_or(0);
        if cohort > 0 && cohort < self.minimum_cohort_size {
            counts.active_customer_count = None;
            return (counts, true);
        }
        (counts, false)
    }

    /// Rebuild one store-day from the ledgers (§14.6: store+business day).
    ///
    /// Idempotent by construction: every figure is a `COUNT` or `SUM` over source rows for
    /// that business day, so a second run writes the same numbers and a run after a
    /// correction writes the corrected ones. §19's 재산출 가능 is not a promise here, it is
    /// how the function is built.
    pub async fn aggregate_day(
        &self,
        pool: &PgPool,
        store_id: Uuid,
        business_day: NaiveDate,
        job_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> ApiResult<AggregationState> {
        let store = sqlx::query!(
            r#"
            SELECT timezone, business_day_cutoff::text AS "cutoff!"
            FROM coupon.stores WHERE id = $1
            "#,
            store_id,
        )
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::StoreNotFound))?;

        let calendar = BusinessCalendar::new(
            &store.timezone,
            crate::stores::business_day::parse_cutoff(&store.cutoff)?,
        );
        // §5.2's half-open `[start, end)`: the day ends where the next one begins.
        let day_start = calendar.business_day_start(business_day);
        let day_end = calendar.business_day_start(
            business_day
                .succ_opt()
                .ok_or_else(|| ApiError::new(ErrorCode::ValidationFailed))?,
        );

        // 확정 is decided by the clock, not by a flag someone sets: the day is final when
        // it has ended, and re-running before then simply refreshes a provisional row.
        let is_final = now >= day_end;

        sqlx::query!(
            r#"
            INSERT INTO coupon.analytics_daily_store AS a
                (store_id, business_day, stamp_earned_count, stamp_voided_count,
                 stamp_transaction_count, active_customer_count, reward_issued_count,
                 reward_used_count, campaign_coupon_issued_count,
                 campaign_coupon_used_count, campaign_coupon_revoked_count,
                 coupon_expired_count, redemption_voided_count, discount_amount_total,
                 computed_through, is_final, aggregated_job_id)
            SELECT
                $1, $2,
                -- 총 발생치: what the ledger recorded, whatever happened to it later.
                COALESCE((
                    SELECT SUM(t.quantity) FROM coupon.stamp_transactions t
                    WHERE t.store_id = $1 AND t.business_day = $2
                ), 0),
                COALESCE((
                    SELECT SUM(t.quantity) FROM coupon.stamp_transactions t
                    WHERE t.store_id = $1 AND t.business_day = $2 AND t.status = 'VOIDED'
                ), 0),
                (SELECT COUNT(*) FROM coupon.stamp_transactions t
                 WHERE t.store_id = $1 AND t.business_day = $2),
                (SELECT COUNT(DISTINCT t.user_id) FROM coupon.stamp_transactions t
                 WHERE t.store_id = $1 AND t.business_day = $2 AND t.status = 'CONFIRMED'),
                (SELECT COUNT(*) FROM coupon.coupon_instances c
                 WHERE c.store_id = $1 AND c.source_type = 'LOYALTY_REWARD'
                   AND c.issued_at >= $3 AND c.issued_at < $4),
                (SELECT COUNT(*) FROM coupon.coupon_instances c
                 WHERE c.store_id = $1 AND c.source_type = 'LOYALTY_REWARD'
                   AND c.used_at >= $3 AND c.used_at < $4),
                (SELECT COUNT(*) FROM coupon.coupon_instances c
                 WHERE c.store_id = $1 AND c.source_type = 'CAMPAIGN'
                   AND c.issued_at >= $3 AND c.issued_at < $4),
                (SELECT COUNT(*) FROM coupon.coupon_instances c
                 WHERE c.store_id = $1 AND c.source_type = 'CAMPAIGN'
                   AND c.used_at >= $3 AND c.used_at < $4),
                (SELECT COUNT(*) FROM coupon.coupon_instances c
                 WHERE c.store_id = $1 AND c.source_type = 'CAMPAIGN'
                   AND c.revoked_at >= $3 AND c.revoked_at < $4),
                (SELECT COUNT(*) FROM coupon.coupon_instances c
                 WHERE c.store_id = $1 AND c.expired_at >= $3 AND c.expired_at < $4),
                (SELECT COUNT(*) FROM coupon.redemption_transactions r
                 WHERE r.store_id = $1 AND r.status = 'VOIDED'
                   AND r.confirmed_at >= $3 AND r.confirmed_at < $4),
                COALESCE((
                    SELECT SUM(r.discount_amount) FROM coupon.redemption_transactions r
                    WHERE r.store_id = $1 AND r.status = 'CONFIRMED'
                      AND r.confirmed_at >= $3 AND r.confirmed_at < $4
                ), 0),
                $5, $6, $7
            ON CONFLICT (store_id, business_day) DO UPDATE
            SET stamp_earned_count = EXCLUDED.stamp_earned_count,
                stamp_voided_count = EXCLUDED.stamp_voided_count,
                stamp_transaction_count = EXCLUDED.stamp_transaction_count,
                active_customer_count = EXCLUDED.active_customer_count,
                reward_issued_count = EXCLUDED.reward_issued_count,
                reward_used_count = EXCLUDED.reward_used_count,
                campaign_coupon_issued_count = EXCLUDED.campaign_coupon_issued_count,
                campaign_coupon_used_count = EXCLUDED.campaign_coupon_used_count,
                campaign_coupon_revoked_count = EXCLUDED.campaign_coupon_revoked_count,
                coupon_expired_count = EXCLUDED.coupon_expired_count,
                redemption_voided_count = EXCLUDED.redemption_voided_count,
                discount_amount_total = EXCLUDED.discount_amount_total,
                computed_through = EXCLUDED.computed_through,
                -- A finalised day never goes back to provisional: a late re-run must not
                -- relabel settled numbers as still moving.
                is_final = a.is_final OR EXCLUDED.is_final,
                aggregated_job_id = EXCLUDED.aggregated_job_id,
                version = a.version + 1
            "#,
            store_id,
            business_day,
            day_start,
            day_end,
            now,
            is_final,
            job_id,
        )
        .execute(pool)
        .await?;

        Ok(if is_final {
            AggregationState::Final
        } else {
            AggregationState::Provisional
        })
    }

    /// Stores with activity that has not been aggregated yet, so the scheduler knows what
    /// to enqueue rather than fanning out over every store every night.
    pub async fn stores_needing_aggregation(
        &self,
        pool: &PgPool,
        business_day: NaiveDate,
        limit: i64,
    ) -> ApiResult<Vec<Uuid>> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT s.id
            FROM coupon.stores s
            WHERE s.status IN ('ACTIVE', 'SUSPENDED')
              AND NOT EXISTS (
                  SELECT 1 FROM coupon.analytics_daily_store a
                  WHERE a.store_id = s.id AND a.business_day = $1 AND a.is_final
              )
            ORDER BY s.id
            LIMIT $2
            "#,
            business_day,
            limit,
        )
        .fetch_all(pool)
        .await?)
    }
}

fn sum(days: &[DailyMetrics]) -> DailyCounts {
    let mut totals = DailyCounts {
        stamp_earned_count: 0,
        stamp_voided_count: 0,
        stamp_net_count: 0,
        stamp_transaction_count: 0,
        active_customer_count: Some(0),
        reward_issued_count: 0,
        reward_used_count: 0,
        campaign_coupon_issued_count: 0,
        campaign_coupon_used_count: 0,
        campaign_coupon_revoked_count: 0,
        coupon_expired_count: 0,
        redemption_voided_count: 0,
        discount_amount_total: 0,
    };

    // The customer figure is summed over days that reported one. Days withheld by the
    // privacy floor are simply not added: a total that quietly re-included them would give
    // back by subtraction exactly what the floor withheld.
    let mut customers = 0i64;
    for day in days {
        let Some(counts) = day.metrics else { continue };
        totals.stamp_earned_count += counts.stamp_earned_count;
        totals.stamp_voided_count += counts.stamp_voided_count;
        totals.stamp_net_count += counts.stamp_net_count;
        totals.stamp_transaction_count += counts.stamp_transaction_count;
        customers += counts.active_customer_count.unwrap_or(0);
        totals.reward_issued_count += counts.reward_issued_count;
        totals.reward_used_count += counts.reward_used_count;
        totals.campaign_coupon_issued_count += counts.campaign_coupon_issued_count;
        totals.campaign_coupon_used_count += counts.campaign_coupon_used_count;
        totals.campaign_coupon_revoked_count += counts.campaign_coupon_revoked_count;
        totals.coupon_expired_count += counts.coupon_expired_count;
        totals.redemption_voided_count += counts.redemption_voided_count;
        totals.discount_amount_total += counts.discount_amount_total;
    }
    totals.active_customer_count = Some(customers);
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> AnalyticsService {
        AnalyticsService::new(5)
    }

    fn counts(customers: i64) -> DailyCounts {
        DailyCounts {
            stamp_earned_count: 12,
            stamp_voided_count: 2,
            stamp_net_count: 10,
            stamp_transaction_count: 9,
            active_customer_count: Some(customers),
            reward_issued_count: 1,
            reward_used_count: 0,
            campaign_coupon_issued_count: 3,
            campaign_coupon_used_count: 1,
            campaign_coupon_revoked_count: 0,
            coupon_expired_count: 0,
            redemption_voided_count: 0,
            discount_amount_total: 4_500,
        }
    }

    fn day(date: &str, state: AggregationState, metrics: Option<DailyCounts>) -> DailyMetrics {
        DailyMetrics {
            business_day: date.parse().expect("valid date"),
            state,
            metrics,
            computed_through: None,
            suppressed: false,
        }
    }

    #[test]
    fn a_cohort_below_the_floor_loses_its_breakdown_but_keeps_the_shop_totals() {
        // §19: 세그먼트가 기준 인원 미만이면 상세 분해를 숨긴다.
        let (withheld, suppressed) = service().apply_privacy_floor(counts(4));

        assert!(suppressed);
        assert_eq!(withheld.active_customer_count, None);
        assert_eq!(
            withheld.stamp_earned_count, 12,
            "the shop's own transaction count is not personal data about a customer"
        );
    }

    #[test]
    fn a_cohort_at_the_floor_is_reported() {
        let (reported, suppressed) = service().apply_privacy_floor(counts(5));
        assert!(!suppressed);
        assert_eq!(reported.active_customer_count, Some(5));
    }

    #[test]
    fn a_day_with_no_customers_at_all_is_not_a_suppression() {
        // Zero is a fact about the shop, not a small cohort to protect.
        let (reported, suppressed) = service().apply_privacy_floor(counts(0));
        assert!(!suppressed);
        assert_eq!(reported.active_customer_count, Some(0));
    }

    #[test]
    fn totals_exclude_the_days_the_floor_withheld() {
        // Otherwise the withheld figure would be recoverable by subtracting the reported
        // days from the total, which is the whole attack the floor exists to prevent.
        let mut withheld = counts(3);
        withheld.active_customer_count = None;

        let totals = sum(&[
            day("2026-08-10", AggregationState::Final, Some(counts(7))),
            day("2026-08-11", AggregationState::Final, Some(withheld)),
        ]);

        assert_eq!(totals.active_customer_count, Some(7));
        assert_eq!(totals.stamp_earned_count, 24, "shop totals still add up");
    }

    #[test]
    fn a_day_the_batch_has_not_reached_carries_no_numbers() {
        // §19: 집계 전이면 0 이 아니라 집계 중이다.
        let pending = day("2026-08-12", AggregationState::Pending, None);

        assert_eq!(pending.state, AggregationState::Pending);
        assert!(
            pending.metrics.is_none(),
            "a pending day must not be renderable as a zero"
        );
    }

    #[test]
    fn a_pending_day_contributes_nothing_to_the_totals() {
        let totals = sum(&[
            day("2026-08-10", AggregationState::Final, Some(counts(7))),
            day("2026-08-11", AggregationState::Pending, None),
        ]);

        assert_eq!(totals.stamp_earned_count, 12);
    }
}
