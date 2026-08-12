//! `/auth/kakao/*`, `/webhooks/kakao/unlink` and `/me/auth-links/kakao` (§11.2).
//!
//! The Kakao endpoints sit on the **public** router. None of them can carry a Firebase
//! token, because their entire purpose is to produce one — putting them on the
//! authenticated tree and exempting them would leave the exemption one refactor away
//! from being forgotten, exactly as the notification callback comment says.
//!
//! `/me/auth-links/kakao` is the opposite case and goes on the authenticated tree:
//! AUTH-003 requires the member to already be signed in as the account they want to
//! keep, and to have signed in *recently* (§9.3).

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use hex::encode as hex_encode;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use utoipa::{IntoParams, ToSchema};

use crate::auth::extractors::{CurrentUser, RecentlyAuthenticated};
use crate::auth::kakao::{
    AuthLink, AuthorizeStart, CallbackResult, SignInResult, UnlinkAck,
};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::client_ip;
use crate::http::query::Query;
use crate::http::rate_limit::Bucket;
use crate::http::response::{ApiMutation, ApiOk, TransactionId};
use crate::notifications::routes::verify_callback;
use crate::state::AppState;

/// How much of `state` goes into the §16.4 rate-limit key.
///
/// §16.4 keys the callback-failure limit on `IP+state prefix`. A prefix, not the whole
/// value: the point is to let one browser's own retries share a bucket while a script
/// cycling fresh states cannot get a fresh bucket each time by changing the tail. The
/// full `state` is a secret and has no business in a Redis keyspace either way.
const STATE_PREFIX_LEN: usize = 8;

/// Kakao's callbacks and login endpoints. Public: they produce credentials rather than
/// consuming them.
pub fn kakao_auth_router() -> Router<AppState> {
    Router::new()
        .route("/auth/kakao/authorize", get(authorize))
        .route("/auth/kakao/callback", get(callback))
        .route("/auth/kakao/exchange", post(exchange))
        .route("/webhooks/kakao/unlink", post(unlink_webhook))
}

/// §11.2 `POST/DELETE /me/auth-links/kakao`, plus the listing the 보안 screen needs
/// (§6.1 `/account/security`: 연결 로그인 수단).
pub fn me_auth_links_router() -> Router<AppState> {
    Router::new()
        .route("/me/auth-links", get(list_auth_links))
        .route(
            "/me/auth-links/kakao",
            post(link_kakao).delete(unlink_kakao),
        )
}

/// §9.2 steps 1–2.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/auth/kakao/authorize",
    tag = "auth",
    responses(
        (status = 200, description = "Where to send the browser", body = AuthorizeStart),
        (status = 429, description = "§16.4 로그인 시작 10회/10분"),
        (status = 503, description = "카카오 앱이 아직 등록되지 않았습니다"),
    ),
)]
pub async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<ApiOk<AuthorizeStart>> {
    // §16.4 로그인/가입 시작 10회/10분, keyed by IP. There is no account yet, so IP is
    // all there is — which is also why the limit only slows a start down and never bans
    // anything.
    state
        .rate_limiter
        .check(
            Bucket::LoginStart,
            &rate_limit_ip(&headers),
            state.config.rate_limit_login_start_per_10min,
            Utc::now(),
        )
        .await?;

    Ok(ApiOk(state.kakao.start_authorize(&state.pool).await?))
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct CallbackQuery {
    /// Kakao's authorization code. Absent when the user declined.
    pub code: Option<String>,
    pub state: Option<String>,
    /// Kakao's own error, e.g. `access_denied` when the user cancelled.
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// §9.2 steps 3–5.
#[utoipa::path(
    get,
    path = "/api/coupon/v1/auth/kakao/callback",
    tag = "auth",
    params(CallbackQuery),
    responses(
        (status = 200, description = "일회용 교환 코드", body = CallbackResult),
        (status = 400, description = "사용자가 취소했습니다"),
        (status = 401, description = "state·nonce·서명 등 보안 검증 실패"),
        (status = 429, description = "§16.4 카카오 callback 실패 20회/10분"),
        (status = 503, description = "카카오 일시 장애"),
    ),
)]
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> ApiResult<ApiOk<CallbackResult>> {
    let limit_key = callback_limit_key(&headers, query.state.as_deref());

    let outcome = complete(&state, &query).await;

    // §16.4 counts *failures* only. A successful callback is somebody signing in, and a
    // popular Monday morning is not an attack.
    if outcome.is_err() {
        state
            .rate_limiter
            .check(
                Bucket::KakaoCallbackFailure,
                &limit_key,
                state.config.rate_limit_kakao_callback_failure_per_10min,
                Utc::now(),
            )
            .await?;
    }

    Ok(ApiOk(outcome?))
}

