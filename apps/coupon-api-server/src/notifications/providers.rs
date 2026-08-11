//! 외부 발송 제공자 (§15.1, §15.4, NOTIFY-003, §19.3).
//!
//! Two seams, one shape. §15.1 is explicit that 알림톡 must sit behind a replaceable
//! `AlimtalkProvider` interface — the business relationship there is with a 대행사 and it
//! changes — and web push has the same problem for a different reason: FCM's own transport
//! is a moving target. So both are traits, both are `dyn`, and the delivery pipeline never
//! names a vendor.
//!
//! What the pipeline *does* name is the outcome vocabulary. [`ProviderOutcome`] is the only
//! thing a provider may say, and it maps onto §15.4's statuses without the caller having to
//! interpret an HTTP code. That is what makes NOTIFY-003 implementable: an expired FCM
//! token is a `PermanentFailure` that disables the subscription, a 429 is a
//! `RetryableFailure` carrying the provider's own `Retry-After`, and a 5xx is a
//! `RetryableFailure` on our schedule.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use uuid::Uuid;

use crate::jobs::transport::BoxFuture;

/// What a provider is asked to send.
#[derive(Debug, Clone)]
pub struct ProviderMessage {
    pub delivery_id: Uuid,
    /// §18.3: the same id the API, the outbox and the job all carry.
    pub correlation_id: Uuid,
    /// The channel-specific address: an FCM registration token, or a phone number for
    /// 알림톡. Never logged.
    pub recipient: String,
    pub subject: String,
    pub body: String,
    /// The provider's own approved template id. Required for 알림톡 (§15.1).
    pub provider_template_id: Option<String>,
    /// Already escaped by the renderer (§15.2).
    pub variables: BTreeMap<String, String>,
}

/// Everything a provider may report, in the terms §15.4 defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOutcome {
    /// Handed off; the provider will confirm by callback. Maps to `SENDING`.
    ///
    /// §15.4 warns that `DELIVERED` follows the provider's own definition of acceptance or
    /// delivery and is never a read receipt, which is why acceptance gets its own state
    /// rather than being rounded up.
    Accepted { provider_reference: String },
    /// The provider considers the message delivered by its own definition.
    Delivered { provider_reference: String },
    /// Worth another attempt (§14.7 transient, or a 429 with the provider's schedule).
    RetryableFailure {
        code: String,
        message: String,
        retry_after: Option<Duration>,
    },
    /// Another attempt would fail identically (§14.6: 수신 거부·템플릿 거절).
    PermanentFailure {
        code: String,
        message: String,
        /// An expired or unregistered token: the subscription itself is dead and must be
        /// deactivated so it is not tried again (NOTIFY-003).
        recipient_gone: bool,
    },
}

impl ProviderOutcome {
    pub fn provider_reference(&self) -> Option<&str> {
        match self {
            ProviderOutcome::Accepted { provider_reference }
            | ProviderOutcome::Delivered { provider_reference } => Some(provider_reference),
            _ => None,
        }
    }
}

/// FCM Web Push (§15.1-2).
pub trait WebPushProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn send(&self, message: ProviderMessage) -> BoxFuture<'_, ProviderOutcome>;
}

/// 카카오 알림톡 (§15.1-3).
///
/// A separate trait rather than one `MessageProvider`: the two carry different obligations
/// — an 알림톡 send is invalid without an approved provider template id, and a web push is
/// invalid without a registration token — and collapsing them would put both checks at
/// every call site.
pub trait AlimtalkProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn send(&self, message: ProviderMessage) -> BoxFuture<'_, ProviderOutcome>;
}

// ---------------------------------------------------------------------------
// HTTP implementation
// ---------------------------------------------------------------------------

/// A provider reached over HTTP.
///
/// One implementation behind both traits: the wire shape a 알림톡 대행사 exposes and the one
/// an FCM relay exposes are the same POST-JSON-get-JSON, and the part that differs — which
/// fields are required — is enforced before the call rather than inside it. §19.3 asks for
/// 2xx/4xx/429/5xx coverage against a contract mock, and a single classifier is what makes
/// that one test rather than two.
pub struct HttpMessageProvider {
    name: &'static str,
    endpoint: String,
    authorization: Option<String>,
    client: reqwest::Client,
}

