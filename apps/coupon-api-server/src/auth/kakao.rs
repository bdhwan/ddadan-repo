//! Kakao OIDC login (§9.2).
//!
//! Out of scope for Phase 1 — Firebase email/password is the only sign-in path that
//! ships first. The interface is fixed here so `auth/mod.rs`, the router and the
//! `auth_identities` table do not have to be reshaped when Phase 2 wires it up.
//!
//! TODO(phase-2): implement against Kakao's OIDC discovery document.
//!   - `GET  /auth/kakao/authorize` — mint `state` + PKCE verifier, return the authorize
//!     URL, and stash the verifier in a short-lived HttpOnly/Secure/SameSite=Lax cookie
//!     (§16.3).
//!   - `GET  /auth/kakao/callback`  — verify `state`, exchange the code, validate the
//!     `id_token` against Kakao's JWKS, and issue a single-use exchange code.
//!   - `POST /auth/kakao/exchange`  — trade the exchange code for a Firebase custom
//!     token, linking `auth_identities (provider='KAKAO', provider_subject=sub)`.
//!   - `POST /webhooks/kakao/unlink` — idempotently mark the identity `UNLINKED`.

use crate::error::{ApiError, ApiResult, ErrorCode};

/// A Kakao identity resolved from an OIDC `id_token`.
#[derive(Debug, Clone)]
pub struct KakaoIdentity {
    /// Kakao's `sub`. Stored as `auth_identities.provider_subject`, never as a user id.
    pub provider_subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
}

/// The seam Phase 2 will implement. Kept as a trait so the HTTP layer can be written and
/// tested against a fake before the real provider exists.
pub trait KakaoOidcProvider: Send + Sync {
    fn authorize_url(&self, state: &str, code_challenge: &str) -> String;

    fn verify_id_token(
        &self,
        id_token: &str,
    ) -> impl std::future::Future<Output = ApiResult<KakaoIdentity>> + Send;
}

/// Placeholder wired into the router so the endpoints exist and fail honestly rather
/// than 404-ing with no explanation.
pub fn not_implemented() -> ApiError {
    ApiError::with_message(
        ErrorCode::ServiceUnavailable,
        "카카오 로그인은 아직 준비 중입니다.",
    )
}
