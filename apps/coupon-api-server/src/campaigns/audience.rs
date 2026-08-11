//! 캠페인 대상 (CAMPAIGN-001, CAMPAIGN-003, CAMPAIGN-004).
//!
//! A campaign's audience is answered twice, and the two answers are deliberately
//! different:
//!
//! * **Direct issuance** freezes the audience into `campaign_audience_members` at publish
//!   time. CAMPAIGN-003 step 2 wants "게시 시점의 대상 조건"; if the worker re-evaluated
//!   per batch, a customer who visited half-way through a long run would appear in one
//!   batch and not another, and a resumed job would issue to a different set than the one
//!   the owner approved.
//! * **First-come** evaluates live, at claim time. There is no snapshot to be in:
//!   CAMPAIGN-004 says "대상 조건을 충족한 소비자" — the condition is checked when the
//!   customer presses the button.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::campaigns::Campaign;
use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};

/// Who a campaign is for (CAMPAIGN-001 대상).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AudienceType {
    /// 전체 상점 고객 — anyone who has ever transacted here.
    #[default]
    AllCustomers,
    /// 관심 고객.
    FavoriteCustomers,
    /// 최근 방문 고객.
    RecentVisitors,
    /// 도장 수 조건.
    StampThreshold,
    /// 특정 고객.
    SpecificUsers,
}

impl AudienceType {
    pub fn as_db(self) -> &'static str {
        match self {
            AudienceType::AllCustomers => "ALL_CUSTOMERS",
            AudienceType::FavoriteCustomers => "FAVORITE_CUSTOMERS",
            AudienceType::RecentVisitors => "RECENT_VISITORS",
            AudienceType::StampThreshold => "STAMP_THRESHOLD",
            AudienceType::SpecificUsers => "SPECIFIC_USERS",
        }
    }

    /// Unknown reads as `SPECIFIC_USERS` with, in practice, an empty list — the narrowest
    /// possible audience. An audience this build cannot evaluate must not default to
    /// "everybody".
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "ALL_CUSTOMERS" => AudienceType::AllCustomers,
            "FAVORITE_CUSTOMERS" => AudienceType::FavoriteCustomers,
            "RECENT_VISITORS" => AudienceType::RecentVisitors,
            "STAMP_THRESHOLD" => AudienceType::StampThreshold,
            _ => AudienceType::SpecificUsers,
        }
    }
}

/// The parameters an audience type needs. Unused fields for a given type are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct AudienceCriteria {
    /// `RECENT_VISITORS`: how far back "recent" reaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_visit_days: Option<i32>,
    /// `STAMP_THRESHOLD`: the minimum currently-held stamp count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_stamps: Option<i32>,
    /// `SPECIFIC_USERS`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_ids: Vec<Uuid>,
}

/// Refuse an audience that cannot be evaluated, before publishing rather than inside a
/// worker where the only outcome is a dead letter (§14.6 잘못된 대상 schema).
pub fn validate(campaign: &Campaign) -> ApiResult<()> {
    match campaign.audience_type {
        AudienceType::RecentVisitors => {
            let days = campaign.audience_criteria.recent_visit_days.unwrap_or(0);
            if !(1..=365).contains(&days) {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    "최근 방문 고객 대상은 1~365일 사이의 기간이 필요합니다.",
                ));
            }
        }
        AudienceType::StampThreshold => {
            let stamps = campaign.audience_criteria.minimum_stamps.unwrap_or(0);
            if !(1..=100).contains(&stamps) {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    "도장 수 조건은 1~100개 사이여야 합니다.",
                ));
            }
        }
        AudienceType::SpecificUsers => {
            if campaign.audience_criteria.user_ids.is_empty() {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    "특정 고객 대상은 한 명 이상을 지정해야 합니다.",
                ));
            }
            if campaign.audience_criteria.user_ids.len() > 10_000 {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    "특정 고객 대상은 한 번에 10,000명까지 지정할 수 있습니다.",
                ));
            }
        }
        AudienceType::AllCustomers | AudienceType::FavoriteCustomers => {}
    }

    Ok(())
}