impl HttpMessageProvider {
    pub fn new(name: &'static str, endpoint: impl Into<String>, authorization: Option<String>) -> Self {
        Self {
            name,
            endpoint: endpoint.into(),
            authorization,
            client: reqwest::Client::builder()
                // A provider that will not answer inside ten seconds is a provider we
                // should retry rather than hold a worker slot for.
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn post(&self, message: ProviderMessage) -> ProviderOutcome {
        let payload = serde_json::json!({
            "delivery_id": message.delivery_id,
            "correlation_id": message.correlation_id,
            "to": message.recipient,
            "template_id": message.provider_template_id,
            "subject": message.subject,
            "body": message.body,
            "variables": message.variables,
        });

        let mut request = self.client.post(&self.endpoint).json(&payload);
        if let Some(authorization) = &self.authorization {
            request = request.header(reqwest::header::AUTHORIZATION, authorization);
        }

        let response = match request.send().await {
            Ok(response) => response,
            // A connection that never opened is exactly §14.7's network class.
            Err(error) => {
                return ProviderOutcome::RetryableFailure {
                    code: "PROVIDER_UNREACHABLE".to_owned(),
                    message: error.to_string(),
                    retry_after: None,
                };
            }
        };

        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);

        let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
        classify(self.name, status.as_u16(), retry_after, &body)
    }
}

/// Turn an HTTP answer into the §15.4 vocabulary.
///
/// Split out from the request so §19.3's status matrix is testable without a socket.
pub fn classify(
    provider: &str,
    status: u16,
    retry_after: Option<Duration>,
    body: &serde_json::Value,
) -> ProviderOutcome {
    let reference = body
        .get("provider_reference")
        .or_else(|| body.get("message_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);

    let code = body
        .get("error_code")
        .or_else(|| body.get("code"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();

    let message = body
        .get("error")
        .or_else(|| body.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();

    match status {
        200..=299 => {
            // A 2xx with no reference is still a successful hand-off; inventing an id would
            // make a later callback impossible to match, so the delivery keeps none and is
            // treated as delivered rather than pending a confirmation that cannot arrive.
            match reference {
                Some(reference) if body.get("delivered").and_then(serde_json::Value::as_bool) == Some(true) => {
                    ProviderOutcome::Delivered { provider_reference: reference }
                }
                Some(reference) => ProviderOutcome::Accepted { provider_reference: reference },
                None => ProviderOutcome::Delivered {
                    provider_reference: format!("{provider}:no-reference"),
                },
            }
        }

        // §14.7: provider 429/Retry-After 는 제공자 값을 우선한다.
        429 => ProviderOutcome::RetryableFailure {
            code: if code.is_empty() { "PROVIDER_THROTTLED".to_owned() } else { code },
            message,
            retry_after,
        },

        // NOTIFY-003: 영구 오류·수신 거부는 재시도하지 않고 채널 상태를 갱신한다.
        400..=499 => ProviderOutcome::PermanentFailure {
            recipient_gone: matches!(status, 404 | 410)
                || matches!(
                    code.as_str(),
                    "UNREGISTERED" | "NOT_FOUND" | "INVALID_TOKEN" | "RECIPIENT_UNSUBSCRIBED"
                ),
            code: if code.is_empty() {
                format!("PROVIDER_HTTP_{status}")
            } else {
                code
            },
            message,
        },

        _ => ProviderOutcome::RetryableFailure {
            code: if code.is_empty() {
                format!("PROVIDER_HTTP_{status}")
            } else {
                code
            },
            message,
            retry_after,
        },
    }
}

impl WebPushProvider for HttpMessageProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn send(&self, message: ProviderMessage) -> BoxFuture<'_, ProviderOutcome> {
        Box::pin(self.post(message))
    }
}

impl AlimtalkProvider for HttpMessageProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn send(&self, message: ProviderMessage) -> BoxFuture<'_, ProviderOutcome> {
        Box::pin(async move {
            // §15.1: 알림톡은 승인된 템플릿으로만 나간다. Refusing here rather than at the
            // provider keeps the reason in our own vocabulary and off the network.
            if message.provider_template_id.is_none() {
                return ProviderOutcome::PermanentFailure {
                    code: "TEMPLATE_NOT_APPROVED".to_owned(),
                    message: "알림톡은 승인된 템플릿 없이 발송할 수 없습니다.".to_owned(),
                    recipient_gone: false,
                };
            }
            self.post(message).await
        })
    }
}

// ---------------------------------------------------------------------------
// Development and test implementations
// ---------------------------------------------------------------------------

/// Records what it was asked to send and accepts it.
///
/// This is what runs when no provider endpoint is configured, which is every local run and
/// every test that is not specifically about the wire. `Config::validate` refuses it in
/// production, so "we forgot to configure FCM" cannot quietly become "everything is
/// delivered".
#[derive(Debug, Default)]
pub struct RecordingProvider {
    name: &'static str,
    sent: Mutex<Vec<ProviderMessage>>,
}

impl RecordingProvider {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            sent: Mutex::new(Vec::new()),
        }
    }

    pub fn sent(&self) -> Vec<ProviderMessage> {
        self.sent.lock().map(|sent| sent.clone()).unwrap_or_default()
    }

    fn record(&self, message: ProviderMessage) -> ProviderOutcome {
        let reference = format!("{}:{}", self.name, message.delivery_id);
        tracing::info!(
            provider = self.name,
            delivery_id = %message.delivery_id,
            correlation_id = %message.correlation_id,
            "notifications.provider_stub_send"
        );
        if let Ok(mut sent) = self.sent.lock() {
            sent.push(message);
        }
        ProviderOutcome::Delivered {
            provider_reference: reference,
        }
    }
}

