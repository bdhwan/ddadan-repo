//! 목적·동의·야간 발송 판정 (§15.3, NOTIFY-001, NOTIFY-002, NOTIFY-004).
//!
//! Everything here is a pure function over a snapshot. That is the point: §19.1 lists
//! 알림 목적·동의·야간 발송 판정 as unit-testable behaviour, and the judgement that decides
//! whether a message may leave the platform should not need a database to be checked.
//!
//! The one rule worth stating twice, because it is the difference between a lawful send
//! and an unlawful one: **the decision made here is not durable.** §15.3 requires the
//! consent to be re-read immediately before the provider call, so this function is called
//! twice — once when the delivery row is created, and once by the worker with a freshly
//! loaded snapshot (NOTIFY-001).

use chrono::{DateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::notifications::NotificationChannel;

/// Why a message is being sent. §15.3 turns this into a legal basis, so it is stored on
/// the delivery rather than re-derived: a later template reclassification must not rewrite
/// the basis a past send was made on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationPurpose {
    /// 거래 완료 — a receipt for something the user just did (§15.3).
    Transactional,
    /// 서비스 안내 — expiry warnings, store status. Informational rather than promotional.
    Informational,
    /// 신규 캠페인, 장기 미방문 유도 (§15.3).
    Marketing,
    /// 계정 보안 (§15.3: 거래 완료·계정 보안은 정보성 서비스 알림이다).
    Security,
}

impl NotificationPurpose {
    pub fn as_db(self) -> &'static str {
        match self {
            NotificationPurpose::Transactional => "TRANSACTIONAL",
            NotificationPurpose::Informational => "INFORMATIONAL",
            NotificationPurpose::Marketing => "MARKETING",
            NotificationPurpose::Security => "SECURITY",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        Some(match raw {
            "TRANSACTIONAL" => NotificationPurpose::Transactional,
            "INFORMATIONAL" => NotificationPurpose::Informational,
            "MARKETING" => NotificationPurpose::Marketing,
            "SECURITY" => NotificationPurpose::Security,
            _ => return None,
        })
    }

    /// Whether §17.2's 야간 발송 동의 요건 applies. Only advertising is time-restricted;
    /// a receipt or a security alert is not something to sit on until morning.
    pub fn is_quiet_hours_restricted(self) -> bool {
        matches!(self, NotificationPurpose::Marketing)
    }
}

/// Why a channel was not used. Recorded on the delivery so "we did not send" is always
/// answerable (§15.4 `SUPPRESSED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SuppressionReason {
    /// No consent, or it was withdrawn — including between enqueue and send (NOTIFY-001).
    ConsentMissing,
    /// 상점별 마케팅 동의만 있고 전체 채널 동의가 없거나, 그 반대 (§15.3).
    StoreConsentMissing,
    /// The browser has no active push subscription (§15.1-2).
    NoActiveSubscription,
    /// §15.1: 마케팅 성격 메시지를 알림톡으로 보내지 않는다.
    ChannelNotPermittedForPurpose,
    /// No approved, active template for this code/channel/locale (§15.2).
    TemplateUnavailable,
    /// NOTIFY-004: 만료 시각 전에 전달할 수 없는 지연 알림은 취소한다.
    ExpiredBeforeDelivery,
    /// The account is suspended or withdrawn.
    RecipientInactive,
    /// Consent exists but there is no address to send to — no live browser token, or no
    /// contact the 알림톡 provider can reach. Distinct from [`Self::ConsentMissing`]
    /// because the remedy is the user's, not ours.
    RecipientUnreachable,
}

impl SuppressionReason {
    pub fn as_db(self) -> &'static str {
        match self {
            SuppressionReason::ConsentMissing => "CONSENT_MISSING",
            SuppressionReason::StoreConsentMissing => "STORE_CONSENT_MISSING",
            SuppressionReason::NoActiveSubscription => "NO_ACTIVE_SUBSCRIPTION",
            SuppressionReason::ChannelNotPermittedForPurpose => "CHANNEL_NOT_PERMITTED",
            SuppressionReason::TemplateUnavailable => "TEMPLATE_UNAVAILABLE",
            SuppressionReason::ExpiredBeforeDelivery => "EXPIRED_BEFORE_DELIVERY",
            SuppressionReason::RecipientInactive => "RECIPIENT_INACTIVE",
            SuppressionReason::RecipientUnreachable => "RECIPIENT_UNREACHABLE",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        Some(match raw {
            "CONSENT_MISSING" => SuppressionReason::ConsentMissing,
            "STORE_CONSENT_MISSING" => SuppressionReason::StoreConsentMissing,
            "NO_ACTIVE_SUBSCRIPTION" => SuppressionReason::NoActiveSubscription,
            "CHANNEL_NOT_PERMITTED" => SuppressionReason::ChannelNotPermittedForPurpose,
            "TEMPLATE_UNAVAILABLE" => SuppressionReason::TemplateUnavailable,
            "EXPIRED_BEFORE_DELIVERY" => SuppressionReason::ExpiredBeforeDelivery,
            "RECIPIENT_INACTIVE" => SuppressionReason::RecipientInactive,
            "RECIPIENT_UNREACHABLE" => SuppressionReason::RecipientUnreachable,
            _ => return None,
        })
    }
}