/// The callback's actual work, separated so the rate limiter can see whether it failed
/// without the happy path having to remember to say so.
async fn complete(state: &AppState, query: &CallbackQuery) -> ApiResult<CallbackResult> {
    // §6.1: 취소 is one of the three outcomes a user is told apart. It is not a failure
    // and the SPA should simply return to /login.
    if let Some(error) = query.error.as_deref() {
        let detail = query.error_description.clone().unwrap_or_default();
        return Err(match error {
            "access_denied" | "consent_required" => ApiError::new(ErrorCode::KakaoLoginCancelled)
                .internal(format!("kakao returned {error}: {detail}")),
            other => ApiError::new(ErrorCode::DependencyUnavailable)
                .internal(format!("kakao returned {other}: {detail}")),
        });
    }

    let (Some(code), Some(login_state)) = (query.code.as_deref(), query.state.as_deref()) else {
        return Err(ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
            .internal("callback is missing code or state"));
    };

    state
        .kakao
        .complete_callback(&state.pool, login_state, code)
        .await
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ExchangeRequest {
    /// The single-use code `GET /auth/kakao/callback` returned.
    pub exchange_code: String,
}

/// §9.2 steps 6–7.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/auth/kakao/exchange",
    tag = "auth",
    request_body = ExchangeRequest,
    responses(
        (status = 200, description = "Firebase Custom Token", body = SignInResult),
        (status = 401, description = "코드가 이미 사용되었거나 만료되었습니다"),
        (status = 403, description = "정지·탈퇴 계정"),
        (status = 503, description = "Firebase 서비스 계정이 설정되지 않았습니다"),
    ),
)]
pub async fn exchange(
    State(state): State<AppState>,
    axum::Json(request): axum::Json<ExchangeRequest>,
) -> ApiResult<ApiOk<SignInResult>> {
    let result = state
        .kakao
        .exchange(&state.pool, request.exchange_code.trim())
        .await?;

    tracing::info!(
        user_id = %result.user_id,
        created = result.created,
        "auth.kakao.signed_in"
    );
    Ok(ApiOk(result))
}

/// Active login methods on my account (§6.1 `/account/security`).
#[utoipa::path(
    get,
    path = "/api/coupon/v1/me/auth-links",
    tag = "auth",
    responses((status = 200, description = "연결된 로그인 수단", body = Vec<AuthLink>)),
    security(("firebase" = [])),
)]
pub async fn list_auth_links(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<ApiOk<Vec<AuthLink>>> {
    Ok(ApiOk(
        state.kakao.links(&state.pool, user.account.user_id).await?,
    ))
}

/// AUTH-003: attach a Kakao account to the one I am signed in to.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/me/auth-links/kakao",
    tag = "auth",
    request_body = ExchangeRequest,
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "연결됨", body = AuthLink),
        (status = 401, description = "코드가 이미 사용되었거나 만료되었습니다"),
        (status = 403, description = "최근 로그인이 필요합니다"),
        (status = 409, description = "이미 다른 회원에 연결된 카카오 계정입니다"),
    ),
    security(("firebase" = [])),
)]
pub async fn link_kakao(
    State(state): State<AppState>,
    // AUTH-003: 로그인된 회원이 비밀번호 재확인 또는 최근 로그인을 수행한 후 연결한다.
    RecentlyAuthenticated(user): RecentlyAuthenticated,
    axum::Json(request): axum::Json<ExchangeRequest>,
) -> ApiResult<ApiMutation<AuthLink>> {
    let transaction_id = TransactionId::new();
    let link = state
        .kakao
        .link(
            &state.pool,
            user.account.user_id,
            request.exchange_code.trim(),
        )
        .await?;

    Ok(ApiMutation::ok(link, transaction_id))
}

