//! 알림 템플릿 버전 관리와 렌더링 (§15.2).
//!
//! Templates are versioned rows, never files: `(code, version_no, locale, channel)` is
//! unique and a partial unique index allows exactly one `active` row per
//! `(code, locale, channel)`. Publishing a new version therefore *adds* a row and flips a
//! flag — the old version stays readable, which is what §15.2's 과거 발송 재현 needs. A
//! delivery records the template id it used, so reproducing a send from six months ago is
//! a join rather than an archaeology exercise.
//!
//! Rendering is deliberately not a general template engine. §15.2 says the payload carries
//! 허용 변수만, escaped — so the renderer knows the allow-list from the template's own
//! `variable_schema`, substitutes nothing outside it, and escapes every value it does
//! substitute. A name that is not on the list renders empty rather than passing through,
//! because a placeholder that survives into a provider payload is how user text becomes
//! markup somewhere downstream.

use std::collections::BTreeMap;

use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiResult;
use crate::notifications::NotificationChannel;
use crate::notifications::policy::NotificationPurpose;

/// The default locale. §23.1 fixes ko-KR for the MVP.
pub const DEFAULT_LOCALE: &str = "ko-KR";

/// One immutable template version.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationTemplate {
    pub id: Uuid,
    pub code: String,
    pub version_no: i32,
    pub locale: String,
    #[serde(skip)]
    pub channel: NotificationChannel,
    pub purpose: NotificationPurpose,
    /// The provider's own template identifier. 알림톡 cannot be sent without one (§15.1).
    pub provider_template_id: Option<String>,
    pub provider_approval_status: Option<String>,
    pub subject_template: Option<String>,
    pub body_template: String,
    /// The allow-list. Anything not named here is not substitutable (§15.2).
    pub variables: Vec<String>,
    pub priority: String,
    pub active: bool,
}

impl NotificationTemplate {
    /// Whether the provider will accept this template today.
    ///
    /// A 알림톡 template that has not been approved is not merely unlikely to work — sending
    /// against it is the misuse §15.1 rules out, so it is treated as unavailable rather
    /// than tried and failed.
    pub fn is_sendable(&self) -> bool {
        if !self.active {
            return false;
        }

        match self.channel {
            NotificationChannel::KakaoAlimtalk => {
                self.provider_template_id.is_some()
                    && self.provider_approval_status.as_deref() == Some("APPROVED")
            }
            _ => true,
        }
    }
}

/// What actually goes to the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMessage {
    pub subject: String,
    pub body: String,
    /// The escaped values that were substituted, recorded on the delivery so the send is
    /// reproducible without re-deriving anything from live domain data.
    pub variables: BTreeMap<String, String>,
}

/// Substitute the allow-listed variables and escape every one of them.
///
/// `supplied` may hold more than the template asks for — a caller assembling variables for
/// three channels at once should not have to slice them per channel. Only what the
/// template names is used.
pub fn render(template: &NotificationTemplate, supplied: &BTreeMap<String, String>) -> RenderedMessage {
    let allowed: BTreeMap<String, String> = template
        .variables
        .iter()
        .map(|name| {
            let value = supplied
                .get(name)
                .map(|raw| escape(raw))
                .unwrap_or_default();
            (name.clone(), value)
        })
        .collect();

    RenderedMessage {
        subject: substitute(template.subject_template.as_deref().unwrap_or_default(), &allowed),
        body: substitute(&template.body_template, &allowed),
        variables: allowed,
    }
}

/// Replace `{{name}}` with the already-escaped value, and erase any placeholder the
/// template's own allow-list does not cover.
fn substitute(source: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];

        let Some(end) = after.find("}}") else {
            // An unterminated placeholder is a broken template, not a licence to emit the
            // rest verbatim: everything from here on is dropped.
            return out;
        };

        let name = after[..end].trim();
        // Unknown names render empty. Leaving `{{name}}` in place would ship a literal
        // brace pair to a customer; passing the raw name through would be worse.
        out.push_str(values.get(name).map(String::as_str).unwrap_or(""));
        rest = &after[end + 2..];
    }

    out.push_str(rest);
    out
}