/// 대상 예상 인원 (CAMPAIGN-002).
pub async fn size<'e, E>(executor: E, store_id: Uuid, campaign: &Campaign) -> ApiResult<i64>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let now = Utc::now();

    Ok(sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM coupon.users u
        WHERE u.status = 'ACTIVE'
          AND CASE $2::text
              WHEN 'ALL_CUSTOMERS' THEN EXISTS (
                  SELECT 1 FROM coupon.store_customers c
                  WHERE c.store_id = $1 AND c.user_id = u.id
              )
              WHEN 'FAVORITE_CUSTOMERS' THEN EXISTS (
                  SELECT 1 FROM coupon.favorite_stores f
                  WHERE f.store_id = $1 AND f.user_id = u.id AND f.removed_at IS NULL
              )
              WHEN 'RECENT_VISITORS' THEN EXISTS (
                  SELECT 1 FROM coupon.store_customers c
                  WHERE c.store_id = $1 AND c.user_id = u.id AND c.last_seen_at >= $3
              )
              WHEN 'STAMP_THRESHOLD' THEN (
                  SELECT COALESCE(SUM(b.balance), 0)::bigint
                  FROM coupon.stamp_lot_balances b
                  WHERE b.store_id = $1 AND b.user_id = u.id AND b.expires_at > $5
              ) >= $4
              WHEN 'SPECIFIC_USERS' THEN u.id = ANY($6)
              ELSE false
          END
        "#,
        store_id,
        campaign.audience_type.as_db(),
        recent_cutoff(campaign, now),
        i64::from(campaign.audience_criteria.minimum_stamps.unwrap_or(0)),
        now,
        &campaign.audience_criteria.user_ids,
    )
    .fetch_one(executor)
    .await?)
}

/// One page of the audience, in a stable id order (CAMPAIGN-003 step 3).
///
/// The order is what makes the checkpoint meaningful: "resume after this id" is only a
/// resumption if the sequence cannot reorder underneath it.
pub async fn page(
    tx: &mut Tx<'_>,
    store_id: Uuid,
    campaign: &Campaign,
    after_user_id: Option<Uuid>,
    limit: i64,
    now: DateTime<Utc>,
) -> ApiResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT u.id
        FROM coupon.users u
        WHERE u.status = 'ACTIVE'
          AND ($7::uuid IS NULL OR u.id > $7)
          AND CASE $2::text
              WHEN 'ALL_CUSTOMERS' THEN EXISTS (
                  SELECT 1 FROM coupon.store_customers c
                  WHERE c.store_id = $1 AND c.user_id = u.id
              )
              WHEN 'FAVORITE_CUSTOMERS' THEN EXISTS (
                  SELECT 1 FROM coupon.favorite_stores f
                  WHERE f.store_id = $1 AND f.user_id = u.id AND f.removed_at IS NULL
              )
              WHEN 'RECENT_VISITORS' THEN EXISTS (
                  SELECT 1 FROM coupon.store_customers c
                  WHERE c.store_id = $1 AND c.user_id = u.id AND c.last_seen_at >= $3
              )
              WHEN 'STAMP_THRESHOLD' THEN (
                  SELECT COALESCE(SUM(b.balance), 0)::bigint
                  FROM coupon.stamp_lot_balances b
                  WHERE b.store_id = $1 AND b.user_id = u.id AND b.expires_at > $5
              ) >= $4
              WHEN 'SPECIFIC_USERS' THEN u.id = ANY($6)
              ELSE false
          END
        ORDER BY u.id
        LIMIT $8
        "#,
        store_id,
        campaign.audience_type.as_db(),
        recent_cutoff(campaign, now),
        i64::from(campaign.audience_criteria.minimum_stamps.unwrap_or(0)),
        now,
        &campaign.audience_criteria.user_ids,
        after_user_id,
        limit,
    )
    .fetch_all(&mut **tx)
    .await?)
}

