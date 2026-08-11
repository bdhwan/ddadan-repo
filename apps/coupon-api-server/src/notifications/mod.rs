//! 앱 내 알림, FCM Web Push, 카카오 알림톡 (§10.2 `notifications`, §15).
//!
//! Three channels, one record. §15.1 makes the **in-app notification the base record of
//! every transactional and operational event**, and the external channels are attempts to
//! reach the same record somewhere else. That ordering is not cosmetic: NOTIFY-003 says a
//! provider failure must not roll back the coupon, and the way to guarantee that is for the
//! domain transaction to own nothing but an outbox row, and for everything to do with
//! sending to happen afterwards, in a job, against a record that already exists.
//!
//! So the flow is:
//!
//! ```text
//! domain tx ──commit──▶ outbox_events ──relay──▶ notifications (+ deliveries)
//!                                                     │
//!                                                     └──▶ notify_event job ──▶ provider
//! ```
//!
//! Everything downstream of the commit is retryable and idempotent, and a provider that is
//! down for a day costs a `FAILED_RETRYABLE` row rather than a coupon.
//!
//! Two rules deserve to be stated where they are implemented rather than only in §15:
//!
//! * **Consent is re-checked immediately before the provider call.** A withdrawal that
//!   lands after the job was enqueued must still stop the send (NOTIFY-001), so
//!   [`policy::evaluate`] runs twice over two different snapshots.
//! * **One send per `event + channel + template version + recipient`.** NOTIFY-004 names
//!   that key, and it is a unique index rather than a lookup, so two relays racing on the
//!   same outbox row cannot both produce a message.

pub mod delivery;
pub mod policy;
pub mod providers;
pub mod routes;
pub mod templates;

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::crypto::{LookupHash, Sealer};
use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::pagination::{Cursor, Page, PageQuery};
use crate::jobs::{JobKey, JobService, JobSpec, JobType};
use crate::notifications::policy::{
    ConsentSnapshot, Eligibility, NotificationPurpose, SuppressionReason,
};
use crate::notifications::templates::{RenderedMessage, TemplateRepository};

pub use routes::{me_notifications_router, notification_webhook_router};

/// Delivery channel. Mirrors `coupon.notification_channel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationChannel {
    InApp,
    FcmWebPush,
    KakaoAlimtalk,
}

impl NotificationChannel {
    pub fn as_db(self) -> &'static str {
        match self {
            NotificationChannel::InApp => "IN_APP",
            NotificationChannel::FcmWebPush => "FCM_WEB_PUSH",
            NotificationChannel::KakaoAlimtalk => "KAKAO_ALIMTALK",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        Some(match raw {
            "IN_APP" => NotificationChannel::InApp,
            "FCM_WEB_PUSH" => NotificationChannel::FcmWebPush,
            "KAKAO_ALIMTALK" => NotificationChannel::KakaoAlimtalk,
            _ => return None,
        })
    }

    /// The short form used in a job's unique key (§14.3 `…:fcm-template-2`).
    pub fn key_token(self) -> &'static str {
        match self {
            NotificationChannel::InApp => "inapp",
            NotificationChannel::FcmWebPush => "fcm",
            NotificationChannel::KakaoAlimtalk => "alimtalk",
        }
    }
}

/// `coupon.notification_delivery_status` (§15.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryStatus {
    Pending,
    Sending,
    Delivered,
    FailedRetryable,
    FailedPermanent,
    Suppressed,
}

impl DeliveryStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            DeliveryStatus::Pending => "PENDING",
            DeliveryStatus::Sending => "SENDING",
            DeliveryStatus::Delivered => "DELIVERED",
            DeliveryStatus::FailedRetryable => "FAILED_RETRYABLE",
            DeliveryStatus::FailedPermanent => "FAILED_PERMANENT",
            DeliveryStatus::Suppressed => "SUPPRESSED",
        }
    }

    pub fn from_db(raw: &str) -> Self {
        match raw {
            "SENDING" => DeliveryStatus::Sending,
            "DELIVERED" => DeliveryStatus::Delivered,
            "FAILED_RETRYABLE" => DeliveryStatus::FailedRetryable,
            "FAILED_PERMANENT" => DeliveryStatus::FailedPermanent,
            "SUPPRESSED" => DeliveryStatus::Suppressed,
            _ => DeliveryStatus::Pending,
        }
    }

    /// Whether nothing further will happen to this delivery.
    ///
    /// `FAILED_RETRYABLE` is deliberately not terminal: it is the waiting state between
    /// attempts, and only the retry budget turns it into `FAILED_PERMANENT`.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DeliveryStatus::Delivered | DeliveryStatus::FailedPermanent | DeliveryStatus::Suppressed
        )
    }
}

/// The §15.2 event table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationEvent {
    StampEarned,
    RewardIssued,
    CouponIssued,
    CouponExpiring,
    CouponUsed,
    TransactionVoided,
    StoreSuspended,
    StoreClosed,
    SecurityAlert,
}

