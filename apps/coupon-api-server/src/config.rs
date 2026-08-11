//! Process configuration.
//!
//! Everything is read from the environment with the `COUPON_` prefix. Secrets are never
//! defaulted: a missing secret in production is a boot failure, not a silent fallback.

use std::net::SocketAddr;
use std::time::Duration;

use figment::Figment;
use figment::providers::Env;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Development,
    Test,
    Staging,
    Production,
}

impl Environment {
    pub fn is_production(self) -> bool {
        matches!(self, Environment::Production)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Environment::Development => "development",
            Environment::Test => "test",
            Environment::Staging => "staging",
            Environment::Production => "production",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_env")]
    pub env: Environment,

    #[serde(default = "default_bind_addr")]
    pub bind_addr: SocketAddr,

    pub database_url: String,

    #[serde(default = "default_db_max_connections")]
    pub database_max_connections: u32,

    #[serde(default = "default_db_connect_timeout_secs")]
    pub database_connect_timeout_secs: u64,

    #[serde(default)]
    pub redis_url: Option<String>,

    /// Firebase project id. Doubles as the ID token `aud` and the `iss` suffix (§9.3).
    #[serde(default)]
    pub firebase_project_id: Option<String>,

    /// Extra audiences accepted alongside `firebase_project_id`, e.g. a separate admin
    /// tenant. Comma separated.
    #[serde(default)]
    pub firebase_extra_audiences: CommaList,

    /// Exact browser origins allowed to call state-changing endpoints (§16.3).
    #[serde(default)]
    pub allowed_origins: CommaList,

    /// Accept `X-Dev-Firebase-Uid` instead of a real ID token. Refused in production.
    #[serde(default, deserialize_with = "deserialize_flag")]
    pub auth_dev_bypass: bool,

    /// Maximum `auth_time` age accepted by high-risk endpoints (§9.3).
    #[serde(default = "default_recent_auth_secs")]
    pub recent_auth_max_age_secs: u64,

    /// How long a completed idempotency record stays replayable.
    #[serde(default = "default_idempotency_ttl_hours")]
    pub idempotency_ttl_hours: i64,

    /// Base64 32-byte AES-256-GCM key for column envelope encryption (§16.5).
    #[serde(default)]
    pub data_encryption_key: Option<String>,

    /// Secret for keyed lookup hashes over searchable personal data (§16.5), and for the
    /// consent IP hash (§9.4).
    #[serde(default)]
    pub lookup_hash_secret: Option<String>,

    #[serde(default = "default_log_format")]
    pub log_format: LogFormat,

    #[serde(default = "default_log_filter")]
    pub log_filter: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,
    Pretty,
}

/// `A,B,C` in the environment, `Vec<String>` in code. Figment's env provider hands us a
/// plain string, so the split happens here rather than at every call site.
#[derive(Debug, Clone, Default)]
pub struct CommaList(pub Vec<String>);

impl CommaList {
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for CommaList {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(CommaList(
            raw.split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned)
                .collect(),
        ))
    }
}

/// Accept the shapes people actually write in a shell for a boolean flag.
///
/// `COUPON_AUTH_DEV_BYPASS=1` is the documented spelling, and figment hands that to us as
/// an integer, so a plain `bool` field would reject it.
fn deserialize_flag<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    struct FlagVisitor;

    impl serde::de::Visitor<'_> for FlagVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a boolean, 0/1, or true/false/yes/no/on/off")
        }

        fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<bool, E> {
            Ok(value)
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<bool, E> {
            Ok(value != 0)
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<bool, E> {
            Ok(value != 0)
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<bool, E> {
            match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "y" | "on" => Ok(true),
                "0" | "false" | "no" | "n" | "off" | "" => Ok(false),
                // Anything ambiguous is refused rather than guessed at: silently reading
                // a typo as `false` would disable a security control without a word.
                other => Err(E::custom(format!("{other:?} is not a boolean"))),
            }
        }
    }

    deserializer.deserialize_any(FlagVisitor)
}

fn default_env() -> Environment {
    Environment::Development
}

fn default_bind_addr() -> SocketAddr {
    "0.0.0.0:7810".parse().expect("valid default bind address")
}

fn default_db_max_connections() -> u32 {
    16
}

fn default_db_connect_timeout_secs() -> u64 {
    5
}

fn default_recent_auth_secs() -> u64 {
    600
}

fn default_idempotency_ttl_hours() -> i64 {
    24
}

fn default_log_format() -> LogFormat {
    LogFormat::Json
}

