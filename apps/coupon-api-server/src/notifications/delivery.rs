//! 발송 실행과 전달 결과 (§15.4, NOTIFY-001, NOTIFY-003, NOTIFY-004, §19.3).
//!
//! This is the half of the pipeline that runs in the worker, and it exists as its own
//! module because of what it must *not* touch. NOTIFY-003 says an external send failure
//! cannot roll back the coupon, the accrual or the use — so nothing here writes to
//! `coupon_instances`, `stamp_ledger` or `redemption_transactions`, and the only tables it
//! updates are `notification_deliveries`, `notification_delivery_callbacks` and
//! `push_subscriptions`. That is a structural guarantee rather than a discipline: there is
//! no code path from a provider error to a wallet row.
//!
//! The other rule with teeth is NOTIFY-001. Consent is evaluated when the delivery row is
//! created *and again here*, from a snapshot read microseconds before the provider call.
//! A withdrawal that lands while the job sat in the queue therefore still stops the send.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::jobs::{RetryBudget, RetryClass, RetryDecision};
use crate::notifications::policy::{self, Eligibility, NotificationPurpose, SuppressionReason};
use crate::notifications::providers::{ProviderMessage, ProviderOutcome};
use crate::notifications::{DeliveryStatus, NotificationChannel, templates};
use crate::state::AppState;

/// §14.6 gives 알림 발송 a retry budget of 5.
pub const MAX_DELIVERY_ATTEMPTS: i32 = 5;

/// One delivery, joined with everything the send needs.
#[derive(Debug, Clone)]
pub struct DeliveryRecord {
    pub id: Uuid,
    pub notification_id: Uuid,
    pub user_id: Uuid,
    pub store_id: Option<Uuid>,
    pub channel: NotificationChannel,
    pub status: DeliveryStatus,
    pub purpose: NotificationPurpose,
    pub template_id: Option<Uuid>,
    pub attempt_count: i32,
    pub max_attempts: i32,
    /// Set once a provider has acknowledged the send. Its presence is what tells a re-run
    /// that the message is already out (NOTIFY-004).
    pub provider_reference: Option<String>,
    pub scheduled_at: DateTime<Utc>,
    pub deliver_before: Option<DateTime<Utc>>,
    pub correlation_id: Option<Uuid>,
    pub variables: BTreeMap<String, String>,
    pub store_timezone: Option<String>,
}

/// What one dispatch attempt did, for the job runner and the logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// The provider accepted or delivered it.
    Sent { status: DeliveryStatus },
    /// Not sent, and not going to be (§15.4 `SUPPRESSED`).
    Suppressed { reason: SuppressionReason },
    /// Failed, and another attempt is scheduled.
    Retrying { after: Duration, code: String },
    /// Failed for good (§14.6: 수신 거부·템플릿 거절).
    Failed { code: String },
    /// The delivery had already settled — a duplicate job, or a callback that got there
    /// first. Not an error (§14.5-4: the worker re-reads state and acts on what it finds).
    AlreadySettled { status: DeliveryStatus },
}

/// A provider's report about a send it already accepted (§15.4).
#[derive(Debug, Clone)]
pub struct ProviderCallback {
    pub channel: NotificationChannel,
    pub provider: String,
    /// The provider's identifier *for this callback*. Two deliveries of the same message
    /// share a `provider_reference` but not this, which is what makes replay detectable.
    pub provider_event_id: String,
    pub provider_reference: String,
    pub reported_status: String,
    pub signed_at: DateTime<Utc>,
    pub payload: serde_json::Value,
    pub signature_valid: bool,
}

/// What recording a callback did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallbackOutcome {
    /// First time seen, and the delivery moved.
    Applied,
    /// Already seen. §15.4: 같은 사건 콜백이 여러 번 와도 결과를 한 번만 확정한다.
    Duplicate,
    /// The reference names no delivery of ours, or the delivery had already settled.
    Ignored,
}

