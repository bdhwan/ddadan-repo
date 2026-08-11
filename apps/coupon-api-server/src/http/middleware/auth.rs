//! Authentication layer (§9.3).
//!
//! Runs once per protected request: verify the credential, then load account status and
//! roles from PostgreSQL. Nothing downstream trusts a token claim for authorisation.

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::{AuthContext, DEV_AUTH_AGE_HEADER, DEV_UID_HEADER, VerifiedToken};
use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;

pub async fn layer(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    // Copy what we need out of the request before any await. A `&Request` cannot be held
    // across one — the body is `Send` but not `Sync`, which would make this future
    // non-`Send` and unusable as a tower layer.
    let credential = match Credential::extract(&state, request.headers()) {
        Ok(credential) => credential,
        Err(error) => return error.into_response(),
    };

    let token = match credential.verify(&state).await {
        Ok(token) => token,
        Err(error) => return error.into_response(),
    };

    let account = match state
        .auth
        .load_account(&state.pool, &token.firebase_uid)
        .await
    {
        Ok(account) => account,
        Err(error) => return error.into_response(),
    };

    // §18.3: once we know who is calling, every later log line in this request carries it.
    if let Some(account) = &account {
        tracing::Span::current().record("actor_id", tracing::field::display(account.user_id));
    }

    request
        .extensions_mut()
        .insert(AuthContext { token, account });
    next.run(request).await
}

/// What the caller presented, owned so it can outlive the borrow on the request.
enum Credential {
    Bearer(String),
    /// Only constructible while `COUPON_AUTH_DEV_BYPASS=1`.
    DevBypass {
        firebase_uid: String,
        auth_age_secs: i64,
    },
}

impl Credential {
    fn extract(state: &AppState, headers: &HeaderMap) -> Result<Self, ApiError> {
        // The bypass is checked first so a developer never needs a real token. It can
        // only be enabled outside production — `Config::validate` refuses to boot
        // otherwise.
        if state.auth.dev_bypass_enabled() {
            if let Some(uid) = headers
                .get(DEV_UID_HEADER)
                .and_then(|value| value.to_str().ok())
            {
                let auth_age_secs = headers
                    .get(DEV_AUTH_AGE_HEADER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0);

                tracing::warn!(
                    firebase_uid = uid,
                    "authenticated via COUPON_AUTH_DEV_BYPASS"
                );
                return Ok(Credential::DevBypass {
                    firebase_uid: uid.to_owned(),
                    auth_age_secs,
                });
            }
        }

        let header = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(ApiError::unauthenticated)?;

        header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .map(str::trim)
            .filter(|credential| !credential.is_empty())
            .map(|credential| Credential::Bearer(credential.to_owned()))
            .ok_or_else(|| {
                ApiError::with_message(
                    ErrorCode::Unauthenticated,
                    "Authorization 헤더는 Bearer 토큰이어야 합니다.",
                )
            })
    }

    async fn verify(self, state: &AppState) -> Result<VerifiedToken, ApiError> {
        match self {
            Credential::Bearer(token) => state.auth.verify_bearer(&token).await,
            Credential::DevBypass {
                firebase_uid,
                auth_age_secs,
            } => state.auth.dev_token(&firebase_uid, auth_age_secs),
        }
    }
}
