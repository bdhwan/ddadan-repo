//! Structured logging (§18.3) and log redaction (§16.3).
//!
//! Redaction is an allowlist: a header is logged only if it is named here. Anything new
//! — a provider callback header, a debug header someone adds next quarter — is dropped
//! by default rather than leaked by default.

use std::collections::BTreeMap;

use http::HeaderMap;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{Config, LogFormat};

/// Headers safe to log verbatim. Deliberately short.
///
/// `authorization`, `cookie`, `x-dev-firebase-uid` and anything QR- or provider-shaped
/// are absent on purpose: they are credentials.
const HEADER_ALLOWLIST: &[&str] = &[
    "accept",
    "accept-language",
    "content-length",
    "content-type",
    "idempotency-key",
    "if-match",
    "origin",
    "referer",
    "user-agent",
    "x-request-id",
];

/// Install the global subscriber. Call once, at process start.
pub fn init(config: &Config) {
    let filter = EnvFilter::try_from_env("COUPON_LOG_FILTER")
        .unwrap_or_else(|_| EnvFilter::new(config.log_filter.clone()));

    let registry = tracing_subscriber::registry().with(filter);

    match config.log_format {
        LogFormat::Json => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_current_span(true)
                    .with_span_list(false)
                    .with_target(true),
            )
            .init(),
        LogFormat::Pretty => registry
            .with(tracing_subscriber::fmt::layer().with_target(true))
            .init(),
    }
}

/// Keep only allowlisted headers, so a log line can never carry a credential.
pub fn redact_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str().to_ascii_lowercase();
            if !HEADER_ALLOWLIST.contains(&name.as_str()) {
                return None;
            }
            let value = value.to_str().ok()?;
            Some((name, redact_value(value)))
        })
        .collect()
}

/// Even allowlisted values are length-capped: a `referer` can carry a long query string
/// that nobody vetted.
fn redact_value(value: &str) -> String {
    const MAX: usize = 200;
    if value.len() <= MAX {
        return value.to_owned();
    }
    let mut truncated: String = value.chars().take(MAX).collect();
    truncated.push('…');
    truncated
}

/// Coarse client classification for consent records (§9.4). Deliberately not a
/// fingerprint: it stores a bucket, not the user agent string.
pub fn classify_user_agent(user_agent: Option<&str>) -> String {
    let Some(user_agent) = user_agent else {
        return "UNKNOWN".to_owned();
    };
    let lowered = user_agent.to_ascii_lowercase();

    if lowered.contains("kakaotalk") {
        "KAKAO_IN_APP".to_owned()
    } else if lowered.contains("android") {
        "MOBILE_ANDROID".to_owned()
    } else if lowered.contains("iphone") || lowered.contains("ipad") || lowered.contains("ios") {
        "MOBILE_IOS".to_owned()
    } else if lowered.contains("mobile") {
        "MOBILE_OTHER".to_owned()
    } else if lowered.contains("mozilla")
        || lowered.contains("chrome")
        || lowered.contains("safari")
    {
        "DESKTOP_WEB".to_owned()
    } else {
        "OTHER".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderName, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("valid header name"),
                HeaderValue::from_str(value).expect("valid header value"),
            );
        }
        map
    }

    #[test]
    fn credentials_never_survive_redaction() {
        let redacted = redact_headers(&headers(&[
            ("authorization", "Bearer super-secret-token"),
            ("cookie", "session=abc"),
            ("x-dev-firebase-uid", "dev-uid"),
            ("x-qr-nonce", "nonce"),
            ("content-type", "application/json"),
        ]));

        assert_eq!(
            redacted.get("content-type").map(String::as_str),
            Some("application/json")
        );
        for dropped in [
            "authorization",
            "cookie",
            "x-dev-firebase-uid",
            "x-qr-nonce",
        ] {
            assert!(!redacted.contains_key(dropped), "{dropped} must be dropped");
        }
    }

    #[test]
    fn allowlisted_values_are_length_capped() {
        let long = "https://example.com/?q=".to_owned() + &"a".repeat(500);
        let redacted = redact_headers(&headers(&[("referer", &long)]));

        let value = redacted.get("referer").expect("referer is allowlisted");
        assert!(value.chars().count() <= 201, "value must be truncated");
        assert!(value.ends_with('…'));
    }

    #[test]
    fn user_agents_collapse_into_coarse_buckets() {
        assert_eq!(classify_user_agent(None), "UNKNOWN");
        assert_eq!(
            classify_user_agent(Some("Mozilla/5.0 (Linux; Android 14) Mobile Safari")),
            "MOBILE_ANDROID"
        );
        assert_eq!(
            classify_user_agent(Some("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0) Safari")),
            "MOBILE_IOS"
        );
        assert_eq!(
            classify_user_agent(Some("Mozilla/5.0 (Windows NT 10.0) Chrome/130")),
            "DESKTOP_WEB"
        );
        assert_eq!(classify_user_agent(Some("curl/8.5.0")), "OTHER");
    }
}