impl NotificationEvent {
    /// The template code, which is also the value stored in `notifications.type`.
    pub fn code(self) -> &'static str {
        match self {
            NotificationEvent::StampEarned => "STAMP_EARNED",
            NotificationEvent::RewardIssued => "REWARD_ISSUED",
            NotificationEvent::CouponIssued => "COUPON_ISSUED",
            NotificationEvent::CouponExpiring => "COUPON_EXPIRING",
            NotificationEvent::CouponUsed => "COUPON_USED",
            NotificationEvent::TransactionVoided => "TRANSACTION_VOIDED",
            NotificationEvent::StoreSuspended => "STORE_SUSPENDED",
            NotificationEvent::StoreClosed => "STORE_CLOSED",
            NotificationEvent::SecurityAlert => "SECURITY_ALERT",
        }
    }

    pub fn from_code(raw: &str) -> Option<Self> {
        Some(match raw {
            "STAMP_EARNED" => NotificationEvent::StampEarned,
            "REWARD_ISSUED" => NotificationEvent::RewardIssued,
            "COUPON_ISSUED" => NotificationEvent::CouponIssued,
            "COUPON_EXPIRING" => NotificationEvent::CouponExpiring,
            "COUPON_USED" => NotificationEvent::CouponUsed,
            "TRANSACTION_VOIDED" => NotificationEvent::TransactionVoided,
            "STORE_SUSPENDED" => NotificationEvent::StoreSuspended,
            "STORE_CLOSED" => NotificationEvent::StoreClosed,
            "SECURITY_ALERT" => NotificationEvent::SecurityAlert,
            _ => return None,
        })
    }

    /// The domain event names the outbox actually carries (§14.2), mapped onto §15.2.
    ///
    /// Two redemption events collapse onto `TRANSACTION_VOIDED` because that is what §15.2
    /// calls the row — "원 거래, 복원 여부" describes a reversed accrual and a reversed use
    /// equally, and a customer does not distinguish them.
    pub fn from_outbox_event(raw: &str) -> Option<Self> {
        Some(match raw {
            "STAMP_EARNED" => NotificationEvent::StampEarned,
            "REWARD_ISSUED" => NotificationEvent::RewardIssued,
            "CAMPAIGN_COUPON_ISSUED" => NotificationEvent::CouponIssued,
            "COUPON_EXPIRING" => NotificationEvent::CouponExpiring,
            "COUPON_REDEEMED" => NotificationEvent::CouponUsed,
            "TRANSACTION_VOIDED" | "REDEMPTION_VOIDED" => NotificationEvent::TransactionVoided,
            "STORE_SUSPENDED" => NotificationEvent::StoreSuspended,
            "STORE_CLOSED" => NotificationEvent::StoreClosed,
            "SECURITY_ALERT" => NotificationEvent::SecurityAlert,
            _ => return None,
        })
    }

    /// §15.3's purpose judgement.
    ///
    /// `COUPON_EXPIRING` is classified 정보성 here. §15.3 leaves the final call to the
    /// pre-launch legal and provider review (§23.2), and this is the conservative reading:
    /// an expiry warning about a benefit the customer already holds is service information,
    /// not an advertisement for a new one. Reclassifying it is a one-line change *and* a
    /// template purpose change, which is the right amount of friction.
    pub fn purpose(self) -> NotificationPurpose {
        match self {
            NotificationEvent::StampEarned
            | NotificationEvent::RewardIssued
            | NotificationEvent::CouponUsed
            | NotificationEvent::TransactionVoided => NotificationPurpose::Transactional,

            NotificationEvent::CouponExpiring
            | NotificationEvent::StoreSuspended
            | NotificationEvent::StoreClosed => NotificationPurpose::Informational,

            // §15.3: 신규 할인 캠페인은 마케팅이다.
            NotificationEvent::CouponIssued => NotificationPurpose::Marketing,

            NotificationEvent::SecurityAlert => NotificationPurpose::Security,
        }
    }

    /// §15.2's 기본 우선순위 column.
    pub fn priority(self) -> &'static str {
        match self {
            NotificationEvent::SecurityAlert => "URGENT",
            NotificationEvent::RewardIssued
            | NotificationEvent::CouponUsed
            | NotificationEvent::TransactionVoided
            | NotificationEvent::StoreSuspended
            | NotificationEvent::StoreClosed => "HIGH",
            _ => "NORMAL",
        }
    }

    /// Which external channels are attempted, per NOTIFY-002.
    ///
    /// Store status changes stay in-app: they concern a shop the customer follows rather
    /// than something that just happened to their money, and pushing every suspension to
    /// every follower is how a notification channel gets muted.
    pub fn external_channels(self) -> &'static [NotificationChannel] {
        match self {
            NotificationEvent::StoreSuspended | NotificationEvent::StoreClosed => &[],
            NotificationEvent::SecurityAlert => &[
                NotificationChannel::FcmWebPush,
                NotificationChannel::KakaoAlimtalk,
            ],
            _ => &[
                NotificationChannel::FcmWebPush,
                NotificationChannel::KakaoAlimtalk,
            ],
        }
    }
}

/// One notification to create.
#[derive(Debug, Clone)]
pub struct NotificationRequest {
    pub user_id: Uuid,
    pub store_id: Option<Uuid>,
    /// The domain event this is *about*. Two relays of the same outbox row must produce
    /// the same value, because it is half of the dedupe key (NOTIFY-004).
    pub event_id: Uuid,
    pub event: NotificationEvent,
    pub correlation_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    /// After this, delivering is pointless — a coupon that has already expired, a hold that
    /// has already been released (NOTIFY-004 마지막 줄).
    pub expires_at: Option<DateTime<Utc>>,
    pub deep_link: Option<String>,
    pub variables: BTreeMap<String, String>,
    pub data: serde_json::Value,
    /// The store's timezone, for the quiet-hours judgement (§5.2, NOTIFY-004).
    pub timezone: Option<String>,
    /// The originating outbox row, so the relay can mark exactly what it consumed.
    pub source_event_type: Option<String>,
}

