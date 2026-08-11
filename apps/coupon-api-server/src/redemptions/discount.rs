//! 할인 계산과 발급 시점 조건 스냅샷 (§8.2, §8.3, §5.3).
//!
//! Everything here is a pure function over a [`CouponConditions`] value. That matters
//! more than it looks: §8.5 says a campaign edit must not reach back into a coupon that
//! is already in someone's wallet, and the only way to guarantee that is for the code
//! that decides a discount to have no way of reading the campaign at all. It reads the
//! snapshot that was frozen into `coupon_instances.condition_snapshot` at issuance, and
//! nothing else.
//!
//! The three §8.2 benefit types and their rounding come straight from §5.3:
//!
//! * **정액** — `min(대상 금액, 할인액)`. Never more than the order it applies to.
//! * **정률** — `대상 금액 × 율`, **1원 미만 버림**, then the maximum discount.
//! * **무료 품목** — the *lowest* unit price among the named items actually ordered.

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::catalog::{ItemRestriction, OrderLine};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::loyalty::BenefitType;
use crate::stores::business_day::resolve_timezone;

/// The `condition_snapshot` schema this build writes and understands.
pub const CONDITION_SCHEMA_VERSION: i32 = 1;

/// The discount a coupon grants, in the form the calculator needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Benefit {
    pub benefit_type: BenefitType,
    /// 정액 할인액.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed_amount: Option<i64>,
    /// 1–100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<i16>,
    /// Required by 정률 (§8.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum_discount_amount: Option<i64>,
    /// 무료 품목 대상. One or more catalogue ids.
    #[serde(default)]
    pub free_item_ids: Vec<Uuid>,
}

impl Benefit {
    /// Reject a benefit whose fields do not match its type, before it can be published
    /// (CAMPAIGN-002). The database carries the same rule as `ck_campaign_benefit_fields`;
    /// this is the copy that can say *why* in Korean.
    pub fn validate(&self) -> ApiResult<()> {
        match self.benefit_type {
            BenefitType::FixedAmount => {
                let amount = self.fixed_amount.unwrap_or(0);
                if amount <= 0 {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "정액 할인은 1원 이상의 할인액이 필요합니다.",
                    ));
                }
                if !self.free_item_ids.is_empty() || self.percentage.is_some() {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "정액 할인에는 할인율이나 무료 품목을 함께 설정할 수 없습니다.",
                    ));
                }
            }
            BenefitType::Percentage => {
                let percentage = self.percentage.unwrap_or(0);
                if !(1..=100).contains(&percentage) {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "할인율은 1~100% 여야 합니다.",
                    ));
                }
                // §8.2 makes the ceiling mandatory, not optional: a percentage without one
                // is an open-ended liability on a large order.
                if self.maximum_discount_amount.unwrap_or(0) <= 0 {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "정률 할인은 최대 할인액을 함께 설정해야 합니다.",
                    ));
                }
                if self.fixed_amount.is_some() || !self.free_item_ids.is_empty() {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "정률 할인에는 정액 할인액이나 무료 품목을 함께 설정할 수 없습니다.",
                    ));
                }
            }
            BenefitType::FreeItem => {
                if self.free_item_ids.is_empty() {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "무료 품목 쿠폰은 대상 품목을 하나 이상 지정해야 합니다.",
                    ));
                }
                if self.fixed_amount.is_some() || self.percentage.is_some() {
                    return Err(ApiError::with_message(
                        ErrorCode::ValidationFailed,
                        "무료 품목 쿠폰에는 할인액이나 할인율을 함께 설정할 수 없습니다.",
                    ));
                }
            }
        }

        Ok(())
    }
}

/// One `HH:MM`–`HH:MM` window a coupon may be used in, in the store's local time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct LocalTimeRange {
    /// `HH:MM`, inclusive.
    pub start: String,
    /// `HH:MM`, exclusive — the same `[start, end)` rule as every other period (§5.2).
    pub end: String,
}