/// §11.2 연결 해제.
#[utoipa::path(
    delete,
    path = "/api/coupon/v1/me/auth-links/kakao",
    tag = "auth",
    params(
        ("Idempotency-Key" = String, Header, description = "UUID, required on every mutation"),
    ),
    responses(
        (status = 200, description = "해제 후 남은 로그인 수단", body = Vec<AuthLink>),
        (status = 403, description = "최근 로그인이 필요합니다"),
        (status = 404, description = "연결된 카카오 계정이 없습니다"),
        (status = 409, description = "마지막 로그인 수단은 해제할 수 없습니다"),
    ),
    security(("firebase" = [])),
)]
pub async fn unlink_kakao(
    State(state): State<AppState>,
    RecentlyAuthenticated(user): RecentlyAuthenticated,
) -> ApiResult<ApiMutation<Vec<AuthLink>>> {
    let transaction_id = TransactionId::new();
    state.kakao.unlink(&state.pool, user.account.user_id).await?;
    let links = state.kakao.links(&state.pool, user.account.user_id).await?;

    Ok(ApiMutation::ok(links, transaction_id))
}

/// Kakao's unlink callback payload.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UnlinkWebhookBody {
    /// Kakao's numeric user id — the same value as the OIDC `sub`.
    #[schema(value_type = String)]
    pub user_id: serde_json::Value,
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub app_id: Option<serde_json::Value>,
    #[serde(default)]
    pub referrer_type: Option<String>,
}

/// 카카오 연결 해제 웹훅 (§9.2 마지막).
///
/// Authenticated by an HMAC over the body and its timestamp, exactly like the §15.4
/// delivery callbacks — the same helper, so there is one implementation of "is this
/// really the provider" to get right. The timestamp inside the signed material is what
/// makes a captured request unusable after five minutes; the event key derived from that
/// same material is what makes it a no-op even inside the window.
#[utoipa::path(
    post,
    path = "/api/coupon/v1/webhooks/kakao/unlink",
    tag = "auth",
    request_body = UnlinkWebhookBody,
    params(
        ("X-Kakao-Signature" = String, Header, description = "Hex HMAC-SHA256 over `<timestamp>.<body>`"),
        ("X-Kakao-Signature-Timestamp" = String, Header, description = "RFC 3339, within 5 minutes"),
    ),
    responses(
        (status = 200, description = "처리 결과", body = UnlinkAck),
        (status = 401, description = "서명 검증 실패"),
        (status = 503, description = "웹훅 비밀이 설정되지 않았습니다"),
    ),
)]
pub async fn unlink_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<ApiOk<UnlinkAck>> {
    let Some(secret) = state.config.kakao_webhook_secret.as_deref() else {
        // With no secret there is no way to tell Kakao from anyone else, and an
        // unauthenticated unlink would let a stranger cut any member's login.
        return Err(ApiError::with_message(
            ErrorCode::DependencyUnavailable,
            "카카오 웹훅을 처리할 수 없습니다.",
        )
        .internal("COUPON_KAKAO_WEBHOOK_SECRET is not configured"));
    };

    let signature = required_header(&headers, "x-kakao-signature")?;
    let timestamp = required_header(&headers, "x-kakao-signature-timestamp")?;
    let signed_at = DateTime::parse_from_rfc3339(&timestamp)
        .map_err(|_| ApiError::new(ErrorCode::WebhookSignatureInvalid))?
        .with_timezone(&Utc);

    if !verify_callback(secret, &timestamp, &body, &signature, Utc::now(), signed_at) {
        tracing::warn!("auth.kakao.unlink_webhook_signature_invalid");
        return Err(ApiError::new(ErrorCode::WebhookSignatureInvalid));
    }

    let parsed: UnlinkWebhookBody = serde_json::from_slice(&body)
        .map_err(|error| ApiError::new(ErrorCode::InvalidRequest).internal(error.to_string()))?;

    let provider_subject = provider_subject(&parsed.user_id).ok_or_else(|| {
        ApiError::new(ErrorCode::InvalidRequest).internal("unlink webhook has no usable user_id")
    })?;

    let ack = state
        .kakao
        .handle_unlink(
            &state.pool,
            &event_key(&timestamp, &body),
            &provider_subject,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
        .await?;

    tracing::info!(outcome = ?ack.outcome, "auth.kakao.unlink_webhook");
    Ok(ApiOk(ack))
}

/// Dedupe key for one webhook delivery.
///
/// Derived from the *signed material* — the timestamp and the body together — so it is
/// exactly as replayable as the signature is. A verbatim replay produces the same key and
/// changes nothing; a genuinely new event, even about the same member, produces a
/// different one and is applied.
fn event_key(timestamp: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(timestamp.as_bytes());
    hasher.update(b".");
    hasher.update(body);
    hex_encode(hasher.finalize())
}

/// Kakao sends `user_id` as a JSON number; a string is accepted too rather than refusing
/// a delivery over a serialisation detail.
fn provider_subject(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(text) if !text.trim().is_empty() => {
            Some(text.trim().to_owned())
        }
        _ => None,
    }
}

