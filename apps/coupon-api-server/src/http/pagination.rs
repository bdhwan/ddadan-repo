//! Cursor pagination shared by every list endpoint (§11.1): default 20, maximum 100.
//!
//! The cursor is an opaque base64url string over `(created_at, id)`. Keyset paging on
//! that pair is stable while rows are inserted, which offset paging is not.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ErrorCode};

pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 100;

/// Keyset position. `id` breaks ties between rows sharing a timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    #[serde(rename = "t")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "i")]
    pub id: Uuid,
}

impl Cursor {
    pub fn new(created_at: DateTime<Utc>, id: Uuid) -> Self {
        Self { created_at, id }
    }

    /// Encode to the opaque token handed to clients.
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("cursor is always serialisable");
        URL_SAFE_NO_PAD.encode(json)
    }

    /// Decode a client-supplied token. Any malformed input is a 400, never a panic.
    pub fn decode(raw: &str) -> Result<Self, ApiError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(raw.as_bytes())
            .map_err(|error| ApiError::new(ErrorCode::InvalidCursor).internal(error.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| ApiError::new(ErrorCode::InvalidCursor).internal(error.to_string()))
    }
}

/// `?limit=&cursor=` on any list endpoint.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub struct PageQuery {
    /// 1–100. Defaults to 20.
    pub limit: Option<u32>,
    /// `next_cursor` from the previous page.
    pub cursor: Option<String>,
}

impl PageQuery {
    /// Clamp into range. Values outside 1..=100 are corrected rather than rejected, so a
    /// client that asks for 1000 gets the maximum page instead of an error.
    pub fn limit(&self) -> u32 {
        self.limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    /// Number of rows to ask the database for: one extra reveals whether more exist.
    pub fn fetch_limit(&self) -> i64 {
        i64::from(self.limit()) + 1
    }

    pub fn cursor(&self) -> Result<Option<Cursor>, ApiError> {
        self.cursor.as_deref().map(Cursor::decode).transpose()
    }
}

/// One page of results.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Pass back as `?cursor=` to fetch the next page. `null` on the last page.
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T> Page<T> {
    /// Build a page from an over-fetched row set.
    ///
    /// `rows` must hold up to `limit + 1` items; the extra one is dropped and only used
    /// to decide `has_more`. `cursor_of` reads the keyset position off the last item
    /// that survives.
    pub fn from_rows(mut rows: Vec<T>, limit: u32, cursor_of: impl Fn(&T) -> Cursor) -> Self {
        let has_more = rows.len() > limit as usize;
        rows.truncate(limit as usize);

        let next_cursor = if has_more {
            rows.last().map(|row| cursor_of(row).encode())
        } else {
            None
        };

        Self {
            items: rows,
            next_cursor,
            has_more,
        }
    }

    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> Cursor {
        Cursor::new(
            DateTime::parse_from_rfc3339("2026-08-10T06:00:00Z")
                .expect("valid timestamp")
                .with_timezone(&Utc),
            Uuid::parse_str("11111111-2222-3333-4444-555555555555").expect("valid uuid"),
        )
    }

    #[test]
    fn cursor_round_trips_through_its_encoding() {
        let original = cursor();
        let decoded = Cursor::decode(&original.encode()).expect("decodes");
        assert_eq!(decoded, original);
    }

    #[test]
    fn cursor_encoding_is_url_safe_and_unpadded() {
        let encoded = cursor().encode();
        assert!(!encoded.contains('='), "{encoded} must not be padded");
        assert!(
            !encoded.contains('+') && !encoded.contains('/'),
            "{encoded} must be url-safe"
        );
    }

    #[test]
    fn malformed_cursors_are_client_errors() {
        for raw in ["not-base64!!", "", &URL_SAFE_NO_PAD.encode(b"{\"nope\":1}")] {
            let error = Cursor::decode(raw).expect_err("must reject");
            assert_eq!(error.code, ErrorCode::InvalidCursor);
            assert_eq!(error.status().as_u16(), 400);
        }
    }

    #[test]
    fn limit_defaults_to_twenty_and_clamps_to_one_hundred() {
        assert_eq!(PageQuery::default().limit(), DEFAULT_PAGE_SIZE);
        assert_eq!(
            PageQuery {
                limit: Some(0),
                cursor: None
            }
            .limit(),
            1,
            "zero is clamped up, not treated as unbounded"
        );
        assert_eq!(
            PageQuery {
                limit: Some(50),
                cursor: None
            }
            .limit(),
            50
        );
        assert_eq!(
            PageQuery {
                limit: Some(1000),
                cursor: None
            }
            .limit(),
            MAX_PAGE_SIZE
        );
        assert_eq!(
            PageQuery {
                limit: Some(20),
                cursor: None
            }
            .fetch_limit(),
            21
        );
    }

    #[test]
    fn a_full_over_fetch_reports_more_and_emits_a_cursor() {
        let rows: Vec<u32> = (0..4).collect();
        let page = Page::from_rows(rows, 3, |row| {
            Cursor::new(cursor().created_at, Uuid::from_u128(u128::from(*row)))
        });

        assert_eq!(page.items, vec![0, 1, 2]);
        assert!(page.has_more);
        let next = Cursor::decode(page.next_cursor.as_deref().expect("cursor")).expect("decodes");
        assert_eq!(
            next.id,
            Uuid::from_u128(2),
            "cursor points at the last returned row"
        );
    }

    #[test]
    fn a_short_page_is_the_last_page() {
        let page = Page::from_rows(vec![0u32, 1], 3, |row| {
            Cursor::new(cursor().created_at, Uuid::from_u128(u128::from(*row)))
        });

        assert!(!page.has_more);
        assert_eq!(page.next_cursor, None);
    }
}