/// Send one delivery.
///
/// Returns `Ok` for every outcome including failure: the *job* succeeded in doing what it
/// was asked, and whether the message arrived is recorded on the delivery row. Only an
/// infrastructure error — the database being unreachable — is an `Err`, because that is the
/// one case where retrying the job itself is the right response.
pub async fn dispatch(state: &AppState, delivery_id: Uuid) -> ApiResult<DispatchOutcome> {
    let Some(delivery) = load(&state.pool, delivery_id).await? else {
        return Err(ApiError::new(ErrorCode::NotificationNotFound)
            .internal(format!("delivery {delivery_id} is gone")));
    };

    // §14.5-4: the worker re-reads state before acting. A settled delivery is done, and a
    // `SENDING` one that already carries a provider reference has been handed over — a
    // second send would be the duplicate NOTIFY-004 forbids. A `SENDING` row with *no*
    // reference is a worker that died mid-call, which is exactly the case worth retrying.
    if delivery.status.is_terminal()
        || (delivery.status == DeliveryStatus::Sending && delivery.provider_reference.is_some())
    {
        return Ok(DispatchOutcome::AlreadySettled {
            status: delivery.status,
        });
    }

    let now = crate::qr::database_now(&state.pool).await?;

    // NOTIFY-004: 만료 시각 전에 전달할 수 없는 지연 알림은 취소한다. Checked again here
    // because the queue may have held the job past the point where sending helps.
    if let Some(deliver_before) = delivery.deliver_before
        && now >= deliver_before
    {
        return suppress(
            &state.pool,
            &delivery,
            SuppressionReason::ExpiredBeforeDelivery,
        )
        .await;
    }

    // NOTIFY-001, and the reason this module exists: consent is re-read *now*, not trusted
    // from when the delivery row was written.
    let consent = state
        .notifications
        .consent_snapshot(&state.pool, delivery.user_id, delivery.store_id)
        .await?;

    if let Eligibility::Suppressed(reason) =
        policy::evaluate(delivery.channel, delivery.purpose, &consent)
    {
        return suppress(&state.pool, &delivery, reason).await;
    }

    // The quiet-hours window can also have closed since the job was scheduled — a retry of
    // a marketing send at 21:05 must wait rather than slip through (NOTIFY-004).
    let timezone = delivery
        .store_timezone
        .as_deref()
        .and_then(|name| name.parse::<chrono_tz::Tz>().ok())
        .unwrap_or(chrono_tz::Asia::Seoul);
    let permitted_at = policy::earliest_send_time(now, timezone, delivery.purpose);
    if permitted_at > now {
        let after = (permitted_at - now).to_std().unwrap_or(Duration::from_secs(60));
        schedule_retry(&state.pool, &delivery, after, "QUIET_HOURS", "야간 발송 정책").await?;
        return Ok(DispatchOutcome::Retrying {
            after,
            code: "QUIET_HOURS".to_owned(),
        });
    }

    // §15.2: the send is reproduced from the *pinned* template version, not from whatever
    // is active today.
    let Some(template) = load_template(state, &delivery).await? else {
        return suppress(&state.pool, &delivery, SuppressionReason::TemplateUnavailable).await;
    };

    if !template.is_sendable() {
        return suppress(&state.pool, &delivery, SuppressionReason::TemplateUnavailable).await;
    }

    let rendered = templates::render(&template, &delivery.variables);

    mark_sending(&state.pool, &delivery, now).await?;

    let (provider, outcome) = match delivery.channel {
        NotificationChannel::InApp => (
            "in-app",
            ProviderOutcome::Delivered {
                provider_reference: format!("in-app:{}", delivery.id),
            },
        ),
        NotificationChannel::FcmWebPush => (
            state.web_push_provider.name(),
            send_web_push(state, &delivery, &template, &rendered).await?,
        ),
        NotificationChannel::KakaoAlimtalk => (
            state.alimtalk_provider.name(),
            send_alimtalk(state, &delivery, &template, &rendered).await?,
        ),
    };

    apply_provider_outcome(state, &delivery, provider, outcome, now).await
}

