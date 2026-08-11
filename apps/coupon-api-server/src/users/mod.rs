//! Profile, roles and account state (§10.2 `users`).
//!
//! Owns `coupon.users` and `coupon.user_roles`. Other modules read a user through
//! [`UserService`]; none of them write these tables.

pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::auth::VerifiedToken;
use crate::crypto::{LookupHash, Sealer};
use crate::db::changed_one_row;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::concurrency;

pub use routes::{me_router, users_router};

/// `coupon.user_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserStatus {
    PendingVerification,
    Active,
    Suspended,
    WithdrawalPending,
    Withdrawn,
}

impl UserStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            UserStatus::PendingVerification => "PENDING_VERIFICATION",
            UserStatus::Active => "ACTIVE",
            UserStatus::Suspended => "SUSPENDED",
            UserStatus::WithdrawalPending => "WITHDRAWAL_PENDING",
            UserStatus::Withdrawn => "WITHDRAWN",
        }
    }

    /// Unknown values are treated as suspended: a status this binary does not understand
    /// must not be assumed safe to act on.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "PENDING_VERIFICATION" => UserStatus::PendingVerification,
            "ACTIVE" => UserStatus::Active,
            "WITHDRAWAL_PENDING" => UserStatus::WithdrawalPending,
            "WITHDRAWN" => UserStatus::Withdrawn,
            _ => UserStatus::Suspended,
        }
    }
}

/// `coupon.account_role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccountRole {
    Consumer,
    StoreOwner,
    Support,
    Operations,
    Security,
    SuperAdmin,
    /// A role this binary does not know. Never grants anything.
    Unknown,
}