fn default_log_filter() -> String {
    "info,sqlx=warn,tower_http=info".to_owned()
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read configuration: {0}")]
    Read(#[from] Box<figment::Error>),
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

impl Config {
    /// Read `COUPON_*` from the process environment and validate it.
    pub fn from_env() -> Result<Self, ConfigError> {
        let config: Config = Figment::new()
            .merge(Env::prefixed("COUPON_"))
            .extract()
            .map_err(Box::new)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        // The single most dangerous misconfiguration: an authentication bypass reachable
        // from the internet. Refuse to boot rather than serve one request.
        if self.env.is_production() && self.auth_dev_bypass {
            return Err(ConfigError::Invalid(
                "COUPON_AUTH_DEV_BYPASS must not be enabled when COUPON_ENV=production".to_owned(),
            ));
        }

        if self.database_url.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "COUPON_DATABASE_URL must not be empty".to_owned(),
            ));
        }

        if !self.auth_dev_bypass && self.firebase_project_id.is_none() {
            return Err(ConfigError::Invalid(
                "COUPON_FIREBASE_PROJECT_ID is required unless COUPON_AUTH_DEV_BYPASS=1".to_owned(),
            ));
        }

        if self.env.is_production() {
            if self.allowed_origins.is_empty() {
                return Err(ConfigError::Invalid(
                    "COUPON_ALLOWED_ORIGINS is required in production".to_owned(),
                ));
            }
            if self.data_encryption_key.is_none() {
                return Err(ConfigError::Invalid(
                    "COUPON_DATA_ENCRYPTION_KEY is required in production".to_owned(),
                ));
            }
            if self.lookup_hash_secret.is_none() {
                return Err(ConfigError::Invalid(
                    "COUPON_LOOKUP_HASH_SECRET is required in production".to_owned(),
                ));
            }
            if self.redis_url.is_none() {
                return Err(ConfigError::Invalid(
                    "COUPON_REDIS_URL is required in production".to_owned(),
                ));
            }
        }

        Ok(())
    }

    pub fn database_connect_timeout(&self) -> Duration {
        Duration::from_secs(self.database_connect_timeout_secs)
    }

    pub fn recent_auth_max_age(&self) -> Duration {
        Duration::from_secs(self.recent_auth_max_age_secs)
    }

    /// Expected `iss` for Firebase ID tokens.
    pub fn firebase_issuer(&self) -> Option<String> {
        self.firebase_project_id
            .as_ref()
            .map(|project| format!("https://securetoken.google.com/{project}"))
    }

    /// Every accepted `aud`: the project id plus any explicitly allowed extra tenant.
    pub fn firebase_audiences(&self) -> Vec<String> {
        let mut audiences = Vec::new();
        if let Some(project) = &self.firebase_project_id {
            audiences.push(project.clone());
        }
        audiences.extend(self.firebase_extra_audiences.0.iter().cloned());
        audiences
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> Config {
        Config {
            env: Environment::Development,
            bind_addr: default_bind_addr(),
            database_url: "postgres://localhost/coupon".to_owned(),
            database_max_connections: 4,
            database_connect_timeout_secs: 5,
            redis_url: None,
            firebase_project_id: Some("ddadan-test".to_owned()),
            firebase_extra_audiences: CommaList::default(),
            allowed_origins: CommaList::default(),
            auth_dev_bypass: false,
            recent_auth_max_age_secs: 600,
            idempotency_ttl_hours: 24,
            data_encryption_key: None,
            lookup_hash_secret: None,
            log_format: LogFormat::Json,
            log_filter: default_log_filter(),
        }
    }

    #[test]
    fn dev_bypass_is_refused_in_production() {
        let mut config = base_config();
        config.env = Environment::Production;
        config.auth_dev_bypass = true;

        let error = config
            .validate()
            .expect_err("production must refuse bypass");
        assert!(error.to_string().contains("COUPON_AUTH_DEV_BYPASS"));
    }

    #[test]
    fn dev_bypass_is_allowed_outside_production() {
        let mut config = base_config();
        config.auth_dev_bypass = true;
        config.firebase_project_id = None;

        config.validate().expect("development may bypass auth");
    }

    #[test]
    fn firebase_issuer_and_audiences_follow_the_project_id() {
        let mut config = base_config();
        config.firebase_extra_audiences = CommaList(vec!["ddadan-admin".to_owned()]);

        assert_eq!(
            config.firebase_issuer().as_deref(),
            Some("https://securetoken.google.com/ddadan-test")
        );
        assert_eq!(
            config.firebase_audiences(),
            vec!["ddadan-test", "ddadan-admin"]
        );
    }

    #[test]
    fn shell_style_booleans_are_accepted() {
        #[derive(Deserialize)]
        struct Flag {
            #[serde(deserialize_with = "deserialize_flag")]
            value: bool,
        }

        let parse = |raw: serde_json::Value| {
            serde_json::from_value::<Flag>(serde_json::json!({ "value": raw })).map(|f| f.value)
        };

        for truthy in [
            serde_json::json!(1),
            serde_json::json!("1"),
            serde_json::json!("true"),
            serde_json::json!("YES"),
            serde_json::json!("on"),
            serde_json::json!(true),
        ] {
            assert!(parse(truthy.clone()).expect("parses"), "{truthy} is truthy");
        }
        for falsy in [
            serde_json::json!(0),
            serde_json::json!("0"),
            serde_json::json!("false"),
            serde_json::json!("off"),
            serde_json::json!(""),
            serde_json::json!(false),
        ] {
            assert!(!parse(falsy.clone()).expect("parses"), "{falsy} is falsy");
        }
    }

    #[test]
    fn an_ambiguous_flag_is_refused_rather_than_read_as_false() {
        #[derive(Debug, Deserialize)]
        struct Flag {
            #[serde(deserialize_with = "deserialize_flag")]
            #[allow(dead_code)]
            value: bool,
        }

        serde_json::from_value::<Flag>(serde_json::json!({ "value": "ture" }))
            .expect_err("a typo must not silently disable the flag");
    }

    #[test]
    fn comma_list_trims_and_drops_blanks() {
        let list: CommaList = serde_json::from_str("\" a , ,b \"").expect("parses");
        assert_eq!(list.as_slice(), ["a", "b"]);
    }
}
