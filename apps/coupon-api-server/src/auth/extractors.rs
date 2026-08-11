//! Handler-facing authorisation extractors.
//!
//! The auth middleware does the verification once and puts an [`AuthContext`] in the
//! request extensions. These extractors turn that into the precondition a given handler
//! actually needs, so the check cannot be forgotten in a handler body.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::auth::{Account, AuthContext};
use crate::error::{ApiError, ErrorCode};
use crate::state::AppState;
use crate::users::AccountRole;

fn context(parts: &Parts) -> Result<AuthContext, ApiError> {
    parts
        .extensions
        .get::<AuthContext>()
        .cloned()
        // Reaching here means a route was mounted without the auth layer. That is a
        // wiring bug, but 401 is still the only safe thing to tell the client.
        .ok_or_else(ApiError::unauthenticated)
}

/// A verified token, with no requirement that an internal account exists yet.
///
/// This is what `POST /users/bootstrap` needs: the caller has signed in with Firebase
/// but has no row in `coupon.users`.
#[derive(Debug, Clone)]
pub struct Authenticated(pub AuthContext);

impl FromRequestParts<AppState> for Authenticated {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Authenticated(context(parts)?))
    }
}

/// A verified token *and* a usable account. The default for member endpoints.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub account: Account,
    pub context: AuthContext,
}

impl CurrentUser {
    pub fn require_role(&self, role: AccountRole) -> Result<(), ApiError> {
        if self.account.has_role(role) {
            Ok(())
        } else {
            Err(ApiError::with_message(
                ErrorCode::RoleRequired,
                format!("{} 권한이 필요합니다.", role.as_db()),
            ))
        }
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let context = context(parts)?;
        let account = context.require_account()?.clone();
        Ok(CurrentUser { account, context })
    }
}

/// [`CurrentUser`] plus a fresh sign-in. Required by high-risk endpoints — withdrawal,
/// role changes, business-identity edits (§9.3).
#[derive(Debug, Clone)]
pub struct RecentlyAuthenticated(pub CurrentUser);

impl FromRequestParts<AppState> for RecentlyAuthenticated {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;

        if !user
            .context
            .authenticated_within(state.config.recent_auth_max_age())
        {
            return Err(ApiError::new(ErrorCode::ReauthenticationRequired));
        }

        Ok(RecentlyAuthenticated(user))
    }
}

/// A caller acting as a system administrator (§3.3).
///
/// Any of the four administrative roles gets through here; what each of them may *do* is
/// decided per endpoint, because §3.3 separates read scope from change scope and a single
/// "is an admin" bit would flatten that distinction.
#[derive(Debug, Clone)]
pub struct SystemAdmin {
    pub user: CurrentUser,
    /// The administrative roles this caller actually holds.
    pub roles: Vec<AccountRole>,
}

impl SystemAdmin {
    /// Roles §3.3 grants administrative access to. `CONSUMER` and `STORE_OWNER` are
    /// deliberately absent.
    pub const ADMIN_ROLES: [AccountRole; 4] = [
        AccountRole::Support,
        AccountRole::Operations,
        AccountRole::Security,
        AccountRole::SuperAdmin,
    ];

    /// Demand one specific role for a narrower action.
    pub fn require_any(&self, allowed: &[AccountRole]) -> Result<(), ApiError> {
        if self.roles.iter().any(|role| allowed.contains(role)) {
            Ok(())
        } else {
            Err(ApiError::with_message(
                ErrorCode::RoleRequired,
                "이 작업에는 더 높은 관리자 권한이 필요합니다.",
            ))
        }
    }
}

impl FromRequestParts<AppState> for SystemAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;

        let roles: Vec<AccountRole> = Self::ADMIN_ROLES
            .into_iter()
            .filter(|role| user.account.has_role(*role))
            .collect();

        if roles.is_empty() {
            return Err(ApiError::with_message(
                ErrorCode::RoleRequired,
                "관리자 권한이 필요합니다.",
            ));
        }

        Ok(SystemAdmin { user, roles })
    }
}

/// A caller acting as a store owner. Phase 1 only needs the role check; the store-scoped
/// permission model lands with the staff features.
#[derive(Debug, Clone)]
pub struct StoreOwner(pub CurrentUser);

impl FromRequestParts<AppState> for StoreOwner {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        user.require_role(AccountRole::StoreOwner)?;
        Ok(StoreOwner(user))
    }
}