fn required_header(headers: &HeaderMap, name: &str) -> ApiResult<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::new(ErrorCode::WebhookSignatureInvalid))
}

/// §16.4 keys the callback-failure limit on IP + `state` prefix.
fn callback_limit_key(headers: &HeaderMap, state: Option<&str>) -> String {
    let prefix: String = state
        .unwrap_or("none")
        .chars()
        .take(STATE_PREFIX_LEN)
        .collect();
    format!("{}:{prefix}", rate_limit_ip(headers))
}

/// The IP half of a rate-limit key.
///
/// A request with no forwarded IP shares one bucket rather than getting a free pass:
/// §16.4 is explicit that an IP never *bans* anything, so the cost of a shared bucket is
/// a 429 the caller can retry out of, and the cost of a free pass is no limit at all.
fn rate_limit_ip(headers: &HeaderMap) -> String {
    client_ip(headers).unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        map
    }

    #[test]
    fn the_callback_limit_key_is_ip_plus_a_state_prefix() {
        let key = callback_limit_key(
            &headers(&[("x-forwarded-for", "203.0.113.7, 10.0.0.1")]),
            Some("abcdefghijklmnop"),
        );
        assert_eq!(key, "203.0.113.7:abcdefgh");
    }

    #[test]
    fn a_caller_cycling_states_cannot_cycle_buckets_by_changing_the_tail() {
        // The prefix is the point: two attempts from one browser share a bucket, and a
        // script that only varies the tail of `state` shares it too.
        let from = headers(&[("x-forwarded-for", "203.0.113.7")]);
        assert_eq!(
            callback_limit_key(&from, Some("abcdefgh-first")),
            callback_limit_key(&from, Some("abcdefgh-second"))
        );
    }

    #[test]
    fn a_missing_state_or_ip_still_produces_a_bucket() {
        assert_eq!(callback_limit_key(&HeaderMap::new(), None), "unknown:none");
    }

    #[test]
    fn the_event_key_covers_the_whole_signed_material() {
        let first = event_key("2026-08-14T00:00:00Z", b"{\"user_id\":1}");

        assert_eq!(
            first,
            event_key("2026-08-14T00:00:00Z", b"{\"user_id\":1}"),
            "a verbatim replay must be recognised"
        );
        assert_ne!(
            first,
            event_key("2026-08-14T00:05:00Z", b"{\"user_id\":1}"),
            "a re-signed delivery is a new event"
        );
        assert_ne!(
            first,
            event_key("2026-08-14T00:00:00Z", b"{\"user_id\":2}"),
            "a different member is a different event"
        );
    }

    #[test]
    fn a_kakao_user_id_is_read_as_a_number_or_a_string() {
        assert_eq!(
            provider_subject(&serde_json::json!(3_141_592_653_u64)).as_deref(),
            Some("3141592653")
        );
        assert_eq!(
            provider_subject(&serde_json::json!(" 3141592653 ")).as_deref(),
            Some("3141592653")
        );
        assert_eq!(provider_subject(&serde_json::json!("")), None);
        assert_eq!(provider_subject(&serde_json::Value::Null), None);
    }
}