/// What `publish` did.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    pub notification_id: Uuid,
    /// True when this event had already produced a notification (NOTIFY-004 중복 방지).
    pub deduplicated: bool,
    /// The deliveries queued for an external channel, in the order they were created.
    pub queued_delivery_ids: Vec<Uuid>,
    pub suppressed: Vec<(NotificationChannel, SuppressionReason)>,
}

/// One row of the consumer's inbox (§11.3 `GET /me/notifications`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Notification {
    pub id: Uuid,
    pub store_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub notification_type: String,
    pub purpose: NotificationPurpose,
    pub priority: String,
    pub title: String,
    pub body: String,
    pub deep_link: Option<String>,
    pub data: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// `PATCH /me/notifications` (§11.3).
#[derive(Debug, Clone, Deserialize, ToSchema, validator::Validate)]
pub struct UpdateNotificationsRequest {
    /// Ids to act on. Empty with `all: true` means "everything currently unread".
    #[serde(default)]
    #[validate(length(max = 200, message = "한 번에 200개까지 처리할 수 있습니다."))]
    pub notification_ids: Vec<Uuid>,
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub action: NotificationAction,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationAction {
    #[default]
    MarkRead,
    MarkUnread,
    /// Hide from the inbox. §15.1: the record of the *event* lives in the ledger, so
    /// clearing a notification never touches a coupon or a transaction.
    Dismiss,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationUpdateResult {
    pub updated: i64,
    pub unread_count: i64,
}

/// `POST /me/push-subscriptions` (§15.1-2).
#[derive(Debug, Clone, Deserialize, ToSchema, validator::Validate)]
pub struct RegisterPushSubscriptionRequest {
    /// The FCM registration token. Stored encrypted with a keyed lookup hash beside it, so
    /// a duplicate registration is recognisable without the plaintext being searchable
    /// (§16.5).
    #[validate(length(min = 8, max = 4096, message = "푸시 토큰이 올바르지 않습니다."))]
    pub token: String,
    #[validate(length(max = 64))]
    pub browser_family: Option<String>,
    #[validate(length(max = 100))]
    pub device_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PushSubscription {
    pub id: Uuid,
    pub browser_family: Option<String>,
    pub device_label: Option<String>,
    pub status: String,
    pub disabled_reason: Option<String>,
    pub last_seen_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PushSubscriptionsResponse {
    pub subscriptions: Vec<PushSubscription>,
}

/// One preference row to write: "for this user (optionally at this store), this purpose
/// on this channel is on/off, because of this consent event".
#[derive(Debug, Clone)]
pub struct PreferenceUpdate {
    pub user_id: Uuid,
    pub store_id: Option<Uuid>,
    pub purpose: String,
    pub channel: NotificationChannel,
    pub enabled: bool,
    pub source_consent_event_id: Uuid,
}

/// The seam other modules use to reach `notification_preferences`.
pub struct NotificationPreferenceService;

impl Default for NotificationPreferenceService {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationPreferenceService {
    pub fn new() -> Self {
        Self
    }

    /// Project a consent decision onto the preference table, inside the caller's
    /// transaction so consent evidence and its effect commit together.
    pub async fn apply(&self, tx: &mut Tx<'_>, update: &PreferenceUpdate) -> ApiResult<()> {
        // The unique index treats a NULL store_id as the all-stores scope via COALESCE,
        // so the ON CONFLICT target has to be written the same way.
        sqlx::query!(
            r#"
            INSERT INTO coupon.notification_preferences
                (user_id, store_id, purpose, channel, enabled, source_consent_event_id)
            VALUES ($1, $2, $3, $4::text::coupon.notification_channel, $5, $6)
            ON CONFLICT (user_id, COALESCE(store_id, '00000000-0000-0000-0000-000000000000'::uuid), purpose, channel)
            DO UPDATE SET
                enabled = EXCLUDED.enabled,
                source_consent_event_id = EXCLUDED.source_consent_event_id
            "#,
            update.user_id,
            update.store_id,
            update.purpose,
            update.channel.as_db(),
            update.enabled,
            update.source_consent_event_id,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// Whether a purpose may currently be sent on a channel.
    ///
    /// Absent means "never decided", which is a no for anything but transactional
    /// in-app messages (§15.3).
    pub async fn is_enabled(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        store_id: Option<Uuid>,
        purpose: &str,
        channel: NotificationChannel,
    ) -> ApiResult<bool> {
        let enabled = sqlx::query_scalar!(
            r#"
            SELECT enabled
            FROM coupon.notification_preferences
            WHERE user_id = $1
              AND COALESCE(store_id, '00000000-0000-0000-0000-000000000000'::uuid)
                = COALESCE($2::uuid, '00000000-0000-0000-0000-000000000000'::uuid)
              AND purpose = $3
              AND channel = $4::text::coupon.notification_channel
            "#,
            user_id,
            store_id,
            purpose,
            channel.as_db(),
        )
        .fetch_optional(pool)
        .await?;

        Ok(enabled.unwrap_or(channel == NotificationChannel::InApp))
    }
}

/// Creates notifications, decides who may be told where, and reads the inbox.
///
/// Sending itself lives in [`delivery`], which the worker drives.
pub struct NotificationService {
    templates: TemplateRepository,
    jobs: Arc<JobService>,
    sealer: Arc<Sealer>,
    lookup_hash: Arc<LookupHash>,
    /// Fallback zone for the quiet-hours judgement when a notification names no store.
    default_timezone: Tz,
}

impl NotificationService {
    pub fn new(jobs: Arc<JobService>, sealer: Arc<Sealer>, lookup_hash: Arc<LookupHash>) -> Self {
        Self {
            templates: TemplateRepository::new(),
            jobs,
            sealer,
            lookup_hash,
            default_timezone: chrono_tz::Asia::Seoul,
        }
    }

    pub fn templates(&self) -> &TemplateRepository {
        &self.templates
    }

    // -----------------------------------------------------------------------
    // Creating
    // -----------------------------------------------------------------------

    /// Record the in-app notification and queue whatever external channels are permitted.
    ///
    /// One transaction, and every write inside it is idempotent: re-running this for the
    /// same `(user, event, type)` finds the existing notification and its deliveries rather
    /// than making a second set. That is what lets the relay be at-least-once (§14.2)
    /// without NOTIFY-004's 중복 방지 depending on the relay being careful.
    pub async fn publish(
        &self,
        pool: &PgPool,
        request: &NotificationRequest,
    ) -> ApiResult<PublishOutcome> {
        let mut tx = pool.begin().await?;
        let outcome = self.publish_in(&mut tx, request).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    /// The same, inside a transaction the caller owns.
    pub async fn publish_in(
        &self,
        tx: &mut Tx<'_>,
        request: &NotificationRequest,
    ) -> ApiResult<PublishOutcome> {
        let code = request.event.code();
        let purpose = request.event.purpose();

        // The in-app template is what gives the record its words. Without one there is
        // nothing to show, which is a deployment error rather than a per-send decision.
        let in_app = self
            .templates
            .active(
                &mut **tx,
                code,
                NotificationChannel::InApp,
                templates::DEFAULT_LOCALE,
            )
            .await?
            .ok_or_else(|| {
                ApiError::with_message(
                    ErrorCode::ServiceUnavailable,
                    "알림 템플릿을 찾을 수 없습니다.",
                )
                .internal(format!("no active IN_APP template for {code}"))
            })?;

        let rendered = templates::render(&in_app, &request.variables);

        // `notifications` requires a non-blank title and body, and a template made entirely
        // of placeholders renders to nothing when the event carried none of them — an older
        // event replayed after a template change, say. Failing the whole relay over that
        // would stop *every* notification behind it in the outbox, so the render degrades
        // to something true and readable instead (§15.1: the record is the point).
        let title = non_blank(&rendered.subject, "알림");
        let body = non_blank(&rendered.body, &title);

        // `uq_notifications_user_event_type` is the first half of NOTIFY-004: the same
        // event cannot produce two notifications for one recipient, however many relays
        // race to create it.
        let inserted = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.notifications
                (user_id, store_id, event_id, notification_type, purpose, title, body,
                 deep_link, data, occurred_at, expires_at, correlation_id, priority,
                 source_event_type)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (user_id, event_id, notification_type) DO NOTHING
            RETURNING id
            "#,
            request.user_id,
            request.store_id,
            request.event_id,
            code,
            purpose.as_db(),
            truncate(&title, 200),
            body,
            request.deep_link,
            request.data,
            request.occurred_at,
            request.expires_at,
            request.correlation_id,
            request.event.priority(),
            request.source_event_type,
        )
        .fetch_optional(&mut **tx)
        .await?;

        let (notification_id, deduplicated) = match inserted {
            Some(id) => (id, false),
            None => (
                sqlx::query_scalar!(
                    r#"
                    SELECT id FROM coupon.notifications
                    WHERE user_id = $1 AND event_id = $2 AND notification_type = $3
                    "#,
                    request.user_id,
                    request.event_id,
                    code,
                )
                .fetch_one(&mut **tx)
                .await?,
                true,
            ),
        };

        // The in-app delivery is recorded as delivered by virtue of existing: there is no
        // provider between us and the record, so there is nothing to be pending on.
        self.record_in_app_delivery(tx, notification_id, request, &in_app, &rendered)
            .await?;

        let consent = self
            .consent_snapshot(&mut **tx, request.user_id, request.store_id)
            .await?;
        let timezone = self.timezone_for(request);

        let mut queued_delivery_ids = Vec::new();
        let mut suppressed = Vec::new();

        for channel in request.event.external_channels().iter().copied() {
            let Some(template) = self
                .templates
                .active(&mut **tx, code, channel, templates::DEFAULT_LOCALE)
                .await?
            else {
                suppressed.push((channel, SuppressionReason::TemplateUnavailable));
                self.record_suppressed(
                    tx,
                    notification_id,
                    request,
                    channel,
                    None,
                    purpose,
                    SuppressionReason::TemplateUnavailable,
                )
                .await?;
                continue;
            };

            if !template.is_sendable() {
                suppressed.push((channel, SuppressionReason::TemplateUnavailable));
                self.record_suppressed(
                    tx,
                    notification_id,
                    request,
                    channel,
                    Some(&template),
                    purpose,
                    SuppressionReason::TemplateUnavailable,
                )
                .await?;
                continue;
            }

            // First of the two consent evaluations (§15.3). The second one, the one that
            // actually authorises the send, happens in the worker.
            if let Eligibility::Suppressed(reason) = policy::evaluate(channel, purpose, &consent) {
                suppressed.push((channel, reason));
                self.record_suppressed(
                    tx,
                    notification_id,
                    request,
                    channel,
                    Some(&template),
                    purpose,
                    reason,
                )
                .await?;
                continue;
            }

            let scheduled_at = policy::earliest_send_time(request.occurred_at, timezone, purpose);

            // NOTIFY-004: 만료 시각 전에 전달할 수 없는 지연 알림은 취소한다.
            if let Some(expires_at) = request.expires_at
                && scheduled_at >= expires_at
            {
                suppressed.push((channel, SuppressionReason::ExpiredBeforeDelivery));
                self.record_suppressed(
                    tx,
                    notification_id,
                    request,
                    channel,
                    Some(&template),
                    purpose,
                    SuppressionReason::ExpiredBeforeDelivery,
                )
                .await?;
                continue;
            }

            let rendered = templates::render(&template, &request.variables);
            let dedupe_key = dedupe_key(
                request.event_id,
                channel,
                &template.code,
                template.version_no,
                request.user_id,
            );

            let delivery_id = sqlx::query_scalar!(
                r#"
                INSERT INTO coupon.notification_deliveries
                    (notification_id, channel, template_id, status, correlation_id, purpose,
                     template_code, template_version_no, rendered_variables, scheduled_at,
                     deliver_before, max_attempts, dedupe_key, next_attempt_at)
                VALUES ($1, $2::text::coupon.notification_channel, $3, 'PENDING', $4, $5, $6,
                        $7, $8, $9, $10, $11, $12, $9)
                ON CONFLICT (dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
                RETURNING id
                "#,
                notification_id,
                channel.as_db(),
                template.id,
                request.correlation_id,
                purpose.as_db(),
                template.code,
                template.version_no,
                serde_json::to_value(&rendered.variables).unwrap_or_default(),
                scheduled_at,
                request.expires_at,
                delivery::MAX_DELIVERY_ATTEMPTS,
                dedupe_key,
            )
            .fetch_optional(&mut **tx)
            .await?;

            let Some(delivery_id) = delivery_id else {
                // Somebody already created this exact send. NOTIFY-004 asks for one, and
                // one is what exists.
                continue;
            };

            // §14.3's own example: `notify_event:user-uuid:event-uuid:fcm-template-2`.
            let spec = JobSpec::new(
                JobKey::notify_event(
                    request.user_id,
                    request.event_id,
                    channel,
                    template.version_no,
                ),
                serde_json::json!({
                    "delivery_id": delivery_id,
                    "notification_id": notification_id,
                    "correlation_id": request.correlation_id,
                }),
            )
            .resource(delivery_id)
            .at(scheduled_at);
            let spec = match request.store_id {
                Some(store_id) => spec.store(store_id),
                None => spec,
            };

            self.jobs.enqueue(tx, &spec).await?;
            queued_delivery_ids.push(delivery_id);
        }

        Ok(PublishOutcome {
            notification_id,
            deduplicated,
            queued_delivery_ids,
            suppressed,
        })
    }

    async fn record_in_app_delivery(
        &self,
        tx: &mut Tx<'_>,
        notification_id: Uuid,
        request: &NotificationRequest,
        template: &templates::NotificationTemplate,
        rendered: &RenderedMessage,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO coupon.notification_deliveries
                (notification_id, channel, template_id, status, correlation_id, purpose,
                 template_code, template_version_no, provider, rendered_variables,
                 scheduled_at, delivered_at, sent_at, attempt_count, dedupe_key)
            VALUES ($1, 'IN_APP', $2, 'DELIVERED', $3, $4, $5, $6, 'in-app', $7, $8, $8, $8,
                    1, $9)
            ON CONFLICT (notification_id, channel) DO NOTHING
            "#,
            notification_id,
            template.id,
            request.correlation_id,
            request.event.purpose().as_db(),
            template.code,
            template.version_no,
            serde_json::to_value(&rendered.variables).unwrap_or_default(),
            request.occurred_at,
            dedupe_key(
                request.event_id,
                NotificationChannel::InApp,
                &template.code,
                template.version_no,
                request.user_id,
            ),
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_suppressed(
        &self,
        tx: &mut Tx<'_>,
        notification_id: Uuid,
        request: &NotificationRequest,
        channel: NotificationChannel,
        template: Option<&templates::NotificationTemplate>,
        purpose: NotificationPurpose,
        reason: SuppressionReason,
    ) -> ApiResult<()> {
        // A suppression is evidence, so it is a row rather than an absence: §15.3 audits
        // consent, and "no message was sent because consent was missing at 12:04" is only
        // provable if it was written down.
        sqlx::query!(
            r#"
            INSERT INTO coupon.notification_deliveries
                (notification_id, channel, template_id, status, correlation_id, purpose,
                 template_code, template_version_no, suppression_reason, scheduled_at,
                 deliver_before, dedupe_key)
            VALUES ($1, $2::text::coupon.notification_channel, $3, 'SUPPRESSED', $4, $5, $6,
                    $7, $8, $9, $10, $11)
            ON CONFLICT (notification_id, channel) DO UPDATE
            SET suppression_reason = EXCLUDED.suppression_reason
            WHERE coupon.notification_deliveries.status = 'SUPPRESSED'
            "#,
            notification_id,
            channel.as_db(),
            template.map(|template| template.id),
            request.correlation_id,
            purpose.as_db(),
            template.map(|template| template.code.clone()),
            template.map(|template| template.version_no),
            reason.as_db(),
            request.occurred_at,
            request.expires_at,
            template.map(|template| dedupe_key(
                request.event_id,
                channel,
                &template.code,
                template.version_no,
                request.user_id,
            )),
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    fn timezone_for(&self, request: &NotificationRequest) -> Tz {
        request
            .timezone
            .as_deref()
            .and_then(|name| name.parse::<Tz>().ok())
            .unwrap_or(self.default_timezone)
    }

    // -----------------------------------------------------------------------
    // Consent
    // -----------------------------------------------------------------------

    /// Read every consent fact the §15.3 judgement needs, at this instant.
    ///
    /// Called once when the delivery is created and again by the worker immediately before
    /// the provider call. The second call is the one that matters (NOTIFY-001).
    pub async fn consent_snapshot<'e, E>(
        &self,
        executor: E,
        user_id: Uuid,
        store_id: Option<Uuid>,
    ) -> ApiResult<ConsentSnapshot>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT
                COALESCE((
                    SELECT p.enabled FROM coupon.notification_preferences p
                    WHERE p.user_id = $1 AND p.store_id IS NULL
                      AND p.purpose = 'TRANSACTIONAL' AND p.channel = 'FCM_WEB_PUSH'
                ), false) AS "web_push_transactional!",
                COALESCE((
                    SELECT p.enabled FROM coupon.notification_preferences p
                    WHERE p.user_id = $1 AND p.store_id IS NULL
                      AND p.purpose = 'MARKETING' AND p.channel = 'FCM_WEB_PUSH'
                ), false) AS "marketing_all!",
                COALESCE((
                    SELECT p.enabled FROM coupon.notification_preferences p
                    WHERE p.user_id = $1 AND p.store_id = $2
                      AND p.purpose = 'MARKETING' AND p.channel = 'FCM_WEB_PUSH'
                ), false) AS "marketing_store!",
                COALESCE((
                    SELECT p.enabled FROM coupon.notification_preferences p
                    WHERE p.user_id = $1 AND p.store_id IS NULL
                      AND p.purpose = 'INFORMATIONAL' AND p.channel = 'KAKAO_ALIMTALK'
                ), false) AS "alimtalk_informational!",
                EXISTS (
                    SELECT 1 FROM coupon.push_subscriptions s
                    WHERE s.user_id = $1 AND s.status = 'ACTIVE'
                ) AS "has_active_push!",
                COALESCE((
                    SELECT u.status = 'ACTIVE' FROM coupon.users u WHERE u.id = $1
                ), false) AS "recipient_active!"
            "#,
            user_id,
            store_id,
        )
        .fetch_one(executor)
        .await?;

        Ok(ConsentSnapshot {
            web_push_transactional: row.web_push_transactional,
            marketing_all_channels: row.marketing_all,
            // A message with no store cannot fail a store-scoped consent it does not have.
            marketing_this_store: store_id.is_none() || row.marketing_store,
            alimtalk_informational: row.alimtalk_informational,
            has_active_push_subscription: row.has_active_push,
            recipient_active: row.recipient_active,
        })
    }

    // -----------------------------------------------------------------------
    // Reading (§11.3)
    // -----------------------------------------------------------------------

    pub async fn list(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        page: &PageQuery,
        unread_only: bool,
    ) -> ApiResult<Page<Notification>> {
        let cursor = page.cursor()?;
        let rows = sqlx::query!(
            r#"
            SELECT id, store_id, notification_type, purpose, priority, title, body,
                   deep_link, data, occurred_at, read_at, expires_at
            FROM coupon.notifications
            WHERE user_id = $1
              AND dismissed_at IS NULL
              AND ($2::bool IS NOT TRUE OR read_at IS NULL)
              AND ($3::timestamptz IS NULL
                   OR (occurred_at, id) < ($3::timestamptz, $4::uuid))
            ORDER BY occurred_at DESC, id DESC
            LIMIT $5
            "#,
            user_id,
            unread_only,
            cursor.as_ref().map(|cursor| cursor.created_at),
            cursor.as_ref().map(|cursor| cursor.id),
            page.fetch_limit(),
        )
        .fetch_all(pool)
        .await?;

        let items: Vec<Notification> = rows
            .into_iter()
            .map(|row| Notification {
                id: row.id,
                store_id: row.store_id,
                notification_type: row.notification_type,
                purpose: NotificationPurpose::from_db(&row.purpose)
                    .unwrap_or(NotificationPurpose::Informational),
                priority: row.priority,
                title: row.title,
                body: row.body,
                deep_link: row.deep_link,
                data: row.data,
                occurred_at: row.occurred_at,
                read_at: row.read_at,
                expires_at: row.expires_at,
            })
            .collect();

        Ok(Page::from_rows(items, page.limit(), |row| {
            Cursor::new(row.occurred_at, row.id)
        }))
    }

    pub async fn unread_count(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<i64> {
        Ok(sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM coupon.notifications
            WHERE user_id = $1 AND read_at IS NULL AND dismissed_at IS NULL
            "#,
            user_id,
        )
        .fetch_one(pool)
        .await?)
    }

    /// Mark, unmark or hide. Scoped to the caller's own rows, so an id from someone else's
    /// inbox simply matches nothing (SEC-001).
    pub async fn update(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        request: &UpdateNotificationsRequest,
    ) -> ApiResult<NotificationUpdateResult> {
        if request.notification_ids.is_empty() && !request.all {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "처리할 알림을 지정해 주세요.",
            ));
        }

        let ids = request.notification_ids.clone();
        let now = Utc::now();

        let updated = match request.action {
            NotificationAction::MarkRead => {
                sqlx::query!(
                    r#"
                    UPDATE coupon.notifications
                    SET read_at = $3
                    WHERE user_id = $1
                      AND read_at IS NULL
                      AND ($2::bool OR id = ANY($4::uuid[]))
                    "#,
                    user_id,
                    request.all,
                    now,
                    &ids,
                )
                .execute(pool)
                .await?
            }
            NotificationAction::MarkUnread => {
                sqlx::query!(
                    r#"
                    UPDATE coupon.notifications
                    SET read_at = NULL
                    WHERE user_id = $1 AND ($2::bool OR id = ANY($3::uuid[]))
                    "#,
                    user_id,
                    request.all,
                    &ids,
                )
                .execute(pool)
                .await?
            }
            NotificationAction::Dismiss => {
                sqlx::query!(
                    r#"
                    UPDATE coupon.notifications
                    SET dismissed_at = $3, read_at = COALESCE(read_at, $3)
                    WHERE user_id = $1
                      AND dismissed_at IS NULL
                      AND ($2::bool OR id = ANY($4::uuid[]))
                    "#,
                    user_id,
                    request.all,
                    now,
                    &ids,
                )
                .execute(pool)
                .await?
            }
        };

        Ok(NotificationUpdateResult {
            updated: updated.rows_affected() as i64,
            unread_count: self.unread_count(pool, user_id).await?,
        })
    }

    // -----------------------------------------------------------------------
    // Push subscriptions (§15.1-2)
    // -----------------------------------------------------------------------

    /// Register, or revive, this browser's subscription.
    ///
    /// Re-registering the same token is the ordinary case — the service worker refreshes
    /// it — so the keyed lookup hash makes it an upsert rather than a duplicate. Revival
    /// also clears a previous `disabled_reason`: a token that FCM rejected yesterday and
    /// the browser handed us again today is a working token (NOTIFY-003).
    pub async fn register_push_subscription(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        request: &RegisterPushSubscriptionRequest,
    ) -> ApiResult<PushSubscription> {
        let ciphertext = self.sealer.seal(&request.token);
        let token_hash = self.lookup_hash.hash("push-token", &request.token);

        let row = sqlx::query!(
            r#"
            INSERT INTO coupon.push_subscriptions
                (user_id, token_ciphertext, token_lookup_hash, browser_family, device_label,
                 status)
            VALUES ($1, $2, $3, $4, $5, 'ACTIVE')
            ON CONFLICT (token_lookup_hash) DO UPDATE
            SET user_id = EXCLUDED.user_id,
                token_ciphertext = EXCLUDED.token_ciphertext,
                browser_family = COALESCE(EXCLUDED.browser_family, coupon.push_subscriptions.browser_family),
                device_label = COALESCE(EXCLUDED.device_label, coupon.push_subscriptions.device_label),
                status = 'ACTIVE',
                disabled_at = NULL,
                disabled_reason = NULL,
                failure_count = 0,
                last_seen_at = clock_timestamp()
            RETURNING id, browser_family, device_label, status::text AS "status!",
                      disabled_reason, last_seen_at, created_at
            "#,
            user_id,
            ciphertext,
            token_hash,
            request.browser_family,
            request.device_label,
        )
        .fetch_one(pool)
        .await?;

        Ok(PushSubscription {
            id: row.id,
            browser_family: row.browser_family,
            device_label: row.device_label,
            status: row.status,
            disabled_reason: row.disabled_reason,
            last_seen_at: row.last_seen_at,
            created_at: row.created_at,
        })
    }

    pub async fn list_push_subscriptions(
        &self,
        pool: &PgPool,
        user_id: Uuid,
    ) -> ApiResult<Vec<PushSubscription>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, browser_family, device_label, status::text AS "status!",
                   disabled_reason, last_seen_at, created_at
            FROM coupon.push_subscriptions
            WHERE user_id = $1
            ORDER BY created_at DESC
            "#,
            user_id,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| PushSubscription {
                id: row.id,
                browser_family: row.browser_family,
                device_label: row.device_label,
                status: row.status,
                disabled_reason: row.disabled_reason,
                last_seen_at: row.last_seen_at,
                created_at: row.created_at,
            })
            .collect())
    }

    /// `DELETE /me/push-subscriptions/:id`.
    ///
    /// The row stays and is marked revoked rather than deleted: NOTIFY-001 wants the
    /// history of channel decisions, and a deleted row cannot explain why a message was
    /// suppressed last Tuesday.
    pub async fn revoke_push_subscription(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        subscription_id: Uuid,
    ) -> ApiResult<()> {
        let result = sqlx::query!(
            r#"
            UPDATE coupon.push_subscriptions
            SET status = 'ARCHIVED', disabled_at = clock_timestamp(),
                disabled_reason = 'USER_REVOKED'
            WHERE id = $1 AND user_id = $2 AND status <> 'ARCHIVED'
            "#,
            subscription_id,
            user_id,
        )
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::new(ErrorCode::NotificationNotFound));
        }

        Ok(())
    }
}

/// NOTIFY-004's 발송 고유키: `event_id + channel + template_version + recipient`.
pub fn dedupe_key(
    event_id: Uuid,
    channel: NotificationChannel,
    template_code: &str,
    template_version: i32,
    recipient_user_id: Uuid,
) -> String {
    format!(
        "{event_id}|{}|{template_code}v{template_version}|{recipient_user_id}",
        channel.as_db()
    )
}

/// `value` if it has any non-whitespace content, otherwise `fallback`.
fn non_blank(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

impl JobKey {
    /// `notify_event:user-uuid:event-uuid:fcm-template-2` (§14.3).
    ///
    /// §14.6 gives notification sending the concurrency key `event+channel+recipient`; the
    /// template version rides along because a re-send under a new template version is a
    /// different logical job, not a retry of the old one.
    pub fn notify_event(
        user_id: Uuid,
        event_id: Uuid,
        channel: NotificationChannel,
        template_version: i32,
    ) -> Self {
        JobKey::new(
            JobType::NotifyEvent,
            user_id.to_string(),
            event_id.to_string(),
            format!("{}-template-{template_version}", channel.key_token()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_use_the_database_spelling() {
        assert_eq!(NotificationChannel::InApp.as_db(), "IN_APP");
        assert_eq!(NotificationChannel::FcmWebPush.as_db(), "FCM_WEB_PUSH");
        assert_eq!(NotificationChannel::KakaoAlimtalk.as_db(), "KAKAO_ALIMTALK");

        for channel in [
            NotificationChannel::InApp,
            NotificationChannel::FcmWebPush,
            NotificationChannel::KakaoAlimtalk,
        ] {
            assert_eq!(NotificationChannel::from_db(channel.as_db()), Some(channel));
        }
    }

    #[test]
    fn every_domain_event_the_outbox_carries_maps_onto_a_notification() {
        // These are the `event_type` values the Phase 2 and Phase 3 writers actually
        // insert. A rename on either side must break here rather than silently stop
        // notifying anyone.
        for (raw, expected) in [
            ("STAMP_EARNED", NotificationEvent::StampEarned),
            ("REWARD_ISSUED", NotificationEvent::RewardIssued),
            ("CAMPAIGN_COUPON_ISSUED", NotificationEvent::CouponIssued),
            ("COUPON_REDEEMED", NotificationEvent::CouponUsed),
            ("TRANSACTION_VOIDED", NotificationEvent::TransactionVoided),
            ("REDEMPTION_VOIDED", NotificationEvent::TransactionVoided),
            ("COUPON_EXPIRING", NotificationEvent::CouponExpiring),
        ] {
            assert_eq!(NotificationEvent::from_outbox_event(raw), Some(expected), "{raw}");
        }

        assert_eq!(NotificationEvent::from_outbox_event("JOB_ENQUEUED"), None);
    }

    #[test]
    fn the_purpose_table_follows_15_3() {
        assert_eq!(
            NotificationEvent::StampEarned.purpose(),
            NotificationPurpose::Transactional
        );
        assert_eq!(
            NotificationEvent::CouponIssued.purpose(),
            NotificationPurpose::Marketing,
            "신규 할인 캠페인은 마케팅이다"
        );
        assert_eq!(
            NotificationEvent::SecurityAlert.purpose(),
            NotificationPurpose::Security
        );
        assert_eq!(
            NotificationEvent::CouponExpiring.purpose(),
            NotificationPurpose::Informational
        );
    }

    #[test]
    fn the_dedupe_key_separates_every_dimension_notify_004_names() {
        let event = Uuid::from_u128(1);
        let user = Uuid::from_u128(2);
        let base = dedupe_key(event, NotificationChannel::FcmWebPush, "STAMP_EARNED", 1, user);

        assert_ne!(
            base,
            dedupe_key(event, NotificationChannel::KakaoAlimtalk, "STAMP_EARNED", 1, user),
            "channel"
        );
        assert_ne!(
            base,
            dedupe_key(event, NotificationChannel::FcmWebPush, "STAMP_EARNED", 2, user),
            "template version"
        );
        assert_ne!(
            base,
            dedupe_key(
                Uuid::from_u128(3),
                NotificationChannel::FcmWebPush,
                "STAMP_EARNED",
                1,
                user
            ),
            "event"
        );
        assert_ne!(
            base,
            dedupe_key(
                event,
                NotificationChannel::FcmWebPush,
                "STAMP_EARNED",
                1,
                Uuid::from_u128(4)
            ),
            "recipient"
        );
    }

    #[test]
    fn the_job_key_matches_the_shape_14_3_documents() {
        let key = JobKey::notify_event(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            NotificationChannel::FcmWebPush,
            2,
        );
        assert!(
            key.to_string().ends_with(":fcm-template-2"),
            "{key}",
        );
        assert!(key.to_string().starts_with("notify_event:"), "{key}");
    }

    #[test]
    fn delivery_statuses_round_trip_and_only_the_settled_ones_are_terminal() {
        for status in [
            DeliveryStatus::Pending,
            DeliveryStatus::Sending,
            DeliveryStatus::Delivered,
            DeliveryStatus::FailedRetryable,
            DeliveryStatus::FailedPermanent,
            DeliveryStatus::Suppressed,
        ] {
            assert_eq!(DeliveryStatus::from_db(status.as_db()), status);
        }

        assert!(DeliveryStatus::Delivered.is_terminal());
        assert!(DeliveryStatus::FailedPermanent.is_terminal());
        assert!(DeliveryStatus::Suppressed.is_terminal());
        assert!(
            !DeliveryStatus::FailedRetryable.is_terminal(),
            "a retryable failure is a waiting state, not an outcome"
        );
    }

    #[test]
    fn a_render_that_produces_nothing_still_yields_a_storable_record() {
        // `coupon.notifications` refuses a blank title or body, and a template that is only
        // placeholders renders blank for an event that carried none of them. The record has
        // to exist anyway (§15.1), so the fallback chain is title → subject → a fixed line.
        assert_eq!(non_blank("  ", "알림"), "알림");
        assert_eq!(non_blank("적립 알림", "알림"), "적립 알림");
        assert_eq!(non_blank("", ""), "");
    }

    #[test]
    fn a_store_status_change_stays_in_the_app() {
        assert!(NotificationEvent::StoreSuspended.external_channels().is_empty());
        assert!(!NotificationEvent::CouponUsed.external_channels().is_empty());
    }
}
