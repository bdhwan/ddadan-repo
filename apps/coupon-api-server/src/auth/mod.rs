//! Identity verification and the per-request authorisation context (§9.3).
//!
//! Two rules drive this module:
//!
//! 1. A token proves *who signed in*. It never proves account status or role — those are
//!    read from PostgreSQL on every request.
//! 2. The development bypass is a boot-time decision, not a request-time one, so a
//!    production process physically cannot honour it.

pub mod custom_token;
pub mod extractors;
pub mod firebase;
pub mod kakao;

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::users::{AccountRole, UserStatus};

pub use firebase::{FirebaseVerifier, VerifiedToken};

/// Header that stands in for a Firebase ID token when the bypass is enabled.
pub const DEV_UID_HEADER: &str = "x-dev-firebase-uid";
/// Optional companion to [`DEV_UID_HEADER`]: seconds of age to give the fake `auth_time`.
pub const DEV_AUTH_AGE_HEADER: &str = "x-dev-auth-age-secs";

/// The account as PostgreSQL sees it right now.
#[derive(Debug, Clone)]
pub struct Account {
    pub user_id: Uuid,
    pub firebase_uid: String,
    pub consumer_key: Uuid,
    pub display_name: String,
    pub status: UserStatus,
    pub version: i64,
    pub roles: Vec<AccountRole>,
}

impl Account {
    pub fn has_role(&self, role: AccountRole) -> bool {
        self.roles.contains(&role)
    }

    /// Reject accounts that exist but may not act. Kept next to the loader so no caller
    /// can forget it.
    pub fn ensure_usable(&self) -> ApiResult<()> {
        match self.status {
            UserStatus::Active | UserStatus::PendingVerification => Ok(()),
            UserStatus::Suspended => Err(ApiError::new(ErrorCode::AccountSuspended)),
            UserStatus::WithdrawalPending | UserStatus::Withdrawn => {
                Err(ApiError::new(ErrorCode::AccountWithdrawn))
            }
        }
    }
}

/// What the auth middleware attaches to every authenticated request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub token: VerifiedToken,
    /// `None` before `POST /users/bootstrap` has run for this Firebase user.
    pub account: Option<Account>,
}

impl AuthContext {
    /// The account, or the error the caller should return if there is none.
    pub fn require_account(&self) -> ApiResult<&Account> {
        let account = self
            .account
            .as_ref()
            .ok_or_else(|| ApiError::new(ErrorCode::UserNotFound))?;
        account.ensure_usable()?;
        Ok(account)
    }

    /// Whether the sign-in is recent enough for a high-risk operation (§9.3).
    pub fn authenticated_within(&self, max_age: std::time::Duration) -> bool {
        let max_age = Duration::from_std(max_age).unwrap_or_else(|_| Duration::seconds(600));
        Utc::now().signed_duration_since(self.token.auth_time) <= max_age
    }
}

pub struct AuthService {
    config: Arc<Config>,
    firebase: FirebaseVerifier,
}

impl AuthService {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            firebase: FirebaseVerifier::new(config.clone()),
            config,
        }
    }

    pub fn dev_bypass_enabled(&self) -> bool {
        self.config.auth_dev_bypass
    }

    /// Verify a `Bearer` credential into a token we are willing to act on.
    pub async fn verify_bearer(&self, token: &str) -> ApiResult<VerifiedToken> {
        self.firebase.verify(token).await
    }

    /// Build a token for the development bypass. Only reachable when
    /// `COUPON_AUTH_DEV_BYPASS=1`, which `Config::validate` forbids in production.
    pub fn dev_token(&self, firebase_uid: &str, auth_age_secs: i64) -> ApiResult<VerifiedToken> {
        if !self.dev_bypass_enabled() {
            return Err(ApiError::unauthenticated());
        }
        if firebase_uid.trim().is_empty() {
            return Err(ApiError::with_message(
                ErrorCode::TokenInvalid,
                "X-Dev-Firebase-Uid must not be empty",
            ));
        }

        let now = Utc::now();
        Ok(VerifiedToken {
            firebase_uid: firebase_uid.to_owned(),
            email: Some(format!("{firebase_uid}@dev.invalid")),
            email_verified: true,
            display_name: None,
            sign_in_provider: "dev-bypass".to_owned(),
            auth_time: now - Duration::seconds(auth_age_secs.max(0)),
            issued_at: now,
            expires_at: now + Duration::hours(1),
        })
    }

    /// Load account state and active roles as of this instant (§9.3).
    pub async fn load_account(
        &self,
        pool: &PgPool,
        firebase_uid: &str,
    ) -> ApiResult<Option<Account>> {
        let row = sqlx::query!(
            r#"
            SELECT
                u.id,
                u.firebase_uid,
                u.consumer_key,
                u.display_name,
                u.status::text AS "status!",
                u.version,
                COALESCE(
                    ARRAY(
                        SELECT r.role::text
                        FROM coupon.user_roles r
                        WHERE r.user_id = u.id AND r.revoked_at IS NULL
                        ORDER BY r.granted_at
                    ),
                    ARRAY[]::text[]
                ) AS "roles!: Vec<String>"
            FROM coupon.users u
            WHERE u.firebase_uid = $1
            "#,
            firebase_uid
        )
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| Account {
            user_id: row.id,
            firebase_uid: row.firebase_uid,
            consumer_key: row.consumer_key,
            display_name: row.display_name,
            status: UserStatus::from_db(&row.status),
            version: row.version,
            roles: row
                .roles
                .iter()
                .map(|role| AccountRole::from_db(role))
                .collect(),
        }))
    }
}

/// Timestamps carried by a verified credential, in the form the rest of the code wants.
#[derive(Debug, Clone, Copy)]
pub struct TokenTimestamps {
    pub auth_time: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(status: UserStatus) -> Account {
        Account {
            user_id: Uuid::nil(),
            firebase_uid: "uid".to_owned(),
            consumer_key: Uuid::nil(),
            display_name: "테스터".to_owned(),
            status,
            version: 1,
            roles: vec![AccountRole::Consumer],
        }
    }

    #[test]
    fn only_live_accounts_may_act() {
        account(UserStatus::Active).ensure_usable().expect("active");
        account(UserStatus::PendingVerification)
            .ensure_usable()
            .expect("pending verification may still act");

        assert_eq!(
            account(UserStatus::Suspended)
                .ensure_usable()
                .expect_err("suspended")
                .code,
            ErrorCode::AccountSuspended
        );
        assert_eq!(
            account(UserStatus::Withdrawn)
                .ensure_usable()
                .expect_err("withdrawn")
                .code,
            ErrorCode::AccountWithdrawn
        );
    }

    #[test]
    fn roles_are_read_from_the_loaded_account() {
        let account = account(UserStatus::Active);
        assert!(account.has_role(AccountRole::Consumer));
        assert!(!account.has_role(AccountRole::StoreOwner));
    }

    #[test]
    fn stale_sign_ins_fail_the_recency_guard() {
        let now = Utc::now();
        let context = |age_secs: i64| AuthContext {
            token: VerifiedToken {
                firebase_uid: "uid".to_owned(),
                email: None,
                email_verified: false,
                display_name: None,
                sign_in_provider: "password".to_owned(),
                auth_time: now - Duration::seconds(age_secs),
                issued_at: now,
                expires_at: now + Duration::hours(1),
            },
            account: None,
        };

        let ten_minutes = std::time::Duration::from_secs(600);
        assert!(context(60).authenticated_within(ten_minutes));
        assert!(!context(601).authenticated_within(ten_minutes));
    }
}
