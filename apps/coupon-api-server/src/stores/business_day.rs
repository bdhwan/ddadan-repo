//! Store-local business days (§5.2, §2.3).
//!
//! A daily limit is not "per UTC day" and not even "per local calendar day": it runs from
//! the store's cutover time to the next one. A bakery that closes at 02:00 wants the
//! 01:30 sale counted against the day it belongs to commercially.
//!
//! Everything here takes an explicit `now`, so the calculation is testable and so callers
//! are pushed towards passing the *database's* clock rather than the process's (§5.2:
//! server time, not client time).

use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::error::{ApiError, ApiResult, ErrorCode};

/// Fallback when a store carries a zone this build's tz database does not know.
///
/// Failing closed to the launch default (§23.1) keeps accrual working; the alternative —
/// refusing every scan — punishes the customer for an operator's typo.
pub const DEFAULT_TIMEZONE: &str = "Asia/Seoul";

/// Resolve an IANA name, falling back to the launch default with a warning.
pub fn resolve_timezone(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or_else(|_| {
        tracing::warn!(timezone = name, "unknown store timezone; using the default");
        DEFAULT_TIMEZONE
            .parse::<Tz>()
            .expect("the default timezone is always known")
    })
}

/// Parse `HH:MM:SS` as PostgreSQL renders a `time` column.
pub fn parse_cutoff(raw: &str) -> ApiResult<NaiveTime> {
    NaiveTime::parse_from_str(raw.trim(), "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(raw.trim(), "%H:%M"))
        .map_err(|error| {
            ApiError::with_message(
                ErrorCode::ValidationFailed,
                "영업일 기준 시각을 해석할 수 없습니다.",
            )
            .internal(error.to_string())
        })
}

/// Everything about one store's day boundaries, resolved once per request.
#[derive(Debug, Clone, Copy)]
pub struct BusinessCalendar {
    pub timezone: Tz,
    pub cutoff: NaiveTime,
}

impl BusinessCalendar {
    pub fn new(timezone: &str, cutoff: NaiveTime) -> Self {
        Self {
            timezone: resolve_timezone(timezone),
            cutoff,
        }
    }

    /// Which business day `at` belongs to.
    ///
    /// Shifting the local wall clock back by the cutoff turns "day starting at 06:00"
    /// into an ordinary calendar date, which is exactly what the `business_day date`
    /// column stores.
    pub fn business_day(&self, at: DateTime<Utc>) -> NaiveDate {
        let local = at.with_timezone(&self.timezone).naive_local();
        let since_midnight = self.cutoff.signed_duration_since(NaiveTime::MIN);
        (local - since_midnight).date()
    }

    /// The instant a given business day starts, in UTC.
    pub fn business_day_start(&self, day: NaiveDate) -> DateTime<Utc> {
        self.resolve_local(day, self.cutoff)
    }

    /// The instant the business day containing `at` rolls over. STAMP-005 shows this to
    /// the owner when a daily limit is already used up.
    pub fn next_business_day_start(&self, at: DateTime<Utc>) -> DateTime<Utc> {
        let next = self
            .business_day(at)
            .checked_add_days(Days::new(1))
            .unwrap_or(NaiveDate::MAX);
        self.business_day_start(next)
    }

    /// Turn a local date and time into an instant, coping with DST.
    ///
    /// A spring-forward gap has no such local time at all, so the first valid instant
    /// after it is used; a fall-back overlap has two, and the earlier one is taken so the
    /// day is never shortened.
    fn resolve_local(&self, day: NaiveDate, time: NaiveTime) -> DateTime<Utc> {
        let naive = day.and_time(time);

        match self.timezone.from_local_datetime(&naive) {
            chrono::LocalResult::Single(resolved) => resolved.with_timezone(&Utc),
            chrono::LocalResult::Ambiguous(earliest, _) => earliest.with_timezone(&Utc),
            chrono::LocalResult::None => {
                // Walk forward a minute at a time until the clock exists again. A DST gap
                // is at most a couple of hours, so this terminates quickly.
                let mut probe = naive;
                for _ in 0..(6 * 60) {
                    probe += chrono::Duration::minutes(1);
                    if let chrono::LocalResult::Single(resolved) =
                        self.timezone.from_local_datetime(&probe)
                    {
                        return resolved.with_timezone(&Utc);
                    }
                }
                // Unreachable for any real zone; a fixed answer beats a panic.
                Utc.from_utc_datetime(&naive)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    fn seoul(cutoff: &str) -> BusinessCalendar {
        BusinessCalendar::new("Asia/Seoul", parse_cutoff(cutoff).expect("valid cutoff"))
    }

    #[test]
    fn midnight_cutoff_is_the_local_calendar_date() {
        let calendar = seoul("00:00:00");

        // 2026-08-10T15:00Z is 2026-08-11 00:00 KST.
        assert_eq!(
            calendar.business_day(utc("2026-08-10T15:00:00Z")),
            NaiveDate::from_ymd_opt(2026, 8, 11).expect("date")
        );
        assert_eq!(
            calendar.business_day(utc("2026-08-10T14:59:59Z")),
            NaiveDate::from_ymd_opt(2026, 8, 10).expect("date")
        );
    }

    #[test]
    fn an_early_morning_sale_belongs_to_the_previous_business_day() {
        let calendar = seoul("06:00:00");

        // 03:00 KST on the 11th is still the 10th's business day.
        assert_eq!(
            calendar.business_day(utc("2026-08-10T18:00:00Z")),
            NaiveDate::from_ymd_opt(2026, 8, 10).expect("date")
        );
        // 06:00 KST on the 11th starts the new one — the boundary is inclusive at the
        // start (§5.2 `[start, end)`).
        assert_eq!(
            calendar.business_day(utc("2026-08-10T21:00:00Z")),
            NaiveDate::from_ymd_opt(2026, 8, 11).expect("date")
        );
    }

    #[test]
    fn the_business_day_start_is_the_cutoff_in_local_time() {
        let calendar = seoul("06:00:00");
        let start = calendar.business_day_start(NaiveDate::from_ymd_opt(2026, 8, 11).expect("date"));

        assert_eq!(start, utc("2026-08-10T21:00:00Z"));
        assert_eq!(
            calendar.business_day(start),
            NaiveDate::from_ymd_opt(2026, 8, 11).expect("date"),
            "the first instant of a day must belong to it"
        );
        assert_eq!(
            calendar.business_day(start - chrono::Duration::seconds(1)),
            NaiveDate::from_ymd_opt(2026, 8, 10).expect("date"),
            "and the instant before it must not"
        );
    }

    #[test]
    fn the_next_rollover_is_exactly_one_day_later() {
        let calendar = seoul("06:00:00");
        let next = calendar.next_business_day_start(utc("2026-08-10T18:00:00Z"));

        assert_eq!(next, utc("2026-08-10T21:00:00Z"));
        assert!(next > utc("2026-08-10T18:00:00Z"));
    }

    #[test]
    fn a_spring_forward_gap_resolves_to_the_first_instant_that_exists() {
        // America/New_York jumps 02:00 → 03:00 on 2026-03-08, so a 02:30 cutoff has no
        // local time that day.
        let calendar =
            BusinessCalendar::new("America/New_York", parse_cutoff("02:30").expect("cutoff"));
        let start = calendar.business_day_start(NaiveDate::from_ymd_opt(2026, 3, 8).expect("date"));

        assert_eq!(
            start,
            utc("2026-03-08T07:00:00Z"),
            "the day starts the moment the clock reaches 03:00 local"
        );
    }

    #[test]
    fn a_fall_back_overlap_takes_the_earlier_instant() {
        // 01:30 happens twice on 2026-11-01 in New York.
        let calendar =
            BusinessCalendar::new("America/New_York", parse_cutoff("01:30").expect("cutoff"));
        let start = calendar.business_day_start(NaiveDate::from_ymd_opt(2026, 11, 1).expect("date"));

        assert_eq!(start, utc("2026-11-01T05:30:00Z"), "EDT, not EST");
    }

    #[test]
    fn an_unknown_timezone_falls_back_rather_than_failing_the_scan() {
        let calendar = BusinessCalendar::new("Mars/Olympus", parse_cutoff("00:00").expect("cutoff"));
        assert_eq!(calendar.timezone, resolve_timezone(DEFAULT_TIMEZONE));
    }

    #[test]
    fn cutoffs_parse_at_both_precisions_and_reject_nonsense() {
        assert_eq!(
            parse_cutoff("06:00:00").expect("parses"),
            NaiveTime::from_hms_opt(6, 0, 0).expect("time")
        );
        assert_eq!(
            parse_cutoff(" 06:00 ").expect("parses"),
            NaiveTime::from_hms_opt(6, 0, 0).expect("time")
        );
        for invalid in ["", "25:00:00", "abc"] {
            assert_eq!(
                parse_cutoff(invalid).expect_err("must reject").code,
                ErrorCode::ValidationFailed
            );
        }
    }
}