impl LocalTimeRange {
    fn parse(&self) -> Option<(NaiveTime, NaiveTime)> {
        let start = NaiveTime::parse_from_str(self.start.trim(), "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(self.start.trim(), "%H:%M:%S"))
            .ok()?;
        let end = NaiveTime::parse_from_str(self.end.trim(), "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(self.end.trim(), "%H:%M:%S"))
            .ok()?;
        Some((start, end))
    }

    fn contains(&self, at: NaiveTime) -> bool {
        let Some((start, end)) = self.parse() else {
            // An unparseable window would otherwise silently forbid every hour. Treat it
            // as "no restriction from this entry" and let validation catch it at publish.
            return true;
        };

        if start <= end {
            at >= start && at < end
        } else {
            // 22:00–02:00 and similar late-night windows wrap past midnight.
            at >= start || at < end
        }
    }
}

/// Everything a coupon carries about *how* it may be spent, frozen at issuance (§8.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CouponConditions {
    #[serde(default = "default_schema_version")]
    pub schema_version: i32,
    pub benefit: Benefit,
    #[serde(default)]
    pub minimum_order_amount: i64,
    #[serde(default)]
    pub eligible_item_ids: Vec<Uuid>,
    #[serde(default)]
    pub eligible_category_ids: Vec<Uuid>,
    #[serde(default)]
    pub excluded_item_ids: Vec<Uuid>,
    /// `0` is Sunday. Empty means every day (§8.5, CAMPAIGN-001).
    #[serde(default)]
    pub allowed_weekdays: Vec<i16>,
    #[serde(default)]
    pub allowed_local_time_ranges: Vec<LocalTimeRange>,
    /// The store's zone at issuance. A later zone change does not move an absolute
    /// expiry (§5.2), but the weekday/hour window is a local-time concept and needs one.
    #[serde(default = "default_timezone")]
    pub timezone: String,
    /// Whether the store said its own in-house discounts may be combined (§5.4). The
    /// system cannot verify an external discount, so this is shown, not enforced.
    #[serde(default)]
    pub external_discount_stackable: bool,
}

fn default_schema_version() -> i32 {
    CONDITION_SCHEMA_VERSION
}

fn default_timezone() -> String {
    crate::stores::business_day::DEFAULT_TIMEZONE.to_owned()
}

impl CouponConditions {
    pub fn restriction(&self) -> ItemRestriction {
        ItemRestriction {
            eligible_item_ids: self.eligible_item_ids.clone(),
            eligible_category_ids: self.eligible_category_ids.clone(),
            excluded_item_ids: self.excluded_item_ids.clone(),
        }
    }

