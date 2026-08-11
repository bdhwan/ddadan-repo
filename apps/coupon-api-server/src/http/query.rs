//! `?a=b` parsed into a typed struct, rejected the way §11.1 rejects everything else.
//!
//! [`axum::extract::Query`] answers a malformed query string with a bare `text/plain`
//! 400 — the one response shape no client of this API is written to read, since every
//! other error arrives as an `{ "error": { "code", "message", "request_id" } }` envelope.
//! A client that hits `?limit=abc` therefore gets a 400 it cannot render and cannot log
//! by code. This wrapper is a drop-in replacement that produces `VALIDATION_FAILED` with
//! a field error naming the offending parameter.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::error::{ApiError, ErrorCode, FieldError};

/// `?a=b`, with a rejection that looks like the rest of the API.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Self(value)),
            Err(rejection) => {
                let detail = rejection.body_text();
                Err(ApiError::with_fields(
                    ErrorCode::ValidationFailed,
                    vec![FieldError::new(
                        offending_parameter(&detail),
                        "INVALID_QUERY_PARAMETER",
                        ErrorCode::ValidationFailed.default_message(),
                    )],
                )
                // The parser's own words are useful in a log and meaningless to a user.
                .internal(detail))
            }
        }
    }
}

/// Pull the parameter name out of axum's rejection text.
///
/// The text reads `Failed to deserialize query string: limit: invalid digit found in
/// string` — the field, then the reason. Some failures (`duplicate field \`limit\``)
/// carry no such prefix, and rather than grow a parser for each we fall back to naming
/// the query string as a whole; the exact reason is in the log either way.
fn offending_parameter(detail: &str) -> &str {
    const PREFIX: &str = "Failed to deserialize query string: ";

    let Some(rest) = detail.strip_prefix(PREFIX) else {
        return "query";
    };
    let Some((field, _)) = rest.split_once(": ") else {
        return "query";
    };

    // A reason that slipped through without a field name would arrive here as a sentence.
    if field.is_empty() || field.contains(' ') {
        "query"
    } else {
        field
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offending_parameter_is_named_when_axum_names_it() {
        assert_eq!(
            offending_parameter(
                "Failed to deserialize query string: limit: invalid digit found in string"
            ),
            "limit"
        );
        assert_eq!(
            offending_parameter(
                "Failed to deserialize query string: store_id: UUID parsing failed: \
                 invalid character: found `n` at 0"
            ),
            "store_id"
        );
    }

    #[test]
    fn an_unattributable_failure_falls_back_to_the_query_string() {
        assert_eq!(
            offending_parameter("Failed to deserialize query string: duplicate field `limit`"),
            "query"
        );
        assert_eq!(offending_parameter("something else entirely"), "query");
    }
}