/// Everything the judgement needs, read from the database at the moment of the decision.
///
/// A struct rather than a set of arguments so the *second* evaluation — the one the worker
/// does immediately before calling the provider — is visibly the same computation over
/// fresher data, not a different rule written twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsentSnapshot {
    /// `TRANSACTIONAL` on `FCM_WEB_PUSH` — 서비스 거래 Web Push 동의.
    pub web_push_transactional: bool,
    /// `MARKETING` on `FCM_WEB_PUSH`, account-wide (§15.3 전체 채널 동의).
    pub marketing_all_channels: bool,
    /// `MARKETING` on `FCM_WEB_PUSH` for the store this message is about.
    pub marketing_this_store: bool,
    /// `INFORMATIONAL` on `KAKAO_ALIMTALK` — 카카오 정보성 알림 동의.
    pub alimtalk_informational: bool,
    /// At least one browser subscription in `ACTIVE` (§15.1-2).
    pub has_active_push_subscription: bool,
    /// The account can still receive anything at all.
    pub recipient_active: bool,
}

/// The judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    Allowed,
    Suppressed(SuppressionReason),
}

impl Eligibility {
    pub fn is_allowed(self) -> bool {
        matches!(self, Eligibility::Allowed)
    }

    pub fn reason(self) -> Option<SuppressionReason> {
        match self {
            Eligibility::Allowed => None,
            Eligibility::Suppressed(reason) => Some(reason),
        }
    }
}

/// May this purpose travel on this channel, for this recipient, right now?
///
/// The in-app channel is deliberately unconditional: §15.1 makes it the 기준 기록 of every
/// transactional and operational event, and NOTIFY-001 says the in-app copy cannot be
/// switched off even though external delivery can. Suppressing it would mean an event
/// happened with no record the user can see.
pub fn evaluate(
    channel: NotificationChannel,
    purpose: NotificationPurpose,
    consent: &ConsentSnapshot,
) -> Eligibility {
    if channel == NotificationChannel::InApp {
        return Eligibility::Allowed;
    }

    if !consent.recipient_active {
        return Eligibility::Suppressed(SuppressionReason::RecipientInactive);
    }

    match channel {
        NotificationChannel::InApp => Eligibility::Allowed,

        NotificationChannel::FcmWebPush => {
            if !consent.has_active_push_subscription {
                return Eligibility::Suppressed(SuppressionReason::NoActiveSubscription);
            }

            match purpose {
                // NOTIFY-002: 보안·계정 변경은 가능하면 발송한다. Having granted the
                // browser permission *is* the consent that matters for a security alert
                // about that account; requiring a marketing-style opt-in on top would mean
                // the users most at risk are the least likely to hear about it.
                NotificationPurpose::Security => Eligibility::Allowed,

                NotificationPurpose::Transactional | NotificationPurpose::Informational => {
                    if consent.web_push_transactional {
                        Eligibility::Allowed
                    } else {
                        Eligibility::Suppressed(SuppressionReason::ConsentMissing)
                    }
                }

                // §15.3: 상점별 마케팅 동의와 전체 채널 동의를 *모두* 통과해야 한다.
                NotificationPurpose::Marketing => {
                    if !consent.marketing_all_channels {
                        Eligibility::Suppressed(SuppressionReason::ConsentMissing)
                    } else if !consent.marketing_this_store {
                        Eligibility::Suppressed(SuppressionReason::StoreConsentMissing)
                    } else {
                        Eligibility::Allowed
                    }
                }
            }
        }

        NotificationChannel::KakaoAlimtalk => {
            // §15.1: 일반 카카오 친구 메시지 API 를 대량 자동 알림 수단으로 쓰지 않는다.
            // 알림톡 carries approved 정보성 templates only, so a marketing message has no
            // lawful shape on this channel and is refused here rather than at the provider.
            if purpose == NotificationPurpose::Marketing {
                return Eligibility::Suppressed(SuppressionReason::ChannelNotPermittedForPurpose);
            }

            if consent.alimtalk_informational {
                Eligibility::Allowed
            } else {
                Eligibility::Suppressed(SuppressionReason::ConsentMissing)
            }
        }
    }
}