/// Escape one substituted value.
///
/// §15.2 forbids user-supplied HTML in a send payload. Store names and campaign titles are
/// user input, so every value is escaped for markup regardless of the channel: the in-app
/// client renders into a DOM, web push renders into a notification body, and 알림톡 rejects
/// payloads it considers malformed. Control characters go too — they are invisible in
/// review and meaningful to some providers' parsers.
pub fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for character in raw.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            // A brace pair inside a *value* must never become a placeholder on a second
            // pass; braces have no meaning in a message body, so they are stripped.
            '{' | '}' => {}
            '\n' | '\t' => out.push(' '),
            other if other.is_control() => {}
            other => out.push(other),
        }
    }
    out
}

/// Reads `notification_templates`. Its own struct so callers cannot reach the table
/// directly (§10.2).
pub struct TemplateRepository;

impl Default for TemplateRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateRepository {
    pub fn new() -> Self {
        Self
    }

    /// The active version for a code on a channel, or `None` when the phase has no
    /// template for that combination — an expiry warning has no 알림톡 template until one
    /// is approved, and that absence is a suppression rather than an error (§15.4).
    pub async fn active<'e, E>(
        &self,
        executor: E,
        code: &str,
        channel: NotificationChannel,
        locale: &str,
    ) -> ApiResult<Option<NotificationTemplate>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, code, version_no, locale, channel::text AS "channel!", purpose,
                   provider_template_id, provider_approval_status, subject_template,
                   body_template, variable_schema, priority, active
            FROM coupon.notification_templates
            WHERE code = $1
              AND channel = $2::text::coupon.notification_channel
              AND locale = $3
              AND active
              AND retired_at IS NULL
            "#,
            code,
            channel.as_db(),
            locale,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|row| NotificationTemplate {
            id: row.id,
            code: row.code,
            version_no: row.version_no,
            locale: row.locale,
            channel: NotificationChannel::from_db(&row.channel).unwrap_or(channel),
            purpose: NotificationPurpose::from_db(&row.purpose)
                .unwrap_or(NotificationPurpose::Informational),
            provider_template_id: row.provider_template_id,
            provider_approval_status: row.provider_approval_status,
            subject_template: row.subject_template,
            body_template: row.body_template,
            variables: read_variable_schema(&row.variable_schema),
            priority: row.priority,
            active: row.active,
        }))
    }

    /// A specific version, by id. This is what reproduces a past send (§15.2).
    pub async fn by_id<'e, E>(&self, executor: E, id: Uuid) -> ApiResult<Option<NotificationTemplate>>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, code, version_no, locale, channel::text AS "channel!", purpose,
                   provider_template_id, provider_approval_status, subject_template,
                   body_template, variable_schema, priority, active
            FROM coupon.notification_templates
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|row| NotificationTemplate {
            id: row.id,
            code: row.code,
            version_no: row.version_no,
            locale: row.locale,
            channel: NotificationChannel::from_db(&row.channel)
                .unwrap_or(NotificationChannel::InApp),
            purpose: NotificationPurpose::from_db(&row.purpose)
                .unwrap_or(NotificationPurpose::Informational),
            provider_template_id: row.provider_template_id,
            provider_approval_status: row.provider_approval_status,
            subject_template: row.subject_template,
            body_template: row.body_template,
            variables: read_variable_schema(&row.variable_schema),
            priority: row.priority,
            active: row.active,
        }))
    }

    /// Every version of a code, newest first — the administrative view of what has been
    /// sent under a name over time.
    pub async fn history(&self, pool: &PgPool, code: &str) -> ApiResult<Vec<NotificationTemplate>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, code, version_no, locale, channel::text AS "channel!", purpose,
                   provider_template_id, provider_approval_status, subject_template,
                   body_template, variable_schema, priority, active
            FROM coupon.notification_templates
            WHERE code = $1
            ORDER BY version_no DESC, channel
            "#,
            code,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| NotificationTemplate {
                id: row.id,
                code: row.code,
                version_no: row.version_no,
                locale: row.locale,
                channel: NotificationChannel::from_db(&row.channel)
                    .unwrap_or(NotificationChannel::InApp),
                purpose: NotificationPurpose::from_db(&row.purpose)
                    .unwrap_or(NotificationPurpose::Informational),
                provider_template_id: row.provider_template_id,
                provider_approval_status: row.provider_approval_status,
                subject_template: row.subject_template,
                body_template: row.body_template,
                variables: read_variable_schema(&row.variable_schema),
                priority: row.priority,
                active: row.active,
            })
            .collect())
    }
}