/// Turn a provider's answer into a §15.4 status.
async fn apply_provider_outcome(
    state: &AppState,
    delivery: &DeliveryRecord,
    // Which implementation answered. Recorded on the delivery so an incident review can
    // tell a 대행사 change apart from a regression (§15.1: the provider is replaceable).
    provider: &str,
    outcome: ProviderOutcome,
    now: DateTime<Utc>,
) -> ApiResult<DispatchOutcome> {
    match outcome {
        ProviderOutcome::Accepted { provider_reference } => {
            settle(
                &state.pool,
                delivery,
                provider,
                DeliveryStatus::Sending,
                Some(&provider_reference),
                "ACCEPTED",
                None,
                now,
            )
            .await?;
            Ok(DispatchOutcome::Sent {
                status: DeliveryStatus::Sending,
            })
        }

        ProviderOutcome::Delivered { provider_reference } => {
            settle(
                &state.pool,
                delivery,
                provider,
                DeliveryStatus::Delivered,
                Some(&provider_reference),
                "DELIVERED",
                None,
                now,
            )
            .await?;
            Ok(DispatchOutcome::Sent {
                status: DeliveryStatus::Delivered,
            })
        }

        ProviderOutcome::RetryableFailure {
            code,
            message,
            retry_after,
        } => {
            let class = match retry_after {
                Some(after) => RetryClass::ProviderThrottled {
                    retry_after_secs: after.as_secs(),
                },
                None => RetryClass::Transient,
            };

            match class.decide(
                delivery.attempt_count + 1,
                RetryBudget::Limited(delivery.max_attempts),
            ) {
                RetryDecision::Retry { after } => {
                    schedule_retry(&state.pool, delivery, after, &code, &message).await?;
                    Ok(DispatchOutcome::Retrying { after, code })
                }
                // §14.6: 최대 재시도 소진은 영구 실패다. The coupon is untouched.
                RetryDecision::DeadLetter => {
                    settle(
                        &state.pool,
                        delivery,
                        provider,
                        DeliveryStatus::FailedPermanent,
                        None,
                        "RETRIES_EXHAUSTED",
                        Some((code.as_str(), message.as_str())),
                        now,
                    )
                    .await?;
                    Ok(DispatchOutcome::Failed { code })
                }
            }
        }

        ProviderOutcome::PermanentFailure {
            code,
            message,
            recipient_gone: _,
        } => {
            settle(
                &state.pool,
                delivery,
                provider,
                DeliveryStatus::FailedPermanent,
                None,
                "PERMANENT",
                Some((code.as_str(), message.as_str())),
                now,
            )
            .await?;
            Ok(DispatchOutcome::Failed { code })
        }
    }
}