/// Whether one consumer qualifies right now (CAMPAIGN-004).
///
/// A `DIRECT` campaign answers from its frozen snapshot instead: once the audience has
/// been decided, membership is a fact about that decision, not about the customer today.
pub async fn is_eligible(
    tx: &mut Tx<'_>,
    campaign: &Campaign,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> ApiResult<bool> {
    if campaign.audience_snapshot_at.is_some() {
        return Ok(sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM coupon.campaign_audience_members
                WHERE campaign_id = $1 AND user_id = $2
            ) AS "member!"
            "#,
            campaign.id,
            user_id,
        )
        .fetch_one(&mut **tx)
        .await?);
    }

    Ok(sqlx::query_scalar!(
        r#"
        SELECT CASE $3::text
            WHEN 'ALL_CUSTOMERS' THEN true
            WHEN 'FAVORITE_CUSTOMERS' THEN EXISTS (
                SELECT 1 FROM coupon.favorite_stores f
                WHERE f.store_id = $1 AND f.user_id = $2 AND f.removed_at IS NULL
            )
            WHEN 'RECENT_VISITORS' THEN EXISTS (
                SELECT 1 FROM coupon.store_customers c
                WHERE c.store_id = $1 AND c.user_id = $2 AND c.last_seen_at >= $4
            )
            WHEN 'STAMP_THRESHOLD' THEN (
                SELECT COALESCE(SUM(b.balance), 0)::bigint
                FROM coupon.stamp_lot_balances b
                WHERE b.store_id = $1 AND b.user_id = $2 AND b.expires_at > $6
            ) >= $5
            WHEN 'SPECIFIC_USERS' THEN $2 = ANY($7)
            ELSE false
        END AS "eligible!"
        "#,
        campaign.store_id,
        user_id,
        campaign.audience_type.as_db(),
        recent_cutoff(campaign, now),
        i64::from(campaign.audience_criteria.minimum_stamps.unwrap_or(0)),
        now,
        &campaign.audience_criteria.user_ids,
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// §11.3's public campaign list treats 전체 상점 고객 as open to anyone who finds the
/// store — a first-come campaign that only existing customers could claim would never
/// bring anyone new in, which is the point of running one.
fn recent_cutoff(campaign: &Campaign, now: DateTime<Utc>) -> DateTime<Utc> {
    let days = campaign
        .audience_criteria
        .recent_visit_days
        .unwrap_or(30)
        .clamp(1, 365);
    now - chrono::Duration::days(i64::from(days))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn campaign_with(audience_type: AudienceType, criteria: AudienceCriteria) -> Campaign {
        let mut campaign = crate::campaigns::tests_support::campaign();
        campaign.audience_type = audience_type;
        campaign.audience_criteria = criteria;
        campaign
    }

    #[test]
    fn audience_types_round_trip_and_unknown_values_narrow_rather_than_widen() {
        for audience_type in [
            AudienceType::AllCustomers,
            AudienceType::FavoriteCustomers,
            AudienceType::RecentVisitors,
            AudienceType::StampThreshold,
            AudienceType::SpecificUsers,
        ] {
            assert_eq!(AudienceType::from_db(audience_type.as_db()), audience_type);
        }

        assert_eq!(
            AudienceType::from_db("EVERYONE_EVERYWHERE"),
            AudienceType::SpecificUsers,
            "an audience this build cannot evaluate must not default to everybody"
        );
    }

    #[test]
    fn a_recent_visitor_audience_needs_a_usable_window() {
        assert!(
            validate(&campaign_with(
                AudienceType::RecentVisitors,
                AudienceCriteria::default()
            ))
            .is_err()
        );
        assert!(
            validate(&campaign_with(
                AudienceType::RecentVisitors,
                AudienceCriteria {
                    recent_visit_days: Some(0),
                    ..Default::default()
                }
            ))
            .is_err()
        );
        assert!(
            validate(&campaign_with(
                AudienceType::RecentVisitors,
                AudienceCriteria {
                    recent_visit_days: Some(30),
                    ..Default::default()
                }
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_stamp_threshold_audience_needs_a_threshold() {
        assert!(
            validate(&campaign_with(
                AudienceType::StampThreshold,
                AudienceCriteria::default()
            ))
            .is_err()
        );
        assert!(
            validate(&campaign_with(
                AudienceType::StampThreshold,
                AudienceCriteria {
                    minimum_stamps: Some(5),
                    ..Default::default()
                }
            ))
            .is_ok()
        );
    }

    #[test]
    fn a_specific_user_audience_needs_at_least_one_and_not_too_many() {
        assert!(
            validate(&campaign_with(
                AudienceType::SpecificUsers,
                AudienceCriteria::default()
            ))
            .is_err()
        );
        assert!(
            validate(&campaign_with(
                AudienceType::SpecificUsers,
                AudienceCriteria {
                    user_ids: vec![Uuid::nil()],
                    ..Default::default()
                }
            ))
            .is_ok()
        );

        let too_many = AudienceCriteria {
            user_ids: (0..10_001).map(Uuid::from_u128).collect(),
            ..Default::default()
        };
        assert!(validate(&campaign_with(AudienceType::SpecificUsers, too_many)).is_err());
    }

    #[test]
    fn the_open_audiences_need_no_parameters() {
        for audience_type in [AudienceType::AllCustomers, AudienceType::FavoriteCustomers] {
            assert!(validate(&campaign_with(audience_type, AudienceCriteria::default())).is_ok());
        }
    }

    #[test]
    fn a_missing_recency_window_falls_back_to_a_bounded_default() {
        let now = "2026-08-10T00:00:00Z".parse::<DateTime<Utc>>().expect("time");
        let campaign = campaign_with(AudienceType::RecentVisitors, AudienceCriteria::default());

        assert_eq!(
            recent_cutoff(&campaign, now),
            now - chrono::Duration::days(30),
            "an unset window must not reach back to the beginning of time"
        );
    }

    #[test]
    fn criteria_only_serialise_what_was_set() {
        let json = serde_json::to_value(AudienceCriteria {
            minimum_stamps: Some(5),
            ..Default::default()
        })
        .expect("serialises");

        assert_eq!(json["minimum_stamps"], 5);
        assert!(json.get("recent_visit_days").is_none());
        assert!(json.get("user_ids").is_none());
    }
}