/// NOTIFY-004 야간 정책: 금지 시간의 마케팅 메시지는 다음 허용 시각으로 지연한다.
///
/// The window is the one 정보통신망법 draws around advertising — 21:00 to 08:00 — evaluated
/// in the recipient-facing local zone rather than UTC, because that is the clock the rule
/// is written against. §5.2's store timezone is what the caller passes in.
pub const QUIET_HOURS_START_HOUR: u32 = 21;
pub const QUIET_HOURS_END_HOUR: u32 = 8;

/// When this message may first be sent.
///
/// Returns `at` unchanged for anything that is not time-restricted, and for a restricted
/// message that is already inside the permitted window.
pub fn earliest_send_time(
    at: DateTime<Utc>,
    timezone: Tz,
    purpose: NotificationPurpose,
) -> DateTime<Utc> {
    if !purpose.is_quiet_hours_restricted() {
        return at;
    }

    let local = at.with_timezone(&timezone);
    let hour = local.time().hour_of_day();

    if (QUIET_HOURS_END_HOUR..QUIET_HOURS_START_HOUR).contains(&hour) {
        return at;
    }

    // Before 08:00 the next opening is today's; from 21:00 it is tomorrow's.
    let target_date = if hour < QUIET_HOURS_END_HOUR {
        local.date_naive()
    } else {
        local.date_naive().succ_opt().unwrap_or(local.date_naive())
    };

    let opening = NaiveTime::from_hms_opt(QUIET_HOURS_END_HOUR, 0, 0).expect("08:00 is a valid time");

    // A DST transition can make a local time ambiguous or absent. Neither happens in
    // Asia/Seoul, but the resolution has to be defined rather than unwrapped: the later of
    // an ambiguous pair, and `at` itself if the wall clock genuinely skipped the hour.
    timezone
        .from_local_datetime(&target_date.and_time(opening))
        .latest()
        .map(|resolved| resolved.with_timezone(&Utc))
        .unwrap_or(at)
}

/// `hour()` on a `NaiveTime` needs the `Timelike` trait in scope at every call site; this
/// keeps that import local to the one place it is used.
trait HourOfDay {
    fn hour_of_day(&self) -> u32;
}

impl HourOfDay for NaiveTime {
    fn hour_of_day(&self) -> u32 {
        use chrono::Timelike;
        self.hour()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEOUL: Tz = chrono_tz::Asia::Seoul;

    fn all_granted() -> ConsentSnapshot {
        ConsentSnapshot {
            web_push_transactional: true,
            marketing_all_channels: true,
            marketing_this_store: true,
            alimtalk_informational: true,
            has_active_push_subscription: true,
            recipient_active: true,
        }
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339)
            .expect("valid timestamp")
            .with_timezone(&Utc)
    }

    #[test]
    fn the_in_app_channel_is_never_suppressed() {
        // §15.1: 앱 내 알림은 모든 거래·운영 사건의 기준 기록이다. Even for a withdrawn
        // marketing consent, and even for an inactive account, the record still exists.
        for purpose in [
            NotificationPurpose::Transactional,
            NotificationPurpose::Informational,
            NotificationPurpose::Marketing,
            NotificationPurpose::Security,
        ] {
            assert_eq!(
                evaluate(
                    NotificationChannel::InApp,
                    purpose,
                    &ConsentSnapshot::default()
                ),
                Eligibility::Allowed
            );
        }
    }