/// Send to every browser this user has registered.
///
/// WALLET-004 allows several devices, so "delivered" means at least one of them took it.
/// A token the provider says is gone is deactivated on the spot (NOTIFY-003) and does not
/// make the delivery fail while another device is still reachable.
async fn send_web_push(
    state: &AppState,
    delivery: &DeliveryRecord,
    template: &templates::NotificationTemplate,
    rendered: &templates::RenderedMessage,
) -> ApiResult<ProviderOutcome> {
    let subscriptions = sqlx::query!(
        r#"
        SELECT id, token_ciphertext
        FROM coupon.push_subscriptions
        WHERE user_id = $1 AND status = 'ACTIVE'
        ORDER BY last_seen_at DESC
        "#,
        delivery.user_id,
    )
    .fetch_all(&state.pool)
    .await?;

    if subscriptions.is_empty() {
        return Ok(ProviderOutcome::PermanentFailure {
            code: "NO_ACTIVE_SUBSCRIPTION".to_owned(),
            message: "활성 푸시 구독이 없습니다.".to_owned(),
            recipient_gone: true,
        });
    }

    let mut best: Option<ProviderOutcome> = None;

    for subscription in subscriptions {
        let Some(token) = state.sealer.open(&subscription.token_ciphertext) else {
            // A token we can no longer decrypt is a token we can never use again.
            disable_subscription(&state.pool, subscription.id, "UNDECRYPTABLE").await?;
            continue;
        };

        let outcome = state
            .web_push_provider
            .send(ProviderMessage {
                delivery_id: delivery.id,
                correlation_id: delivery.correlation_id.unwrap_or(delivery.id),
                recipient: token,
                subject: rendered.subject.clone(),
                body: rendered.body.clone(),
                provider_template_id: template.provider_template_id.clone(),
                variables: rendered.variables.clone(),
            })
            .await;

        if let ProviderOutcome::PermanentFailure {
            recipient_gone: true,
            ref code,
            ..
        } = outcome
        {
            disable_subscription(&state.pool, subscription.id, code).await?;
        }

        if matches!(
            outcome,
            ProviderOutcome::Accepted { .. } | ProviderOutcome::Delivered { .. }
        ) {
            touch_subscription(&state.pool, subscription.id).await?;
            return Ok(outcome);
        }

        // Keep the most actionable failure: a retryable one beats a permanent one, because
        // one dead device must not condemn a send another device could still take.
        best = Some(match (best.take(), outcome) {
            (Some(ProviderOutcome::RetryableFailure { .. }), other)
                if !matches!(other, ProviderOutcome::RetryableFailure { .. }) =>
            {
                ProviderOutcome::RetryableFailure {
                    code: "PROVIDER_RETRY".to_owned(),
                    message: String::new(),
                    retry_after: None,
                }
            }
            (_, other) => other,
        });
    }

    Ok(best.unwrap_or(ProviderOutcome::PermanentFailure {
        code: "NO_ACTIVE_SUBSCRIPTION".to_owned(),
        message: "발송 가능한 구독이 없습니다.".to_owned(),
        recipient_gone: true,
    }))
}

/// Send one 알림톡.
///
/// The recipient comes from the linked Kakao identity's profile snapshot. The MVP does not
/// collect a consumer phone number of its own (§17.1 makes it optional), so when the
/// snapshot carries none there is simply nobody to address — a suppression, not a failure.
async fn send_alimtalk(
    state: &AppState,
    delivery: &DeliveryRecord,
    template: &templates::NotificationTemplate,
    rendered: &templates::RenderedMessage,
) -> ApiResult<ProviderOutcome> {
    let recipient = sqlx::query_scalar!(
        r#"
        SELECT provider_profile_snapshot ->> 'phone_number'
        FROM coupon.auth_identities
        WHERE user_id = $1 AND provider = 'KAKAO' AND status = 'ACTIVE'
        ORDER BY linked_at DESC
        LIMIT 1
        "#,
        delivery.user_id,
    )
    .fetch_optional(&state.pool)
    .await?
    .flatten();

    let Some(recipient) = recipient else {
        return Ok(ProviderOutcome::PermanentFailure {
            code: "RECIPIENT_UNREACHABLE".to_owned(),
            message: "알림톡을 보낼 연락처가 없습니다.".to_owned(),
            recipient_gone: true,
        });
    };

    Ok(state
        .alimtalk_provider
        .send(ProviderMessage {
            delivery_id: delivery.id,
            correlation_id: delivery.correlation_id.unwrap_or(delivery.id),
            recipient,
            subject: rendered.subject.clone(),
            body: rendered.body.clone(),
            provider_template_id: template.provider_template_id.clone(),
            variables: rendered.variables.clone(),
        })
        .await)
}

// ---------------------------------------------------------------------------
// Callbacks (§15.4)
// ---------------------------------------------------------------------------

