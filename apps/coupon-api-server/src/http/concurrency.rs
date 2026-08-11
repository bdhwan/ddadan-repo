//! Optimistic concurrency helpers (§11.1).
//!
//! Mutable aggregates carry a `version` column that a trigger bumps on every UPDATE. A
//! client that read version 7 sends it back — as `If-Match: "7"` or as `version` in the
//! body — and the UPDATE is scoped `WHERE version = 7`. Zero rows changed means someone
//! else got there first, which is a 409, not a silent overwrite.

use axum::http::HeaderMap;
use axum::http::header::IF_MATCH;

use crate::error::{ApiError, ApiResult, ErrorCode};

/// The expected version for this request, from `If-Match` or the request body.
///
/// `body_version` wins when both are present and agree; a disagreement is a client bug
/// and is rejected rather than guessed at.
pub fn expected_version(headers: &HeaderMap, body_version: Option<i64>) -> ApiResult<Option<i64>> {
    let header_version = parse_if_match(headers)?;

    match (header_version, body_version) {
        (Some(header), Some(body)) if header != body => Err(ApiError::with_message(
            ErrorCode::InvalidVersion,
            "If-Match 헤더와 본문의 version 이 서로 다릅니다.",
        )),
        (header, body) => Ok(body.or(header)),
    }
}

/// Parse `If-Match: "7"` (quoted per RFC 9110) or the bare `7` clients tend to send.
fn parse_if_match(headers: &HeaderMap) -> ApiResult<Option<i64>> {
    let Some(raw) = headers.get(IF_MATCH) else {
        return Ok(None);
    };

    let value = raw
        .to_str()
        .map_err(|_| ApiError::new(ErrorCode::InvalidVersion))?
        .trim();

    if value == "*" {
        // "any current version" — the caller opts out of the check.
        return Ok(None);
    }

    value
        .trim_start_matches("W/")
        .trim_matches('"')
        .parse::<i64>()
        .map(Some)
        .map_err(|_| {
            ApiError::with_message(
                ErrorCode::InvalidVersion,
                "If-Match 는 정수 버전이어야 합니다.",
            )
        })
}

/// Turn an UPDATE's row count into a result.
///
/// `exists` distinguishes the two ways zero rows can happen: the row is gone (404) or
/// someone else changed it first (409).
pub fn ensure_updated(rows_affected: u64, exists: bool) -> ApiResult<()> {
    if rows_affected == 1 {
        return Ok(());
    }
    if exists {
        Err(ApiError::new(ErrorCode::VersionConflict))
    } else {
        Err(ApiError::not_found())
    }
}

/// `ETag` value for a resource at a given version.
pub fn etag(version: i64) -> String {
    format!("\"{version}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(if_match: Option<&str>) -> HeaderMap {
        let mut map = HeaderMap::new();
        if let Some(value) = if_match {
            map.insert(
                IF_MATCH,
                HeaderValue::from_str(value).expect("valid header"),
            );
        }
        map
    }

    #[test]
    fn quoted_weak_and_bare_versions_all_parse() {
        assert_eq!(
            expected_version(&headers(Some("\"7\"")), None).expect("parses"),
            Some(7)
        );
        assert_eq!(
            expected_version(&headers(Some("W/\"7\"")), None).expect("parses"),
            Some(7)
        );
        assert_eq!(
            expected_version(&headers(Some("7")), None).expect("parses"),
            Some(7)
        );
    }

    #[test]
    fn no_version_anywhere_means_no_check() {
        assert_eq!(
            expected_version(&headers(None), None).expect("parses"),
            None
        );
        assert_eq!(
            expected_version(&headers(Some("*")), None).expect("parses"),
            None,
            "If-Match: * opts out"
        );
    }

    #[test]
    fn a_body_version_is_used_when_the_header_is_absent() {
        assert_eq!(
            expected_version(&headers(None), Some(3)).expect("parses"),
            Some(3)
        );
    }

    #[test]
    fn contradictory_versions_are_rejected_rather_than_guessed() {
        let error = expected_version(&headers(Some("\"7\"")), Some(3)).expect_err("must reject");
        assert_eq!(error.code, ErrorCode::InvalidVersion);
        assert_eq!(error.status().as_u16(), 400);
    }

    #[test]
    fn a_non_numeric_if_match_is_a_client_error() {
        let error = expected_version(&headers(Some("\"abc\"")), None).expect_err("must reject");
        assert_eq!(error.code, ErrorCode::InvalidVersion);
    }

    #[test]
    fn zero_rows_distinguishes_a_lost_race_from_a_missing_row() {
        ensure_updated(1, true).expect("one row is success");

        assert_eq!(
            ensure_updated(0, true).expect_err("lost race").code,
            ErrorCode::VersionConflict
        );
        assert_eq!(
            ensure_updated(0, false).expect_err("gone").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn etags_are_quoted() {
        assert_eq!(etag(7), "\"7\"");
    }
}
