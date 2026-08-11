//! Assigns a request id, opens the tracing span every log line inherits (§18.3), and
//! echoes the id back in `X-Request-Id`.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use tracing::field::Empty;
use tracing::{Instrument, info_span};

use crate::http::request_id;
use crate::telemetry;

pub const REQUEST_ID_HEADER: &str = "x-request-id";

pub async fn layer(request: Request, next: Next) -> Response {
    // Honour a caller-supplied id only if it is short and printable; otherwise it is a
    // log-injection vector.
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            (1..=64).contains(&value.len())
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(str::to_owned)
        .unwrap_or_else(request_id::generate);

    let method = request.method().clone();
    let path = request.uri().path().to_owned();

    // `actor_id`, `store_id` and `transaction_id` start empty and are recorded once the
    // request knows them, so a single span carries the whole §18.3 field set.
    let span = info_span!(
        "http_request",
        request_id = %request_id,
        http.method = %method,
        http.path = %path,
        http.status = Empty,
        actor_id = Empty,
        store_id = Empty,
        transaction_id = Empty,
    );

    let headers = telemetry::redact_headers(request.headers());
    let scoped_id = request_id.clone();

    async move {
        tracing::debug!(?headers, "request received");

        let mut response = request_id::scope(scoped_id, next.run(request)).await;

        tracing::Span::current().record("http.status", response.status().as_u16());
        tracing::info!(status = response.status().as_u16(), "request completed");

        if let Ok(value) = HeaderValue::from_str(&request_id) {
            response.headers_mut().insert(REQUEST_ID_HEADER, value);
        }
        response
    }
    .instrument(span)
    .await
}
