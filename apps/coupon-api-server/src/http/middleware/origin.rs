//! Origin check for state-changing requests (§16.3).
//!
//! Bearer tokens are not sent automatically by browsers, so this is defence in depth
//! rather than the primary CSRF control. It costs one header comparison and closes the
//! gap if a future endpoint ever accepts a cookie.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;

pub async fn layer(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if let Err(error) = check(&state, &request) {
        return error.into_response();
    }
    next.run(request).await
}

fn check(state: &AppState, request: &Request) -> Result<(), ApiError> {
    if !is_state_changing(request.method()) {
        return Ok(());
    }

    let Some(origin) = request
        .headers()
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        // Same-origin browser requests and non-browser clients (curl, the mobile shell,
        // server-to-server) send no Origin at all. Bearer auth still applies to them.
        return Ok(());
    };

    // An empty allowlist means "not configured yet", which only happens outside
    // production — `Config::validate` requires entries there.
    let allowlist = state.config.allowed_origins.as_slice();
    if allowlist.is_empty() || allowlist.iter().any(|allowed| allowed == origin) {
        return Ok(());
    }

    Err(ApiError::new(ErrorCode::OriginNotAllowed).internal(format!("rejected origin {origin}")))
}

fn is_state_changing(method: &axum::http::Method) -> bool {
    use axum::http::Method;
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[test]
    fn reads_are_never_origin_checked() {
        assert!(!is_state_changing(&Method::GET));
        assert!(!is_state_changing(&Method::HEAD));
        assert!(!is_state_changing(&Method::OPTIONS));
    }

    #[test]
    fn every_mutating_verb_is_checked() {
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(is_state_changing(&method), "{method} must be checked");
        }
    }
}