impl AccountRole {
    pub fn as_db(self) -> &'static str {
        match self {
            AccountRole::Consumer => "CONSUMER",
            AccountRole::StoreOwner => "STORE_OWNER",
            AccountRole::Support => "SUPPORT",
            AccountRole::Operations => "OPERATIONS",
            AccountRole::Security => "SECURITY",
            AccountRole::SuperAdmin => "SUPER_ADMIN",
            AccountRole::Unknown => "UNKNOWN",
        }
    }

    pub fn from_db(raw: &str) -> Self {
        match raw {
            "CONSUMER" => AccountRole::Consumer,
            "STORE_OWNER" => AccountRole::StoreOwner,
            "SUPPORT" => AccountRole::Support,
            "OPERATIONS" => AccountRole::Operations,
            "SECURITY" => AccountRole::Security,
            "SUPER_ADMIN" => AccountRole::SuperAdmin,
            _ => AccountRole::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserProfile {
    pub id: Uuid,
    /// Stable pseudonymous identifier. Safe to show a store owner; never put it in a QR
    /// payload (§16.2).
    pub consumer_key: Uuid,
    pub display_name: String,
    pub status: UserStatus,
    pub email: Option<String>,
    pub email_verified: bool,
    pub roles: Vec<AccountRole>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct BootstrapRequest {
    /// Defaults to the Firebase display name, then to a generated nickname.
    #[validate(length(min = 1, max = 100, message = "이름은 1~100자여야 합니다."))]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateProfileRequest {
    #[validate(length(min = 1, max = 100, message = "이름은 1~100자여야 합니다."))]
    pub display_name: Option<String>,
    /// Expected `version`; may also be supplied as `If-Match`.
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RoleGrant {
    pub role: AccountRole,
    pub granted_at: DateTime<Utc>,
    /// Set for store-scoped roles. Phase 1 grants only account-wide roles.
    pub store_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RolesResponse {
    pub roles: Vec<RoleGrant>,
}

pub struct UserService {
    sealer: Arc<Sealer>,
    lookup_hash: Arc<LookupHash>,
}

impl UserService {
    pub fn new(sealer: Arc<Sealer>, lookup_hash: Arc<LookupHash>) -> Self {
        Self {
            sealer,
            lookup_hash,
        }
    }

    /// Create the internal account for a Firebase user, or return the existing one.
    ///
    /// Idempotent by construction: `users.firebase_uid` is unique, so a concurrent
    /// duplicate loses the insert and reads the winner's row.
    pub async fn bootstrap(
        &self,
        pool: &PgPool,
        token: &VerifiedToken,
        request: &BootstrapRequest,
    ) -> ApiResult<(UserProfile, bool)> {
        if let Some(existing) = self.find_by_firebase_uid(pool, &token.firebase_uid).await? {
            return Ok((existing, false));
        }

        let display_name = resolve_display_name(
            request.display_name.as_deref(),
            token.display_name.as_deref(),
            token.email.as_deref(),
        );

        // Email verification state comes from Firebase; an unverified user still gets an
        // account so they can see the "verify your email" screen.
        let status = if token.email_verified {
            UserStatus::Active
        } else {
            UserStatus::PendingVerification
        };

        let email_ciphertext = token.email.as_deref().map(|email| self.sealer.seal(email));
        let email_hash = token
            .email
            .as_deref()
            .map(|email| self.lookup_hash.hash("user-email", email));

        let mut tx = pool.begin().await?;

        let inserted = sqlx::query!(
            r#"
            INSERT INTO coupon.users
                (firebase_uid, display_name, status, primary_email_ciphertext,
                 primary_email_lookup_hash, email_verified_at)
            VALUES ($1, $2, $3::text::coupon.user_status, $4, $5, $6)
            ON CONFLICT (firebase_uid) DO NOTHING
            RETURNING id
            "#,
            token.firebase_uid,
            display_name,
            status.as_db(),
            email_ciphertext,
            email_hash,
            token.email_verified.then(Utc::now),
        )
        .fetch_optional(&mut *tx)
        .await?;

        let Some(inserted) = inserted else {
            // Lost the race. The winner's row is the answer.
            tx.rollback().await?;
            let existing = self
                .find_by_firebase_uid(pool, &token.firebase_uid)
                .await?
                .ok_or_else(|| {
                    ApiError::new(ErrorCode::ServiceUnavailable)
                        .internal("bootstrap conflict resolved to a missing row")
                })?;
            return Ok((existing, false));
        };

        // Every account starts as a consumer. Store-owner arrives with the first store.
        sqlx::query!(
            r#"
            INSERT INTO coupon.user_roles (user_id, role)
            VALUES ($1, 'CONSUMER')
            ON CONFLICT DO NOTHING
            "#,
            inserted.id,
        )
        .execute(&mut *tx)
        .await?;

        // Record the identity so a later Kakao link lands beside it (§9.2).
        sqlx::query!(
            r#"
            INSERT INTO coupon.auth_identities (user_id, provider, provider_subject)
            VALUES ($1, 'FIREBASE_PASSWORD', $2)
            ON CONFLICT (provider, provider_subject) DO NOTHING
            "#,
            inserted.id,
            token.firebase_uid,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let profile = self
            .find_by_id(pool, inserted.id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::UserNotFound))?;
        Ok((profile, true))
    }

    pub async fn find_by_id(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<Option<UserProfile>> {
        let row = sqlx::query!(
            r#"
            SELECT
                u.id,
                u.consumer_key,
                u.display_name,
                u.status::text AS "status!",
                u.primary_email_ciphertext,
                u.email_verified_at,
                u.created_at,
                u.updated_at,
                u.version,
                COALESCE(
                    ARRAY(
                        SELECT r.role::text FROM coupon.user_roles r
                        WHERE r.user_id = u.id AND r.revoked_at IS NULL
                        ORDER BY r.granted_at
                    ),
                    ARRAY[]::text[]
                ) AS "roles!: Vec<String>"
            FROM coupon.users u
            WHERE u.id = $1
            "#,
            user_id,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| UserProfile {
            id: row.id,
            consumer_key: row.consumer_key,
            display_name: row.display_name,
            status: UserStatus::from_db(&row.status),
            email: row
                .primary_email_ciphertext
                .as_deref()
                .and_then(|sealed| self.sealer.open(sealed)),
            email_verified: row.email_verified_at.is_some(),
            roles: row
                .roles
                .iter()
                .map(|role| AccountRole::from_db(role))
                .collect(),
            created_at: row.created_at,
            updated_at: row.updated_at,
            version: row.version,
        }))
    }

    pub async fn find_by_firebase_uid(
        &self,
        pool: &PgPool,
        firebase_uid: &str,
    ) -> ApiResult<Option<UserProfile>> {
        let id = sqlx::query_scalar!(
            "SELECT id FROM coupon.users WHERE firebase_uid = $1",
            firebase_uid
        )
        .fetch_optional(pool)
        .await?;

        match id {
            Some(id) => self.find_by_id(pool, id).await,
            None => Ok(None),
        }
    }

    /// Update the mutable parts of a profile under optimistic concurrency.
    pub async fn update_profile(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        request: &UpdateProfileRequest,
        expected_version: Option<i64>,
    ) -> ApiResult<UserProfile> {
        let display_name = request.display_name.as_deref().map(str::trim);

        if let Some(name) = display_name {
            if name.is_empty() {
                return Err(ApiError::with_message(
                    ErrorCode::ValidationFailed,
                    "이름은 비워둘 수 없습니다.",
                ));
            }
        }

        let result = sqlx::query!(
            r#"
            UPDATE coupon.users
            SET display_name = COALESCE($2, display_name)
            WHERE id = $1
              AND ($3::bigint IS NULL OR version = $3)
            "#,
            user_id,
            display_name,
            expected_version,
        )
        .execute(pool)
        .await?;

        if !changed_one_row(&result) {
            let exists = sqlx::query_scalar!(
                "SELECT EXISTS (SELECT 1 FROM coupon.users WHERE id = $1)",
                user_id
            )
            .fetch_one(pool)
            .await?
            .unwrap_or(false);

            concurrency::ensure_updated(result.rows_affected(), exists)?;
        }

        self.find_by_id(pool, user_id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::UserNotFound))
    }

    /// Grant a role inside the caller's transaction.
    ///
    /// `user_roles` belongs to this module, so other modules ask for a grant rather than
    /// inserting the row themselves (§10.2). Re-granting an active role is a no-op — the
    /// partial unique index already guarantees at most one live grant per role.
    pub async fn grant_role(
        &self,
        tx: &mut crate::db::Tx<'_>,
        user_id: Uuid,
        role: AccountRole,
    ) -> ApiResult<()> {
        if role == AccountRole::Unknown {
            return Err(ApiError::new(ErrorCode::InvalidRequest).internal("cannot grant UNKNOWN"));
        }

        sqlx::query!(
            r#"
            INSERT INTO coupon.user_roles (user_id, role)
            SELECT $1, $2::text::coupon.account_role
            WHERE NOT EXISTS (
                SELECT 1 FROM coupon.user_roles
                WHERE user_id = $1
                  AND role = $2::text::coupon.account_role
                  AND revoked_at IS NULL
            )
            "#,
            user_id,
            role.as_db(),
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// Active roles, in grant order.
    pub async fn roles(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<Vec<RoleGrant>> {
        let rows = sqlx::query!(
            r#"
            SELECT r.role::text AS "role!", r.granted_at, s.id AS "store_id?"
            FROM coupon.user_roles r
            LEFT JOIN coupon.stores s
                   ON s.owner_user_id = r.user_id
                  AND r.role = 'STORE_OWNER'
                  AND s.status <> 'CLOSED'
            WHERE r.user_id = $1 AND r.revoked_at IS NULL
            ORDER BY r.granted_at
            "#,
            user_id,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| RoleGrant {
                role: AccountRole::from_db(&row.role),
                granted_at: row.granted_at,
                store_id: row.store_id,
            })
            .collect())
    }
}

/// Pick a display name: what the user typed, then Firebase's, then the email local part.
///
/// `users.display_name` is `NOT NULL` with a non-blank check, so this must always
/// produce something usable.
fn resolve_display_name(
    requested: Option<&str>,
    token_name: Option<&str>,
    email: Option<&str>,
) -> String {
    let candidates = [
        requested,
        token_name,
        email.and_then(|e| e.split('@').next()),
    ];

    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|name| !name.is_empty())
        .map(|name| name.chars().take(100).collect())
        .unwrap_or_else(|| "회원".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_and_roles_round_trip_through_their_database_spelling() {
        for status in [
            UserStatus::PendingVerification,
            UserStatus::Active,
            UserStatus::Suspended,
            UserStatus::WithdrawalPending,
            UserStatus::Withdrawn,
        ] {
            assert_eq!(UserStatus::from_db(status.as_db()), status);
        }

        for role in [
            AccountRole::Consumer,
            AccountRole::StoreOwner,
            AccountRole::Support,
            AccountRole::Operations,
            AccountRole::Security,
            AccountRole::SuperAdmin,
        ] {
            assert_eq!(AccountRole::from_db(role.as_db()), role);
        }
    }

    #[test]
    fn unknown_database_values_fail_closed() {
        assert_eq!(
            UserStatus::from_db("SOMETHING_NEW"),
            UserStatus::Suspended,
            "an unrecognised status must not be treated as usable"
        );
        assert_eq!(
            AccountRole::from_db("SOMETHING_NEW"),
            AccountRole::Unknown,
            "an unrecognised role must not grant anything"
        );
    }

    #[test]
    fn display_name_prefers_what_the_user_typed() {
        assert_eq!(
            resolve_display_name(Some("따단"), Some("Firebase Name"), Some("a@b.c")),
            "따단"
        );
    }

    #[test]
    fn display_name_falls_back_through_token_then_email_then_a_default() {
        assert_eq!(
            resolve_display_name(None, Some("Firebase Name"), Some("owner@example.com")),
            "Firebase Name"
        );
        assert_eq!(
            resolve_display_name(None, None, Some("owner@example.com")),
            "owner"
        );
        assert_eq!(resolve_display_name(None, None, None), "회원");
        assert_eq!(
            resolve_display_name(Some("   "), None, None),
            "회원",
            "blank input must not reach the NOT NULL column"
        );
    }

    #[test]
    fn display_name_is_capped_at_the_column_width() {
        let long = "가".repeat(300);
        assert_eq!(
            resolve_display_name(Some(&long), None, None)
                .chars()
                .count(),
            100
        );
    }
}