impl WebPushProvider for RecordingProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn send(&self, message: ProviderMessage) -> BoxFuture<'_, ProviderOutcome> {
        let outcome = self.record(message);
        Box::pin(async move { outcome })
    }
}

impl AlimtalkProvider for RecordingProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn send(&self, message: ProviderMessage) -> BoxFuture<'_, ProviderOutcome> {
        let outcome = self.record(message);
        Box::pin(async move { outcome })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: serde_json::Value) -> serde_json::Value {
        json
    }

    #[test]
    fn a_2xx_with_a_reference_is_an_acceptance_not_a_delivery() {
        // §15.4: DELIVERED 는 제공자 정의를 따르며 사용자가 읽었다는 뜻이 아니다. An
        // acknowledged hand-off is `SENDING` until the provider says otherwise.
        assert_eq!(
            classify("fcm", 200, None, &body(serde_json::json!({ "provider_reference": "abc" }))),
            ProviderOutcome::Accepted {
                provider_reference: "abc".to_owned()
            }
        );
        assert_eq!(
            classify(
                "fcm",
                202,
                None,
                &body(serde_json::json!({ "message_id": "m-1", "delivered": true })),
            ),
            ProviderOutcome::Delivered {
                provider_reference: "m-1".to_owned()
            }
        );
    }

    #[test]
    fn a_429_carries_the_providers_own_schedule() {
        // §14.7: provider 429/Retry-After 는 제공자 값을 우선한다.
        let outcome = classify(
            "alimtalk",
            429,
            Some(Duration::from_secs(90)),
            &body(serde_json::json!({ "code": "RATE_LIMITED" })),
        );

        assert_eq!(
            outcome,
            ProviderOutcome::RetryableFailure {
                code: "RATE_LIMITED".to_owned(),
                message: String::new(),
                retry_after: Some(Duration::from_secs(90)),
            }
        );
    }

    #[test]
    fn a_4xx_is_permanent_and_a_5xx_is_not() {
        let permanent = classify("fcm", 400, None, &body(serde_json::json!({})));
        assert!(matches!(permanent, ProviderOutcome::PermanentFailure { .. }));

        let transient = classify("fcm", 503, None, &body(serde_json::json!({})));
        assert!(matches!(transient, ProviderOutcome::RetryableFailure { .. }));
    }

    #[test]
    fn an_unregistered_token_marks_the_recipient_gone() {
        // NOTIFY-003: FCM 만료 토큰은 비활성화한다.
        for (status, code) in [(404u16, ""), (410, ""), (400, "UNREGISTERED")] {
            let outcome = classify("fcm", status, None, &body(serde_json::json!({ "code": code })));
            match outcome {
                ProviderOutcome::PermanentFailure { recipient_gone, .. } => {
                    assert!(recipient_gone, "status {status} code {code:?}")
                }
                other => panic!("expected a permanent failure, got {other:?}"),
            }
        }

        // A plain validation error is permanent but says nothing about the token.
        let outcome = classify("fcm", 422, None, &body(serde_json::json!({ "code": "BAD_PAYLOAD" })));
        assert!(matches!(
            outcome,
            ProviderOutcome::PermanentFailure {
                recipient_gone: false,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn alimtalk_refuses_to_send_without_an_approved_template() {
        // §15.1: 알림톡은 승인된 정보성 템플릿 전용이다.
        let provider = HttpMessageProvider::new("alimtalk", "http://127.0.0.1:1/never", None);
        let outcome = AlimtalkProvider::send(
            &provider,
            ProviderMessage {
                delivery_id: Uuid::from_u128(1),
                correlation_id: Uuid::from_u128(2),
                recipient: "01000000000".to_owned(),
                subject: String::new(),
                body: "본문".to_owned(),
                provider_template_id: None,
                variables: BTreeMap::new(),
            },
        )
        .await;

        assert!(matches!(
            outcome,
            ProviderOutcome::PermanentFailure { ref code, .. } if code == "TEMPLATE_NOT_APPROVED"
        ));
    }

    #[tokio::test]
    async fn the_recording_provider_reports_what_it_was_given() {
        let provider = RecordingProvider::new("fcm-stub");
        let outcome = WebPushProvider::send(
            &provider,
            ProviderMessage {
                delivery_id: Uuid::from_u128(9),
                correlation_id: Uuid::from_u128(10),
                recipient: "token".to_owned(),
                subject: "제목".to_owned(),
                body: "본문".to_owned(),
                provider_template_id: None,
                variables: BTreeMap::new(),
            },
        )
        .await;

        assert!(matches!(outcome, ProviderOutcome::Delivered { .. }));
        assert_eq!(provider.sent().len(), 1);
        assert_eq!(provider.sent()[0].body, "본문");
    }
}
