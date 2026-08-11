//! Terms and channel consent (§9.4, §10.2 `consents`).
//!
//! `coupon.consent_events` is append-only and enforced as such by a trigger: a
//! withdrawal is a new `REVOKED` row, never an edit. Current state is therefore always
//! *derived* — the latest event per scope wins — so the evidence trail and the answer
//! can never drift apart.

pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::crypto::LookupHash;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::notifications::{NotificationChannel, NotificationPreferenceService, PreferenceUpdate};

pub use routes::consents_router;

/// The consent scopes from the §9.4 table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConsentScope {
    /// 서비스 이용약관 — required.
    TermsOfService,
    /// 개인정보 수집·이용 — required.
    PrivacyPolicy,
    /// 위치 기반 공개 상점 검색.
    LocationBasedSearch,
    /// 서비스 거래 Web Push.
    TransactionalWebPush,
    /// 카카오 정보성 알림.
    KakaoInformational,
    /// 전체 마케팅.
    MarketingAll,
    /// 상점별 마케팅 — requires `store_id`.
    MarketingStore,
}

impl ConsentScope {
    pub fn as_db(self) -> &'static str {
        match self {
            ConsentScope::TermsOfService => "TERMS_OF_SERVICE",
            ConsentScope::PrivacyPolicy => "PRIVACY_POLICY",
            ConsentScope::LocationBasedSearch => "LOCATION_BASED_SEARCH",
            ConsentScope::TransactionalWebPush => "TRANSACTIONAL_WEB_PUSH",
            ConsentScope::KakaoInformational => "KAKAO_INFORMATIONAL",
            ConsentScope::MarketingAll => "MARKETING_ALL",
            ConsentScope::MarketingStore => "MARKETING_STORE",
        }
    }

    pub fn from_db(raw: &str) -> Option<Self> {
        Some(match raw {
            "TERMS_OF_SERVICE" => ConsentScope::TermsOfService,
            "PRIVACY_POLICY" => ConsentScope::PrivacyPolicy,
            "LOCATION_BASED_SEARCH" => ConsentScope::LocationBasedSearch,
            "TRANSACTIONAL_WEB_PUSH" => ConsentScope::TransactionalWebPush,
            "KAKAO_INFORMATIONAL" => ConsentScope::KakaoInformational,
            "MARKETING_ALL" => ConsentScope::MarketingAll,
            "MARKETING_STORE" => ConsentScope::MarketingStore,
            _ => return None,
        })
    }

    /// Required consents cannot be revoked while the account is in use (§9.4): the
    /// withdrawal flow handles that, not a toggle.
    pub fn is_required(self) -> bool {
        matches!(
            self,
            ConsentScope::TermsOfService | ConsentScope::PrivacyPolicy
        )
    }

    /// Whether the scope is meaningful only for one store.
    pub fn is_store_scoped(self) -> bool {
        matches!(self, ConsentScope::MarketingStore)
    }

    /// The notification preference this scope projects onto, if any.
    fn preference(self) -> Option<(&'static str, NotificationChannel)> {
        match self {
            ConsentScope::TransactionalWebPush => {
                Some(("TRANSACTIONAL", NotificationChannel::FcmWebPush))
            }
            ConsentScope::KakaoInformational => {
                Some(("INFORMATIONAL", NotificationChannel::KakaoAlimtalk))
            }
            ConsentScope::MarketingAll | ConsentScope::MarketingStore => {
                Some(("MARKETING", NotificationChannel::FcmWebPush))
            }
            // Terms, privacy and location consent gate features, not sends.
            _ => None,
        }
    }

    pub fn all() -> [ConsentScope; 7] {
        [
            ConsentScope::TermsOfService,
            ConsentScope::PrivacyPolicy,
            ConsentScope::LocationBasedSearch,
            ConsentScope::TransactionalWebPush,
            ConsentScope::KakaoInformational,
            ConsentScope::MarketingAll,
            ConsentScope::MarketingStore,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConsentAction {
    Granted,
    Revoked,
}

impl ConsentAction {
    pub fn as_db(self) -> &'static str {
        match self {
            ConsentAction::Granted => "GRANTED",
            ConsentAction::Revoked => "REVOKED",
        }
    }

    pub fn is_granted(self) -> bool {
        matches!(self, ConsentAction::Granted)
    }
}

/// One consent as it stands right now, derived from the latest event.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConsentState {
    pub scope: ConsentScope,
    pub store_id: Option<Uuid>,
    pub granted: bool,
    pub required: bool,
    pub document_version: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConsentsResponse {
    pub consents: Vec<ConsentState>,
}

/// One change in a `POST /me/consents` batch.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct ConsentChange {
    pub scope: ConsentScope,
    pub action: ConsentAction,
    /// Required for store-scoped consent, rejected otherwise.
    pub store_id: Option<Uuid>,
    /// Version label of the document shown to the user, e.g. `2026-08-01`.
    #[validate(length(max = 32))]
    pub document_version: Option<String>,
    /// The screen that collected the consent (§9.4), e.g. `signup`, `settings/alerts`.
    #[validate(length(min = 1, max = 64, message = "수집 화면을 지정해야 합니다."))]
    pub source: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateConsentsRequest {
    #[validate(length(min = 1, max = 50, message = "한 번에 1~50개까지 변경할 수 있습니다."))]
    #[validate(nested)]
    pub consents: Vec<ConsentChange>,
}

/// Evidence captured about *how* a consent was given (§9.4).
#[derive(Debug, Clone)]
pub struct ConsentEvidence {
    pub ip: Option<String>,
    pub user_agent_class: String,
}

pub struct ConsentService {
    lookup_hash: Arc<LookupHash>,
    preferences: Arc<NotificationPreferenceService>,
}

impl ConsentService {
    pub fn new(
        lookup_hash: Arc<LookupHash>,
        preferences: Arc<NotificationPreferenceService>,
    ) -> Self {
        Self {
            lookup_hash,
            preferences,
        }
    }

    /// Current consent state: the latest event per `(scope, store_id)`, with every scope
    /// the user has never touched reported as not granted.
    pub async fn current(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<Vec<ConsentState>> {
        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT ON (e.scope, COALESCE(e.store_id, '00000000-0000-0000-0000-000000000000'::uuid))
                e.scope,
                e.store_id,
                e.action::text AS "action!",
                e.occurred_at,
                d.version_label AS "version_label?"
            FROM coupon.consent_events e
            LEFT JOIN coupon.terms_documents d ON d.id = e.document_id
            WHERE e.user_id = $1
            ORDER BY
                e.scope,
                COALESCE(e.store_id, '00000000-0000-0000-0000-000000000000'::uuid),
                e.occurred_at DESC
            "#,
            user_id,
        )
        .fetch_all(pool)
        .await?;

        let mut states: Vec<ConsentState> = rows
            .into_iter()
            .filter_map(|row| {
                let scope = ConsentScope::from_db(&row.scope)?;
                Some(ConsentState {
                    scope,
                    store_id: row.store_id,
                    granted: row.action == ConsentAction::Granted.as_db(),
                    required: scope.is_required(),
                    document_version: row.version_label,
                    decided_at: Some(row.occurred_at),
                })
            })
            .collect();

        // Report untouched account-wide scopes explicitly, so a client renders a full
        // settings screen from one response.
        for scope in ConsentScope::all() {
            if scope.is_store_scoped() {
                continue;
            }
            if states
                .iter()
                .any(|state| state.scope == scope && state.store_id.is_none())
            {
                continue;
            }
            states.push(ConsentState {
                scope,
                store_id: None,
                granted: false,
                required: scope.is_required(),
                document_version: None,
                decided_at: None,
            });
        }

        Ok(states)
    }

    /// Append consent events and project them onto notification preferences.
    ///
    /// The whole batch is one transaction: a half-applied consent screen would leave
    /// evidence that disagrees with behaviour.
    pub async fn record(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        request: &UpdateConsentsRequest,
        evidence: &ConsentEvidence,
    ) -> ApiResult<Vec<ConsentState>> {
        for change in &request.consents {
            validate_change(change)?;
        }

        let ip_hash = evidence
            .ip
            .as_deref()
            .map(|ip| self.lookup_hash.hash_ip(ip));
        let mut tx = pool.begin().await?;

        for change in &request.consents {
            // Resolve the document the user actually agreed to, when one was named.
            let document_id = match (&change.document_version, change.scope) {
                (Some(version), scope) => {
                    sqlx::query_scalar!(
                        r#"
                        SELECT id FROM coupon.terms_documents
                        WHERE document_type = $1 AND version_label = $2
                        ORDER BY effective_at DESC
                        LIMIT 1
                        "#,
                        scope.as_db(),
                        version,
                    )
                    .fetch_optional(&mut *tx)
                    .await?
                }
                _ => None,
            };

            let event_id = sqlx::query_scalar!(
                r#"
                INSERT INTO coupon.consent_events
                    (user_id, document_id, scope, store_id, channel, action, source,
                     ip_hash, user_agent_class)
                VALUES ($1, $2, $3, $4, $5::text::coupon.notification_channel,
                        $6::text::coupon.consent_action, $7, $8, $9)
                RETURNING id
                "#,
                user_id,
                document_id,
                change.scope.as_db(),
                change.store_id,
                change
                    .scope
                    .preference()
                    .map(|(_, channel)| channel.as_db()),
                change.action.as_db(),
                change.source,
                ip_hash,
                evidence.user_agent_class,
            )
            .fetch_one(&mut *tx)
            .await?;

            if let Some((purpose, channel)) = change.scope.preference() {
                self.preferences
                    .apply(
                        &mut tx,
                        &PreferenceUpdate {
                            user_id,
                            store_id: change.store_id,
                            purpose: purpose.to_owned(),
                            channel,
                            enabled: change.action.is_granted(),
                            source_consent_event_id: event_id,
                        },
                    )
                    .await?;
            }
        }

        tx.commit().await?;

        self.current(pool, user_id).await
    }
}

/// Reject changes the §9.4 table does not allow, before anything is written.
fn validate_change(change: &ConsentChange) -> ApiResult<()> {
    if change.scope.is_store_scoped() && change.store_id.is_none() {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "상점별 동의에는 store_id 가 필요합니다.",
        ));
    }

    if !change.scope.is_store_scoped() && change.store_id.is_some() {
        return Err(ApiError::with_message(
            ErrorCode::ValidationFailed,
            "이 동의 항목에는 store_id 를 지정할 수 없습니다.",
        ));
    }

    // Revoking the service terms is withdrawal, which has its own re-authenticated flow.
    if change.scope.is_required() && change.action == ConsentAction::Revoked {
        return Err(ApiError::with_message(
            ErrorCode::UnprocessableRequest,
            "필수 동의는 철회할 수 없습니다. 탈퇴를 진행해 주세요.",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(scope: ConsentScope, action: ConsentAction, store_id: Option<Uuid>) -> ConsentChange {
        ConsentChange {
            scope,
            action,
            store_id,
            document_version: None,
            source: "signup".to_owned(),
        }
    }

    #[test]
    fn scopes_round_trip_through_their_database_spelling() {
        for scope in ConsentScope::all() {
            assert_eq!(ConsentScope::from_db(scope.as_db()), Some(scope));
        }
        assert_eq!(ConsentScope::from_db("SOMETHING_NEW"), None);
    }

    #[test]
    fn only_terms_and_privacy_are_required() {
        assert!(ConsentScope::TermsOfService.is_required());
        assert!(ConsentScope::PrivacyPolicy.is_required());
        assert!(!ConsentScope::MarketingAll.is_required());
        assert!(!ConsentScope::LocationBasedSearch.is_required());
    }

    #[test]
    fn required_consent_cannot_be_revoked_through_this_endpoint() {
        let error = validate_change(&change(
            ConsentScope::TermsOfService,
            ConsentAction::Revoked,
            None,
        ))
        .expect_err("must reject");

        assert_eq!(error.code, ErrorCode::UnprocessableRequest);
        assert_eq!(error.status().as_u16(), 422);
    }

    #[test]
    fn store_scoped_consent_needs_a_store_and_others_must_not_have_one() {
        let store = Some(Uuid::new_v4());

        validate_change(&change(
            ConsentScope::MarketingStore,
            ConsentAction::Granted,
            store,
        ))
        .expect("store-scoped consent with a store is fine");

        assert_eq!(
            validate_change(&change(
                ConsentScope::MarketingStore,
                ConsentAction::Granted,
                None
            ))
            .expect_err("missing store")
            .code,
            ErrorCode::ValidationFailed
        );
        assert_eq!(
            validate_change(&change(
                ConsentScope::MarketingAll,
                ConsentAction::Granted,
                store
            ))
            .expect_err("stray store")
            .code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn marketing_and_push_scopes_project_onto_a_send_channel() {
        assert_eq!(
            ConsentScope::TransactionalWebPush.preference(),
            Some(("TRANSACTIONAL", NotificationChannel::FcmWebPush))
        );
        assert_eq!(
            ConsentScope::KakaoInformational.preference(),
            Some(("INFORMATIONAL", NotificationChannel::KakaoAlimtalk))
        );
        assert_eq!(
            ConsentScope::TermsOfService.preference(),
            None,
            "agreeing to the terms is not a send permission"
        );
    }
}