    /// Read the conditions back out of a stored `condition_snapshot`.
    ///
    /// Two shapes exist in the wild: the campaign shape this module writes, and the
    /// loyalty-reward shape Phase 2 writes under a `reward` key. Both describe the same
    /// thing, and a reward coupon is spent through exactly the same path as a campaign
    /// coupon, so both are accepted here rather than forcing every caller to branch.
    pub fn from_snapshot(snapshot: &serde_json::Value) -> ApiResult<Self> {
        if let Some(conditions) = snapshot.get("conditions") {
            return serde_json::from_value(conditions.clone()).map_err(malformed_snapshot);
        }

        if let Some(reward) = snapshot.get("reward") {
            let reward: LoyaltyRewardShape =
                serde_json::from_value(reward.clone()).map_err(malformed_snapshot)?;
            return Ok(Self {
                schema_version: CONDITION_SCHEMA_VERSION,
                benefit: Benefit {
                    benefit_type: reward.benefit_type,
                    fixed_amount: reward.fixed_amount,
                    percentage: reward.percentage,
                    maximum_discount_amount: reward.maximum_discount_amount,
                    free_item_ids: reward.free_item_ids,
                },
                minimum_order_amount: reward.minimum_order_amount,
                eligible_item_ids: Vec::new(),
                eligible_category_ids: Vec::new(),
                excluded_item_ids: Vec::new(),
                allowed_weekdays: Vec::new(),
                allowed_local_time_ranges: Vec::new(),
                timezone: snapshot
                    .get("store")
                    .and_then(|store| store.get("timezone"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(crate::stores::business_day::DEFAULT_TIMEZONE)
                    .to_owned(),
                external_discount_stackable: false,
            });
        }

        Err(ApiError::with_message(
            ErrorCode::RequiresAdminReview,
            "쿠폰 조건을 해석할 수 없습니다. 고객센터에 문의해 주세요.",
        )
        .internal("condition_snapshot has neither a `conditions` nor a `reward` key"))
    }

    /// Whether the local weekday and hour allow spending right now (§8.5, REDEEM-003).
    pub fn allows_moment(&self, now: DateTime<Utc>) -> bool {
        let zone = resolve_timezone(&self.timezone);
        let local = now.with_timezone(&zone);

        if !self.allowed_weekdays.is_empty() {
            let weekday = i16::try_from(local.weekday().num_days_from_sunday()).unwrap_or(0);
            if !self.allowed_weekdays.contains(&weekday) {
                return false;
            }
        }

        if self.allowed_local_time_ranges.is_empty() {
            return true;
        }

        let time = NaiveTime::from_hms_opt(local.hour(), local.minute(), local.second())
            .unwrap_or(NaiveTime::MIN);
        self.allowed_local_time_ranges
            .iter()
            .any(|range| range.contains(time))
    }
}

/// The `reward` object Phase 2 freezes into a loyalty coupon's snapshot.
#[derive(Debug, Deserialize)]
struct LoyaltyRewardShape {
    benefit_type: BenefitType,
    #[serde(default)]
    fixed_amount: Option<i64>,
    #[serde(default)]
    percentage: Option<i16>,
    #[serde(default)]
    maximum_discount_amount: Option<i64>,
    #[serde(default)]
    free_item_ids: Vec<Uuid>,
    #[serde(default)]
    minimum_order_amount: i64,
}

fn malformed_snapshot(error: serde_json::Error) -> ApiError {
    ApiError::with_message(
        ErrorCode::RequiresAdminReview,
        "쿠폰 조건을 해석할 수 없습니다. 고객센터에 문의해 주세요.",
    )
    .internal(format!("condition_snapshot is malformed: {error}"))
}

/// The item a 무료 품목 coupon actually gave away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct FreeItemAward {
    pub catalog_item_id: Option<Uuid>,
    pub name_snapshot: String,
    pub unit_price: i64,
}

/// What a coupon is worth against one order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Discount {
    /// The part of the order the benefit applies to, after item restrictions (§8.3).
    pub eligible_amount: i64,
    pub discount_amount: i64,
    /// Set only for 무료 품목.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_item: Option<FreeItemAward>,
}

impl Discount {
    /// The amount the customer would still owe. Never negative (§5.3).
    pub fn payable_amount(&self, gross_amount: i64) -> i64 {
        (gross_amount - self.discount_amount).max(0)
    }
}

/// Work out what a coupon takes off one order (§8.2, §5.3).
///
/// Fails rather than returning zero when the order does not qualify: "이 쿠폰은 0원
/// 할인입니다" is not an answer an owner can act on, and REDEEM-003 wants a reason.
pub fn calculate(
    conditions: &CouponConditions,
    lines: &[OrderLine],
    gross_amount: i64,
) -> ApiResult<Discount> {
    let restriction = conditions.restriction();
    let eligibility = restriction.evaluate(lines, gross_amount);

    if let Some(rejection) = eligibility.rejection() {
        return Err(rejection);
    }

    // §8.3, and the reason it is stated separately from the restriction: a 무료 품목
    // coupon is item-restricted by construction even when no `eligible_item_ids` were set,
    // so an order typed without line items cannot be judged and must not be approved.
    if conditions.benefit.benefit_type == BenefitType::FreeItem && lines.is_empty() {
        return Err(ApiError::with_message(
            ErrorCode::ItemNotEligible,
            "무료 품목 쿠폰은 주문 품목을 입력해야 승인할 수 있습니다.",
        ));
    }

    if eligibility.eligible_amount < conditions.minimum_order_amount {
        return Err(ApiError::with_message(
            ErrorCode::MinimumOrderNotMet,
            format!(
                "최소 주문 금액까지 {}원 부족합니다.",
                conditions.minimum_order_amount - eligibility.eligible_amount
            ),
        ));
    }

    let eligible_amount = eligibility.eligible_amount;
    let benefit = &conditions.benefit;

    let (raw_discount, free_item) = match benefit.benefit_type {
        BenefitType::FixedAmount => (benefit.fixed_amount.unwrap_or(0), None),

        BenefitType::Percentage => {
            let percentage = i64::from(benefit.percentage.unwrap_or(0)).clamp(0, 100);
            // §5.3: 대상 금액 × 할인율에서 1원 미만을 버린다. Both operands are
            // non-negative, so integer division is the floor.
            let raw = eligible_amount.saturating_mul(percentage) / 100;
            let capped = match benefit.maximum_discount_amount {
                Some(maximum) if maximum >= 0 => raw.min(maximum),
                _ => raw,
            };
            (capped, None)
        }

        BenefitType::FreeItem => {
            // §8.2: 여러 대상 품목을 주문했다면 가장 낮은 단가 1개를 무료 처리한다.
            let award = lines
                .iter()
                .filter(|line| {
                    line.catalog_item_id
                        .is_some_and(|id| benefit.free_item_ids.contains(&id))
                        && restriction.accepts(line)
                })
                .min_by(|left, right| {
                    left.unit_price.cmp(&right.unit_price).then_with(|| {
                        left.name_snapshot.cmp(&right.name_snapshot)
                    })
                })
                .ok_or_else(|| {
                    ApiError::with_message(
                        ErrorCode::ItemNotEligible,
                        "무료 대상 품목이 주문에 포함되어 있지 않습니다.",
                    )
                })?;

            (
                award.unit_price,
                Some(FreeItemAward {
                    catalog_item_id: award.catalog_item_id,
                    name_snapshot: award.name_snapshot.clone(),
                    unit_price: award.unit_price,
                }),
            )
        }
    };

    // §8.2: 실제 할인액은 대상 주문액을 넘지 않는다. §5.3: 최종 결제 예정액은 0원
    // 미만이 될 수 없다. Both are the same clamp seen from two sides.
    let discount_amount = raw_discount.max(0).min(eligible_amount).min(gross_amount);

    Ok(Discount {
        eligible_amount,
        discount_amount,
        free_item,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(item: Option<u128>, quantity: i64, unit_price: i64) -> OrderLine {
        OrderLine {
            catalog_item_id: item.map(Uuid::from_u128),
            category_id: None,
            name_snapshot: format!("품목-{}", item.unwrap_or(0)),
            quantity,
            unit_price,
        }
    }

    fn conditions(benefit: Benefit) -> CouponConditions {
        CouponConditions {
            schema_version: CONDITION_SCHEMA_VERSION,
            benefit,
            minimum_order_amount: 0,
            eligible_item_ids: Vec::new(),
            eligible_category_ids: Vec::new(),
            excluded_item_ids: Vec::new(),
            allowed_weekdays: Vec::new(),
            allowed_local_time_ranges: Vec::new(),
            timezone: "Asia/Seoul".to_owned(),
            external_discount_stackable: false,
        }
    }

    fn fixed(amount: i64) -> Benefit {
        Benefit {
            benefit_type: BenefitType::FixedAmount,
            fixed_amount: Some(amount),
            percentage: None,
            maximum_discount_amount: None,
            free_item_ids: Vec::new(),
        }
    }

    fn percentage(percentage: i16, maximum: i64) -> Benefit {
        Benefit {
            benefit_type: BenefitType::Percentage,
            fixed_amount: None,
            percentage: Some(percentage),
            maximum_discount_amount: Some(maximum),
            free_item_ids: Vec::new(),
        }
    }

    fn free_item(ids: &[u128]) -> Benefit {
        Benefit {
            benefit_type: BenefitType::FreeItem,
            fixed_amount: None,
            percentage: None,
            maximum_discount_amount: None,
            free_item_ids: ids.iter().map(|id| Uuid::from_u128(*id)).collect(),
        }
    }

    #[test]
    fn a_fixed_discount_is_taken_off_the_order() {
        let discount = calculate(&conditions(fixed(3_000)), &[], 12_000).expect("applies");
        assert_eq!(discount.discount_amount, 3_000);
        assert_eq!(discount.payable_amount(12_000), 9_000);
    }

    #[test]
    fn a_fixed_discount_never_exceeds_the_order_it_applies_to() {
        // §8.2: 실제 할인액은 대상 주문액을 넘지 않는다.
        let discount = calculate(&conditions(fixed(10_000)), &[], 4_000).expect("applies");
        assert_eq!(discount.discount_amount, 4_000);
        assert_eq!(discount.payable_amount(4_000), 0, "결제액은 음수가 될 수 없다");
    }

    #[test]
    fn a_percentage_discount_drops_the_fraction_of_a_won() {
        // §5.3: 대상 금액 × 할인율에서 1원 미만을 버린다. 12,345 × 10% = 1,234.5 → 1,234.
        let discount =
            calculate(&conditions(percentage(10, 100_000)), &[], 12_345).expect("applies");
        assert_eq!(discount.discount_amount, 1_234);
    }

    #[test]
    fn the_fraction_is_dropped_before_the_ceiling_is_applied() {
        // 9,999 × 33% = 3,299.67 → 3,299, and the 5,000 ceiling does not bite.
        let discount =
            calculate(&conditions(percentage(33, 5_000)), &[], 9_999).expect("applies");
        assert_eq!(discount.discount_amount, 3_299);
    }

    #[test]
    fn a_percentage_discount_stops_at_its_maximum() {
        let discount =
            calculate(&conditions(percentage(50, 3_000)), &[], 100_000).expect("applies");
        assert_eq!(discount.discount_amount, 3_000);
    }

    #[test]
    fn a_hundred_percent_discount_is_the_whole_eligible_amount() {
        let discount =
            calculate(&conditions(percentage(100, 1_000_000)), &[], 8_000).expect("applies");
        assert_eq!(discount.discount_amount, 8_000);
        assert_eq!(discount.payable_amount(8_000), 0);
    }

    #[test]
    fn a_percentage_applies_to_the_eligible_part_only() {
        let mut conditions = conditions(percentage(10, 100_000));
        conditions.eligible_item_ids = vec![Uuid::from_u128(1)];

        // 12,000 of coffee qualifies; the 9,000 cake does not.
        let discount = calculate(
            &conditions,
            &[line(Some(1), 2, 6_000), line(Some(2), 1, 9_000)],
            21_000,
        )
        .expect("applies");

        assert_eq!(discount.eligible_amount, 12_000);
        assert_eq!(discount.discount_amount, 1_200);
    }

    #[test]
    fn a_free_item_coupon_gives_away_the_cheapest_named_item() {
        // §8.2: 여러 대상 품목을 주문했다면 가장 낮은 단가 1개를 무료 처리한다.
        let discount = calculate(
            &conditions(free_item(&[1, 2])),
            &[line(Some(1), 1, 6_000), line(Some(2), 1, 4_500)],
            10_500,
        )
        .expect("applies");

        assert_eq!(discount.discount_amount, 4_500);
        assert_eq!(
            discount.free_item.expect("names the item").unit_price,
            4_500
        );
    }

    #[test]
    fn a_free_item_coupon_gives_away_one_unit_not_the_whole_line() {
        let discount = calculate(
            &conditions(free_item(&[1])),
            &[line(Some(1), 3, 6_000)],
            18_000,
        )
        .expect("applies");

        assert_eq!(discount.discount_amount, 6_000, "1개만 무료 처리한다");
    }

    #[test]
    fn a_free_item_coupon_refuses_an_order_without_the_named_item() {
        let error = calculate(
            &conditions(free_item(&[1])),
            &[line(Some(9), 1, 6_000)],
            6_000,
        )
        .expect_err("nothing to give away");

        assert_eq!(error.code, ErrorCode::ItemNotEligible);
    }

    #[test]
    fn a_free_item_coupon_cannot_be_approved_without_line_items() {
        // §8.3: 주문 품목을 입력하지 않은 경우 품목 제한 쿠폰을 승인할 수 없다.
        let error =
            calculate(&conditions(free_item(&[1])), &[], 20_000).expect_err("nothing to judge");

        assert_eq!(error.code, ErrorCode::ItemNotEligible);
    }

    #[test]
    fn an_item_restricted_coupon_cannot_be_approved_without_line_items() {
        let mut conditions = conditions(fixed(2_000));
        conditions.eligible_item_ids = vec![Uuid::from_u128(1)];

        let error = calculate(&conditions, &[], 20_000).expect_err("nothing to judge");
        assert_eq!(error.code, ErrorCode::ItemNotEligible);
    }

    #[test]
    fn an_excluded_item_cannot_be_the_free_one() {
        // §8.3: 대상과 제외가 겹치면 제외가 우선한다.
        let mut conditions = conditions(free_item(&[1, 2]));
        conditions.excluded_item_ids = vec![Uuid::from_u128(2)];

        let discount = calculate(
            &conditions,
            &[line(Some(1), 1, 6_000), line(Some(2), 1, 4_500)],
            10_500,
        )
        .expect("applies");

        assert_eq!(
            discount.discount_amount, 6_000,
            "the cheaper item is excluded, so the coupon falls back to the eligible one"
        );
    }

    #[test]
    fn the_minimum_order_is_measured_against_the_eligible_amount() {
        let mut conditions = conditions(fixed(2_000));
        conditions.minimum_order_amount = 15_000;
        conditions.eligible_item_ids = vec![Uuid::from_u128(1)];

        // 20,000 on the bill, but only 12,000 of it qualifies.
        let error = calculate(
            &conditions,
            &[line(Some(1), 2, 6_000), line(Some(2), 1, 8_000)],
            20_000,
        )
        .expect_err("below the minimum");

        assert_eq!(error.code, ErrorCode::MinimumOrderNotMet);
    }

    #[test]
    fn a_weekday_window_only_allows_its_days() {
        let mut conditions = conditions(fixed(1_000));
        // 1 = Monday, counting from Sunday.
        conditions.allowed_weekdays = vec![1];

        // 2026-08-10 is a Monday; 2026-08-11 a Tuesday. Both at 03:00Z = noon in Seoul.
        let monday = "2026-08-10T03:00:00Z".parse::<DateTime<Utc>>().expect("time");
        let tuesday = "2026-08-11T03:00:00Z".parse::<DateTime<Utc>>().expect("time");

        assert!(conditions.allows_moment(monday));
        assert!(!conditions.allows_moment(tuesday));
    }

    #[test]
    fn a_time_window_is_judged_in_the_stores_local_time() {
        let mut conditions = conditions(fixed(1_000));
        conditions.allowed_local_time_ranges = vec![LocalTimeRange {
            start: "11:00".to_owned(),
            end: "14:00".to_owned(),
        }];

        // 03:00Z is 12:00 in Seoul; 06:00Z is 15:00.
        assert!(conditions.allows_moment(
            "2026-08-10T03:00:00Z".parse::<DateTime<Utc>>().expect("time")
        ));
        assert!(!conditions.allows_moment(
            "2026-08-10T06:00:00Z".parse::<DateTime<Utc>>().expect("time")
        ));
    }

    #[test]
    fn a_window_ending_at_the_boundary_excludes_it() {
        let range = LocalTimeRange {
            start: "11:00".to_owned(),
            end: "14:00".to_owned(),
        };
        assert!(range.contains(NaiveTime::from_hms_opt(11, 0, 0).expect("time")));
        assert!(!range.contains(NaiveTime::from_hms_opt(14, 0, 0).expect("time")), "[start, end)");
    }

    #[test]
    fn a_late_night_window_wraps_past_midnight() {
        let range = LocalTimeRange {
            start: "22:00".to_owned(),
            end: "02:00".to_owned(),
        };
        assert!(range.contains(NaiveTime::from_hms_opt(23, 30, 0).expect("time")));
        assert!(range.contains(NaiveTime::from_hms_opt(1, 0, 0).expect("time")));
        assert!(!range.contains(NaiveTime::from_hms_opt(12, 0, 0).expect("time")));
    }

    #[test]
    fn no_window_at_all_means_any_moment() {
        let conditions = conditions(fixed(1_000));
        assert!(conditions.allows_moment(Utc::now()));
    }

    #[test]
    fn a_benefit_must_match_its_own_type() {
        assert!(fixed(1_000).validate().is_ok());
        assert!(percentage(10, 5_000).validate().is_ok());
        assert!(free_item(&[1]).validate().is_ok());

        // §8.2: 정률 할인은 최대 할인액을 필수로 둔다.
        let uncapped = Benefit {
            maximum_discount_amount: None,
            ..percentage(10, 0)
        };
        assert_eq!(
            uncapped.validate().expect_err("needs a ceiling").code,
            ErrorCode::ValidationFailed
        );

        let no_items = Benefit {
            free_item_ids: Vec::new(),
            ..free_item(&[])
        };
        assert!(no_items.validate().is_err());

        let zero_amount = Benefit {
            fixed_amount: Some(0),
            ..fixed(0)
        };
        assert!(zero_amount.validate().is_err());

        for out_of_range in [0i16, 101] {
            let benefit = Benefit {
                percentage: Some(out_of_range),
                ..percentage(10, 5_000)
            };
            assert!(benefit.validate().is_err(), "{out_of_range}% must be refused");
        }
    }

    #[test]
    fn a_campaign_snapshot_round_trips() {
        let mut original = conditions(percentage(15, 4_000));
        original.minimum_order_amount = 10_000;
        original.allowed_weekdays = vec![1, 2, 3];

        let snapshot = serde_json::json!({
            "schema_version": 1,
            "source": "CAMPAIGN",
            "conditions": serde_json::to_value(&original).expect("serialises"),
        });

        assert_eq!(
            CouponConditions::from_snapshot(&snapshot).expect("parses"),
            original
        );
    }

    #[test]
    fn a_loyalty_reward_snapshot_is_understood_by_the_same_calculator() {
        // A reward coupon is spent through exactly the same path (§13.3), so its Phase 2
        // snapshot shape has to be readable here.
        let snapshot = serde_json::json!({
            "schema_version": 1,
            "source": "LOYALTY_REWARD",
            "store": { "id": Uuid::nil(), "name": "빵집", "timezone": "Asia/Seoul" },
            "reward": {
                "benefit_type": "FIXED_AMOUNT",
                "fixed_amount": 5_000,
                "percentage": null,
                "maximum_discount_amount": null,
                "free_item_ids": [],
                "minimum_order_amount": 3_000,
                "validity_days": 30,
                "title": "아메리카노 5천원 할인",
                "description": "",
                "customer_notice": "",
            },
        });

        let conditions = CouponConditions::from_snapshot(&snapshot).expect("parses");
        assert_eq!(conditions.benefit.benefit_type, BenefitType::FixedAmount);
        assert_eq!(conditions.minimum_order_amount, 3_000);

        let discount = calculate(&conditions, &[], 12_000).expect("applies");
        assert_eq!(discount.discount_amount, 5_000);
    }

    #[test]
    fn an_unreadable_snapshot_asks_for_a_human_rather_than_guessing() {
        let error = CouponConditions::from_snapshot(&serde_json::json!({ "nothing": true }))
            .expect_err("cannot be read");
        assert_eq!(error.code, ErrorCode::RequiresAdminReview);
    }
}