/// `variable_schema` is a JSON array of names. Anything else is treated as an empty
/// allow-list, which renders a template with no substitutions rather than failing a send.
fn read_variable_schema(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(body: &str, variables: &[&str]) -> NotificationTemplate {
        NotificationTemplate {
            id: Uuid::from_u128(1),
            code: "STAMP_EARNED".to_owned(),
            version_no: 1,
            locale: DEFAULT_LOCALE.to_owned(),
            channel: NotificationChannel::InApp,
            purpose: NotificationPurpose::Transactional,
            provider_template_id: None,
            provider_approval_status: Some("NOT_REQUIRED".to_owned()),
            subject_template: Some("{{store_name}}".to_owned()),
            body_template: body.to_owned(),
            variables: variables.iter().map(|name| (*name).to_owned()).collect(),
            priority: "NORMAL".to_owned(),
            active: true,
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn allow_listed_variables_are_substituted() {
        let rendered = render(
            &template("{{store_name}}에서 도장 {{quantity}}개", &["store_name", "quantity"]),
            &vars(&[("store_name", "브로트베르크"), ("quantity", "2")]),
        );

        assert_eq!(rendered.body, "브로트베르크에서 도장 2개");
        assert_eq!(rendered.subject, "브로트베르크");
    }

    #[test]
    fn user_supplied_markup_never_survives_into_the_payload() {
        // §15.2: 발송 payload 에 사용자 입력 HTML 을 넣지 않는다. A store name is user input.
        let rendered = render(
            &template("{{store_name}} 알림", &["store_name"]),
            &vars(&[("store_name", "<script>alert('x')</script>")]),
        );

        assert!(!rendered.body.contains('<'), "{}", rendered.body);
        assert!(!rendered.body.contains('>'), "{}", rendered.body);
        assert!(rendered.body.contains("&lt;script&gt;"), "{}", rendered.body);
    }

    #[test]
    fn a_variable_the_template_does_not_name_is_not_substitutable() {
        let rendered = render(
            &template("{{store_name}} / {{secret}}", &["store_name"]),
            &vars(&[("store_name", "가게"), ("secret", "내부 메모")]),
        );

        assert_eq!(rendered.body, "가게 / ");
        assert!(!rendered.variables.contains_key("secret"));
    }

    #[test]
    fn a_value_cannot_smuggle_a_second_placeholder() {
        // Substitution is single-pass, and braces are stripped from values, so a name that
        // arrives as data cannot be re-read as a template on any later pass.
        let rendered = render(
            &template("{{store_name}}", &["store_name", "secret"]),
            &vars(&[("store_name", "{{secret}}"), ("secret", "내부 메모")]),
        );

        assert_eq!(rendered.body, "secret");
    }

    #[test]
    fn a_missing_value_renders_empty_rather_than_leaking_the_placeholder() {
        let rendered = render(&template("남은 {{remaining}}개", &["remaining"]), &vars(&[]));
        assert_eq!(rendered.body, "남은 개");
    }

    #[test]
    fn an_unterminated_placeholder_truncates_instead_of_leaking() {
        let rendered = render(&template("안녕 {{store_name", &["store_name"]), &vars(&[]));
        assert_eq!(rendered.body, "안녕 ");
    }

    #[test]
    fn an_unapproved_alimtalk_template_is_not_sendable() {
        // §15.1: 승인된 정보성 템플릿만 알림톡으로 나간다.
        let mut alimtalk = template("본문", &[]);
        alimtalk.channel = NotificationChannel::KakaoAlimtalk;
        alimtalk.provider_template_id = None;
        alimtalk.provider_approval_status = Some("PENDING".to_owned());
        assert!(!alimtalk.is_sendable());

        alimtalk.provider_template_id = Some("TPL-1".to_owned());
        assert!(!alimtalk.is_sendable(), "an id without approval is not enough");

        alimtalk.provider_approval_status = Some("APPROVED".to_owned());
        assert!(alimtalk.is_sendable());
    }

    #[test]
    fn an_inactive_template_is_never_sendable() {
        let mut inactive = template("본문", &[]);
        inactive.active = false;
        assert!(!inactive.is_sendable());
    }

    #[test]
    fn a_malformed_variable_schema_yields_an_empty_allow_list() {
        assert!(read_variable_schema(&serde_json::json!({ "store_name": "x" })).is_empty());
        assert_eq!(
            read_variable_schema(&serde_json::json!(["a", 2, "b"])),
            vec!["a".to_owned(), "b".to_owned()]
        );
    }
}