/// Record a provider callback and, if it is new and valid, move the delivery.
///
/// Three things happen here and all three are §15.4:
///
/// 1. the signature verdict is stored whether it passed or failed — a forged callback is
///    evidence, and dropping it silently would hide an attack;
/// 2. the provider's event id is a unique key, so a replay is *recognised* rather than
///    re-applied (`Duplicate`);
/// 3. the `provider_reference` must match a delivery on the same channel, so a valid
///    signature over somebody else's reference still changes nothing.
pub async fn record_callback(
    pool: &PgPool,
    callback: &ProviderCallback,
) -> ApiResult<CallbackOutcome> {
    let mut tx = pool.begin().await?;

    // Resolve the delivery first so the stored callback points at it even when the
    // signature failed — an investigation wants to know what was targeted.
    let delivery = sqlx::query!(
        r#"
        SELECT id, status::text AS "status!", attempt_count
        FROM coupon.notification_deliveries
        WHERE channel = $1::text::coupon.notification_channel
          AND provider_reference = $2
        "#,
        callback.channel.as_db(),
        callback.provider_reference,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let inserted = sqlx::query_scalar!(
        r#"
        INSERT INTO coupon.notification_delivery_callbacks
            (delivery_id, channel, provider, provider_event_id, provider_reference,
             reported_status, signature_valid, signed_at, payload, applied)
        VALUES ($1, $2::text::coupon.notification_channel, $3, $4, $5, $6, $7, $8, $9, false)
        ON CONFLICT (channel, provider, provider_event_id) DO NOTHING
        RETURNING id
        "#,
        delivery.as_ref().map(|row| row.id),
        callback.channel.as_db(),
        callback.provider,
        callback.provider_event_id,
        callback.provider_reference,
        callback.reported_status,
        callback.signature_valid,
        callback.signed_at,
        callback.payload,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(callback_id) = inserted else {
        tx.commit().await?;
        return Ok(CallbackOutcome::Duplicate);
    };

    if !callback.signature_valid {
        tx.commit().await?;
        tracing::warn!(
            provider = callback.provider,
            provider_reference = callback.provider_reference,
            "notifications.callback_signature_invalid"
        );
        return Ok(CallbackOutcome::Ignored);
    }

    let Some(delivery) = delivery else {
        tx.commit().await?;
        return Ok(CallbackOutcome::Ignored);
    };

    let current = DeliveryStatus::from_db(&delivery.status);
    // A callback never *un*-settles anything. §15.4 asks for the result to be confirmed
    // once; a late "failed" after a "delivered" is the provider catching up, not news.
    if current.is_terminal() {
        tx.commit().await?;
        return Ok(CallbackOutcome::Ignored);
    }

    let next = match callback.reported_status.to_ascii_uppercase().as_str() {
        "DELIVERED" | "SUCCESS" | "COMPLETED" => DeliveryStatus::Delivered,
        "FAILED" | "REJECTED" | "BOUNCED" => DeliveryStatus::FailedPermanent,
        _ => DeliveryStatus::Sending,
    };

    sqlx::query!(
        r#"
        UPDATE coupon.notification_deliveries
        SET status = $2::text::coupon.notification_delivery_status,
            provider_status = $3,
            delivered_at = CASE WHEN $2 = 'DELIVERED' THEN clock_timestamp() ELSE delivered_at END,
            version = version + 1
        WHERE id = $1
        "#,
        delivery.id,
        next.as_db(),
        callback.reported_status,
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"UPDATE coupon.notification_delivery_callbacks SET applied = true WHERE id = $1"#,
        callback_id,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(CallbackOutcome::Applied)
}

// ---------------------------------------------------------------------------
// Row-level helpers
// ---------------------------------------------------------------------------

pub async fn load(pool: &PgPool, delivery_id: Uuid) -> ApiResult<Option<DeliveryRecord>> {
    let row = sqlx::query!(
        r#"
        SELECT d.id, d.notification_id, n.user_id, n.store_id,
               d.channel::text AS "channel!", d.status::text AS "status!", d.purpose,
               d.template_id, d.attempt_count, d.max_attempts, d.provider_reference,
               d.scheduled_at, d.deliver_before, d.correlation_id, d.rendered_variables,
               s.timezone AS "timezone?"
        FROM coupon.notification_deliveries d
        JOIN coupon.notifications n ON n.id = d.notification_id
        LEFT JOIN coupon.stores s ON s.id = n.store_id
        WHERE d.id = $1
        "#,
        delivery_id,
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| DeliveryRecord {
        id: row.id,
        notification_id: row.notification_id,
        user_id: row.user_id,
        store_id: row.store_id,
        channel: NotificationChannel::from_db(&row.channel).unwrap_or(NotificationChannel::InApp),
        status: DeliveryStatus::from_db(&row.status),
        purpose: NotificationPurpose::from_db(&row.purpose)
            .unwrap_or(NotificationPurpose::Informational),
        template_id: row.template_id,
        attempt_count: row.attempt_count,
        max_attempts: row.max_attempts,
        provider_reference: row.provider_reference,
        scheduled_at: row.scheduled_at,
        deliver_before: row.deliver_before,
        correlation_id: row.correlation_id,
        variables: read_variables(&row.rendered_variables),
        store_timezone: row.timezone,
    }))
}

async fn load_template(
    state: &AppState,
    delivery: &DeliveryRecord,
) -> ApiResult<Option<templates::NotificationTemplate>> {
    match delivery.template_id {
        Some(id) => state.notifications.templates().by_id(&state.pool, id).await,
        None => Ok(None),
    }
}

async fn suppress(
    pool: &PgPool,
    delivery: &DeliveryRecord,
    reason: SuppressionReason,
) -> ApiResult<DispatchOutcome> {
    sqlx::query!(
        r#"
        UPDATE coupon.notification_deliveries
        SET status = 'SUPPRESSED', suppression_reason = $2, next_attempt_at = NULL,
            version = version + 1
        WHERE id = $1 AND status NOT IN ('DELIVERED', 'FAILED_PERMANENT', 'SUPPRESSED')
        "#,
        delivery.id,
        reason.as_db(),
    )
    .execute(pool)
    .await?;

    tracing::info!(
        delivery_id = %delivery.id,
        channel = delivery.channel.as_db(),
        reason = reason.as_db(),
        "notifications.suppressed"
    );

    Ok(DispatchOutcome::Suppressed { reason })
}

async fn mark_sending(
    pool: &PgPool,
    delivery: &DeliveryRecord,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    sqlx::query!(
        r#"
        UPDATE coupon.notification_deliveries
        SET status = 'SENDING', attempt_count = attempt_count + 1, sent_at = $2,
            version = version + 1
        WHERE id = $1
        "#,
        delivery.id,
        now,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn settle(
    pool: &PgPool,
    delivery: &DeliveryRecord,
    provider: &str,
    status: DeliveryStatus,
    provider_reference: Option<&str>,
    provider_status: &str,
    error: Option<(&str, &str)>,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    sqlx::query!(
        r#"
        UPDATE coupon.notification_deliveries
        SET status = $2::text::coupon.notification_delivery_status,
            provider = $3,
            provider_reference = COALESCE($4, provider_reference),
            provider_status = $5,
            last_error_code = $6,
            last_error_message = $7,
            delivered_at = CASE WHEN $2 = 'DELIVERED' THEN $8 ELSE delivered_at END,
            next_attempt_at = NULL,
            version = version + 1
        WHERE id = $1
        "#,
        delivery.id,
        status.as_db(),
        provider,
        provider_reference,
        provider_status,
        error.map(|(code, _)| code),
        error.map(|(_, message)| truncate(message, 2000)),
        now,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn schedule_retry(
    pool: &PgPool,
    delivery: &DeliveryRecord,
    after: Duration,
    code: &str,
    message: &str,
) -> ApiResult<()> {
    let next_attempt_at = Utc::now()
        + chrono::Duration::from_std(after).unwrap_or_else(|_| chrono::Duration::minutes(1));

    sqlx::query!(
        r#"
        UPDATE coupon.notification_deliveries
        SET status = 'FAILED_RETRYABLE', next_attempt_at = $2, last_error_code = $3,
            last_error_message = $4, version = version + 1
        WHERE id = $1
        "#,
        delivery.id,
        next_attempt_at,
        code,
        truncate(message, 2000),
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// NOTIFY-003: FCM 만료 토큰은 비활성화하고 사용자에게 권한 복구 안내를 표시한다. The
/// consumer app reads `disabled_reason` from `GET /me/push-subscriptions` to show it.
async fn disable_subscription(pool: &PgPool, subscription_id: Uuid, reason: &str) -> ApiResult<()> {
    sqlx::query!(
        r#"
        UPDATE coupon.push_subscriptions
        SET status = 'INACTIVE', disabled_at = clock_timestamp(), disabled_reason = $2,
            failure_count = failure_count + 1, version = version + 1
        WHERE id = $1 AND status = 'ACTIVE'
        "#,
        subscription_id,
        truncate(reason, 64),
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn touch_subscription(pool: &PgPool, subscription_id: Uuid) -> ApiResult<()> {
    sqlx::query!(
        r#"
        UPDATE coupon.push_subscriptions
        SET last_seen_at = clock_timestamp(), last_success_at = clock_timestamp(),
            failure_count = 0
        WHERE id = $1
        "#,
        subscription_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Deliveries whose retry is due, for the worker's sweep (§18.4 notification backlog).
pub async fn due_deliveries(
    pool: &PgPool,
    now: DateTime<Utc>,
    limit: i64,
) -> ApiResult<Vec<Uuid>> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT id
        FROM coupon.notification_deliveries
        WHERE status IN ('PENDING', 'FAILED_RETRYABLE')
          AND COALESCE(next_attempt_at, scheduled_at) <= $1
        ORDER BY COALESCE(next_attempt_at, scheduled_at)
        LIMIT $2
        FOR UPDATE SKIP LOCKED
        "#,
        now,
        limit,
    )
    .fetch_all(pool)
    .await?)
}

fn read_variables(value: &serde_json::Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retryable_failure_inside_the_budget_retries_and_beyond_it_gives_up() {
        // §14.6: 알림 발송 retry 5.
        let budget = RetryBudget::Limited(MAX_DELIVERY_ATTEMPTS);
        assert!(matches!(
            RetryClass::Transient.decide(1, budget),
            RetryDecision::Retry { .. }
        ));
        assert!(matches!(
            RetryClass::Transient.decide(MAX_DELIVERY_ATTEMPTS, budget),
            RetryDecision::DeadLetter
        ));
    }

    #[test]
    fn a_throttled_provider_sets_the_schedule() {
        // §14.7: provider 429/Retry-After 는 제공자 값을 우선한다.
        let decision = RetryClass::ProviderThrottled {
            retry_after_secs: 45,
        }
        .decide(1, RetryBudget::Limited(MAX_DELIVERY_ATTEMPTS));

        assert_eq!(
            decision,
            RetryDecision::Retry {
                after: Duration::from_secs(45)
            }
        );
    }

    #[test]
    fn variables_survive_a_round_trip_through_jsonb() {
        let value = serde_json::json!({ "store_name": "가게", "quantity": "2", "n": 3 });
        let read = read_variables(&value);

        assert_eq!(read.get("store_name").map(String::as_str), Some("가게"));
        assert_eq!(read.get("quantity").map(String::as_str), Some("2"));
        assert!(
            !read.contains_key("n"),
            "a non-string value is not a rendered variable"
        );
    }

    #[test]
    fn a_reported_status_maps_onto_the_15_4_vocabulary() {
        // Written as a table because the provider vocabulary is theirs, not ours, and the
        // mapping is the thing that has to be reviewable.
        for (reported, expected) in [
            ("delivered", DeliveryStatus::Delivered),
            ("SUCCESS", DeliveryStatus::Delivered),
            ("failed", DeliveryStatus::FailedPermanent),
            ("REJECTED", DeliveryStatus::FailedPermanent),
            ("queued", DeliveryStatus::Sending),
        ] {
            let mapped = match reported.to_ascii_uppercase().as_str() {
                "DELIVERED" | "SUCCESS" | "COMPLETED" => DeliveryStatus::Delivered,
                "FAILED" | "REJECTED" | "BOUNCED" => DeliveryStatus::FailedPermanent,
                _ => DeliveryStatus::Sending,
            };
            assert_eq!(mapped, expected, "{reported}");
        }
    }
}