    #[test]
    fn marketing_push_needs_both_the_account_and_the_store_consent() {
        // §15.3: 상점별 마케팅 동의와 전체 채널 동의를 모두 통과해야 한다.
        let mut consent = all_granted();
        assert!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Marketing,
                &consent
            )
            .is_allowed()
        );

        consent.marketing_this_store = false;
        assert_eq!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Marketing,
                &consent
            ),
            Eligibility::Suppressed(SuppressionReason::StoreConsentMissing)
        );

        let mut consent = all_granted();
        consent.marketing_all_channels = false;
        assert_eq!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Marketing,
                &consent
            ),
            Eligibility::Suppressed(SuppressionReason::ConsentMissing)
        );
    }

    #[test]
    fn a_transactional_push_needs_its_own_consent_and_a_live_subscription() {
        let mut consent = all_granted();
        consent.web_push_transactional = false;
        assert_eq!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Transactional,
                &consent
            ),
            Eligibility::Suppressed(SuppressionReason::ConsentMissing)
        );

        let mut consent = all_granted();
        consent.has_active_push_subscription = false;
        assert_eq!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Transactional,
                &consent
            ),
            Eligibility::Suppressed(SuppressionReason::NoActiveSubscription)
        );
    }

    #[test]
    fn a_security_alert_rides_the_browser_permission_alone() {
        // NOTIFY-002: 보안·계정 변경은 가능하면 발송한다.
        let mut consent = ConsentSnapshot {
            recipient_active: true,
            has_active_push_subscription: true,
            ..ConsentSnapshot::default()
        };
        assert!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Security,
                &consent
            )
            .is_allowed()
        );

        consent.has_active_push_subscription = false;
        assert_eq!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Security,
                &consent
            ),
            Eligibility::Suppressed(SuppressionReason::NoActiveSubscription)
        );
    }

    #[test]
    fn marketing_never_travels_on_alimtalk() {
        // §15.1: 알림톡은 승인된 정보성 템플릿 전용이다.
        assert_eq!(
            evaluate(
                NotificationChannel::KakaoAlimtalk,
                NotificationPurpose::Marketing,
                &all_granted()
            ),
            Eligibility::Suppressed(SuppressionReason::ChannelNotPermittedForPurpose)
        );
        assert!(
            evaluate(
                NotificationChannel::KakaoAlimtalk,
                NotificationPurpose::Informational,
                &all_granted()
            )
            .is_allowed()
        );
    }

    #[test]
    fn an_inactive_recipient_gets_no_external_message() {
        let consent = ConsentSnapshot {
            recipient_active: false,
            ..all_granted()
        };
        assert_eq!(
            evaluate(
                NotificationChannel::FcmWebPush,
                NotificationPurpose::Security,
                &consent
            ),
            Eligibility::Suppressed(SuppressionReason::RecipientInactive)
        );
    }

    #[test]
    fn only_marketing_waits_for_the_morning() {
        // 2026-08-11T13:00Z is 22:00 in Seoul — inside the quiet window.
        let late = at("2026-08-11T13:00:00Z");

        assert_eq!(
            earliest_send_time(late, SEOUL, NotificationPurpose::Transactional),
            late
        );
        assert_eq!(
            earliest_send_time(late, SEOUL, NotificationPurpose::Security),
            late
        );

        let deferred = earliest_send_time(late, SEOUL, NotificationPurpose::Marketing);
        assert!(deferred > late);
        // 08:00 the next Seoul morning is 2026-08-11T23:00Z.
        assert_eq!(deferred, at("2026-08-11T23:00:00Z"));
    }

    #[test]
    fn an_early_morning_marketing_message_waits_for_the_same_day() {
        // 2026-08-11T20:00Z is 05:00 on the 12th in Seoul.
        let dawn = at("2026-08-11T20:00:00Z");
        assert_eq!(
            earliest_send_time(dawn, SEOUL, NotificationPurpose::Marketing),
            at("2026-08-11T23:00:00Z"),
        );
    }

    #[test]
    fn a_marketing_message_inside_the_window_is_not_delayed() {
        // 2026-08-11T04:00Z is 13:00 in Seoul.
        let midday = at("2026-08-11T04:00:00Z");
        assert_eq!(
            earliest_send_time(midday, SEOUL, NotificationPurpose::Marketing),
            midday
        );
    }

    #[test]
    fn the_window_boundaries_are_half_open() {
        // 08:00 exactly is permitted; 21:00 exactly is not.
        let eight = at("2026-08-10T23:00:00Z"); // 08:00 Seoul on the 11th
        assert_eq!(
            earliest_send_time(eight, SEOUL, NotificationPurpose::Marketing),
            eight
        );

        let nine_pm = at("2026-08-11T12:00:00Z"); // 21:00 Seoul
        assert!(earliest_send_time(nine_pm, SEOUL, NotificationPurpose::Marketing) > nine_pm);
    }

    #[test]
    fn purposes_round_trip_through_their_database_spelling() {
        for purpose in [
            NotificationPurpose::Transactional,
            NotificationPurpose::Informational,
            NotificationPurpose::Marketing,
            NotificationPurpose::Security,
        ] {
            assert_eq!(NotificationPurpose::from_db(purpose.as_db()), Some(purpose));
        }
    }

    #[test]
    fn suppression_reasons_round_trip_through_their_database_spelling() {
        for reason in [
            SuppressionReason::ConsentMissing,
            SuppressionReason::StoreConsentMissing,
            SuppressionReason::NoActiveSubscription,
            SuppressionReason::ChannelNotPermittedForPurpose,
            SuppressionReason::TemplateUnavailable,
            SuppressionReason::ExpiredBeforeDelivery,
            SuppressionReason::RecipientInactive,
            SuppressionReason::RecipientUnreachable,
        ] {
            assert_eq!(SuppressionReason::from_db(reason.as_db()), Some(reason));
        }
    }
}
