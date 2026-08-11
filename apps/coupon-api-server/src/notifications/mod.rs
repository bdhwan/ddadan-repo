//! In-app notifications, FCM, Alimtalk, and consent-based send eligibility
//! (§10.2 `notifications`, §15).
//!
//! Phase 4 owns the delivery pipeline. What exists now is the piece Phase 1 genuinely
//! needs: `coupon.notification_preferences` is this module's table, so `consents` asks
//! *it* to project a consent decision rather than writing the rows itself (§10.2).
//!
//! TODO(phase-4): notification creation, FCM Web Push and Alimtalk delivery, template
//! versioning, delivery-result reconciliation, and the §15.3 purpose/consent judgement
//! that decides whether a given event may leave the platform.

use sqlx::PgPool;
use uuid::Uuid;

use crate::db::Tx;
use crate::error::ApiResult;

/// Delivery channel. Mirrors `coupon.notification_channel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_use_the_database_spelling() {
        assert_eq!(NotificationChannel::InApp.as_db(), "IN_APP");
        assert_eq!(NotificationChannel::FcmWebPush.as_db(), "FCM_WEB_PUSH");
        assert_eq!(NotificationChannel::KakaoAlimtalk.as_db(), "KAKAO_ALIMTALK");
    }
}
