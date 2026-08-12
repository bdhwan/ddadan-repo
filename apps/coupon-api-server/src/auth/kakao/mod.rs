//! Kakao OIDC login (§9.2), account linking (§11.2, AUTH-003) and the 연결 해제 웹훅.
//!
//! The eight steps of §9.2 map onto four endpoints:
//!
//! | Step | Endpoint |
//! |---|---|
//! | 1–2 | `GET /auth/kakao/authorize` — mint `state` + PKCE verifier, return the URL |
//! | 3–5 | `GET /auth/kakao/callback` — verify everything, hand back a one-time code |
//! | 6–7 | `POST /auth/kakao/exchange` — resolve the member, mint a Firebase custom token |
//! | 8 | Angular's `signInWithCustomToken`, which is not ours |
//!
//! Three rules from §9.2 shape the code more than the steps do:
//!
//! * **Kakao's access and refresh tokens are discarded.** Not stored and then deleted —
//!   never read. [`oidc::TokenResponse`] has no field for either. MVP has no feature that
//!   calls a Kakao user API, so holding a token would be a liability with no upside.
//! * **Email is a hint, not a merge key.** A Kakao account whose email matches an
//!   existing member is still a *different* login. Joining the two is [`link`], which the
//!   member performs while signed in to the account they want to keep (AUTH-003).
//! * **One canonical Firebase UID per member.** A Kakao-first sign-up gets a UID minted
//!   here and keeps it; a Kakao identity linked onto an existing account adopts *that*
//!   account's UID. Either way the custom token names the member, not the provider.

pub mod oidc;
pub mod routes;
pub mod sessions;

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::audit::{ActorType, AuditEntry, AuditService};
use crate::auth::custom_token::{self, CustomTokenSigner, MintedCustomToken};
use crate::config::Config;
use crate::crypto::{LookupHash, Sealer};
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::users::UserStatus;

use oidc::KakaoOidc;

/// The `iss` every Kakao `id_token` must carry (§9.2-5). Not configurable — see
/// [`oidc`] for why the endpoint origin is and this is not.
pub const KAKAO_ISSUER: &str = "https://kauth.kakao.com";

/// `coupon.auth_provider` spelling for Kakao.
pub const PROVIDER_KAKAO: &str = "KAKAO";

/// Scopes we ask Kakao for. `openid` is what makes it an OIDC flow at all; the other two
/// are the profile fields §9.2-6 uses to fill in a new member, and Kakao may return
/// neither if the user declines.
const KAKAO_SCOPES: &str = "openid account_email profile_nickname";

/// Bytes of entropy behind `state`, the nonce, the PKCE verifier and the exchange code.
const SECRET_BYTES: usize = 32;

/// A Kakao identity resolved from an OIDC `id_token`.
#[derive(Debug, Clone)]
pub struct KakaoIdentity {
    /// Kakao's `sub`. Stored as `auth_identities.provider_subject`, never as a user id.
    pub provider_subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
}

/// What `GET /auth/kakao/authorize` returns.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuthorizeStart {
    /// Where to send the browser.
    pub authorize_url: String,
    /// Echoed so the SPA can correlate its own navigation. The server holds the only
    /// copy that matters.
    pub state: String,
    /// After this the login must be started again.
    pub expires_at: DateTime<Utc>,
}

/// What `GET /auth/kakao/callback` returns.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CallbackResult {
    /// Single use. Spend it at `POST /auth/kakao/exchange` to sign in, or at
    /// `POST /me/auth-links/kakao` to attach this Kakao account to the one already
    /// signed in (AUTH-003).
    pub exchange_code: String,
    pub expires_at: DateTime<Utc>,
}

/// What `POST /auth/kakao/exchange` returns (§9.2-7).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SignInResult {
    /// Feed this to `signInWithCustomToken` (§9.2-8).
    pub custom_token: String,
    pub expires_at: DateTime<Utc>,
    /// The member this token signs in as.
    pub user_id: Uuid,
    /// `true` when this login created the member. The SPA uses it to route a first-time
    /// user through 약관 동의 (AUTH-002 step 5).
    pub created: bool,
}

/// One row of `GET`/`POST`/`DELETE /me/auth-links/kakao`.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuthLink {
    pub provider: String,
    pub status: String,
    pub linked_at: DateTime<Utc>,
}

/// What the unlink webhook did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UnlinkOutcome {
    /// An active link was found and deactivated.
    Applied,
    /// This exact event has already been handled. Nothing changed (§9.2 멱등).
    AlreadyProcessed,
    /// Kakao told us about a `sub` we have never seen. Recorded, nothing to do.
    UnknownIdentity,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnlinkAck {
    pub outcome: UnlinkOutcome,
}

/// Kakao app registration. Absent until somebody registers the app.
#[derive(Debug, Clone)]
pub struct KakaoApp {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

pub struct KakaoService {
    config: Arc<Config>,
    oidc: KakaoOidc,
    sealer: Arc<Sealer>,
    lookup_hash: Arc<LookupHash>,
    audit: Arc<AuditService>,
    /// `None` until a Firebase service account is configured (§9.2-7).
    custom_tokens: Option<CustomTokenSigner>,
}

impl std::fmt::Debug for KakaoService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KakaoService")
            .field("app_configured", &self.app().is_ok())
            .field("can_mint_custom_tokens", &self.custom_tokens.is_some())
            .finish()
    }
}

impl KakaoService {
    pub fn new(
        config: Arc<Config>,
        sealer: Arc<Sealer>,
        lookup_hash: Arc<LookupHash>,
        audit: Arc<AuditService>,
        custom_tokens: Option<CustomTokenSigner>,
    ) -> Self {
        Self {
            oidc: KakaoOidc::new(config.kakao_oidc_base_url()),
            config,
            sealer,
            lookup_hash,
            audit,
            custom_tokens,
        }
    }

    /// The registered Kakao app, or the error to return when there is none.
    ///
    /// Every Kakao endpoint starts here, so an unregistered deployment fails the same
    /// way everywhere: a 503 that says the feature is not available, with the missing
    /// settings named in the internal detail for whoever has to fix it.
    pub fn app(&self) -> ApiResult<KakaoApp> {
        let (Some(client_id), Some(redirect_uri)) = (
            non_blank(self.config.kakao_client_id.as_deref()),
            non_blank(self.config.kakao_redirect_uri.as_deref()),
        ) else {
            return Err(ApiError::with_message(
                ErrorCode::ServiceUnavailable,
                "카카오 로그인을 사용할 수 없습니다.",
            )
            .internal(
                "COUPON_KAKAO_CLIENT_ID and COUPON_KAKAO_REDIRECT_URI are required for \
                 Kakao login (§9.2)",
            ));
        };

        Ok(KakaoApp {
            client_id,
            client_secret: non_blank(self.config.kakao_client_secret.as_deref()),
            redirect_uri,
        })
    }

    /// §9.2 steps 1–2. Mint `state`, a nonce and a PKCE verifier; keep them server-side.
    pub async fn start_authorize(&self, pool: &PgPool) -> ApiResult<AuthorizeStart> {
        let app = self.app()?;
        let discovery = self.oidc.discovery().await?;

        let state = random_secret();
        let nonce = random_secret();
        let code_verifier = random_secret();
        let code_challenge = pkce_challenge(&code_verifier);
        let expires_at = Utc::now() + self.config.oauth_login_session_ttl();

        sessions::create_login_session(
            pool,
            &self.lookup_hash,
            &self.sealer,
            PROVIDER_KAKAO,
            &state,
            &nonce,
            &code_verifier,
            &app.redirect_uri,
            expires_at,
        )
        .await?;

        let authorize_url = build_authorize_url(
            &discovery.authorization_endpoint,
            &app,
            &state,
            &nonce,
            &code_challenge,
        );

        Ok(AuthorizeStart {
            authorize_url,
            state,
            expires_at,
        })
    }

    /// §9.2 steps 3–5, ending in the single-use exchange code.
    pub async fn complete_callback(
        &self,
        pool: &PgPool,
        state: &str,
        code: &str,
    ) -> ApiResult<CallbackResult> {
        let app = self.app()?;

        // The session lookup *is* the state check: no live row for this `state` means it
        // was never issued, has already been spent, or has expired.
        //
        // Spending it here — before the token exchange, not after — means a transient
        // Kakao failure burns the state and the member has to start the login again.
        // That is the right trade: Kakao's authorization code is single-use too, so a
        // retry of the same callback could never have succeeded, and leaving the state
        // live would hand a replayed callback a second chance at it.
        let session = sessions::consume_login_session(
            pool,
            &self.lookup_hash,
            &self.sealer,
            PROVIDER_KAKAO,
            state,
            Utc::now(),
        )
        .await?
        .ok_or_else(|| {
            ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                .internal("no live login session for the presented state")
        })?;

        let tokens = self
            .oidc
            .exchange_code(
                &app.client_id,
                app.client_secret.as_deref(),
                &session.redirect_uri,
                code,
                &session.code_verifier,
            )
            .await?;

        let identity = self
            .oidc
            .verify_id_token(&tokens.id_token, &app.client_id, &session.nonce)
            .await?;

        // Everything Kakao sent us that is not the identity goes out of scope here and
        // is never written down (§9.2: access/refresh 토큰은 로그인 완료 후 폐기).
        drop(tokens);

        let exchange_code = random_secret();
        let expires_at = Utc::now() + self.config.oauth_exchange_code_ttl();
        sessions::issue_exchange_code(
            pool,
            &self.lookup_hash,
            &self.sealer,
            PROVIDER_KAKAO,
            &exchange_code,
            &identity,
            expires_at,
        )
        .await?;

        Ok(CallbackResult {
            exchange_code,
            expires_at,
        })
    }

    /// §9.2 steps 6–7: resolve or create the member, then mint their custom token.
    pub async fn exchange(&self, pool: &PgPool, exchange_code: &str) -> ApiResult<SignInResult> {
        let identity = self.spend_exchange_code(pool, exchange_code).await?;
        let (user_id, firebase_uid, created) = self.resolve_or_create_member(pool, &identity).await?;
        let minted = self.mint_for(&firebase_uid)?;

        Ok(SignInResult {
            custom_token: minted.token,
            expires_at: minted.expires_at,
            user_id,
            created,
        })
    }

    /// AUTH-003: attach a Kakao identity to the account that is already signed in.
    ///
    /// This is the *only* way two login methods end up on one member. Matching emails
    /// never merge anything by themselves (§9.2), and an identity that already belongs to
    /// somebody else is refused here rather than transferred — AUTH-003 sends that case
    /// to 고객센터 본인 확인 instead.
    pub async fn link(
        &self,
        pool: &PgPool,
        user_id: Uuid,
        exchange_code: &str,
    ) -> ApiResult<AuthLink> {
        let identity = self.spend_exchange_code(pool, exchange_code).await?;

        let mut tx = pool.begin().await?;
        Self::lock_subject(&mut tx, &identity.provider_subject).await?;

        let existing = sqlx::query!(
            r#"
            SELECT id, user_id, status::text AS "status!"
            FROM coupon.auth_identities
            WHERE provider = 'KAKAO' AND provider_subject = $1
            FOR UPDATE
            "#,
            identity.provider_subject,
        )
        .fetch_optional(&mut *tx)
        .await?;

        // §11.2 spells the endpoint `/me/auth-links/kakao`, singular, and `unlink` picks
        // "the active Kakao identity" without a tie-break. Two of them on one member would
        // make that pick arbitrary, so the second is refused rather than quietly stored.
        let other_kakao = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM coupon.auth_identities
            WHERE user_id = $1
              AND provider = 'KAKAO'
              AND status = 'ACTIVE'
              AND provider_subject <> $2
            "#,
            user_id,
            identity.provider_subject,
        )
        .fetch_one(&mut *tx)
        .await?;

        if other_kakao > 0 {
            return Err(ApiError::with_message(
                ErrorCode::Conflict,
                "이미 다른 카카오 계정이 연결되어 있습니다. 먼저 해제해 주세요.",
            )
            .internal("member already has an active KAKAO identity"));
        }

        let linked_at = match existing {
            Some(row) if row.user_id != user_id => {
                return Err(ApiError::new(ErrorCode::AuthLinkAlreadyClaimed).internal(format!(
                    "kakao subject already linked to user {}",
                    row.user_id
                )));
            }
            // Already ours. Re-linking is a no-op, and re-linking something the unlink
            // webhook deactivated is how a member gets back in (AUTH-002).
            Some(row) => {
                sqlx::query_scalar!(
                    r#"
                    UPDATE coupon.auth_identities
                    SET status = 'ACTIVE', unlinked_at = NULL
                    WHERE id = $1
                    RETURNING linked_at
                    "#,
                    row.id,
                )
                .fetch_one(&mut *tx)
                .await?
            }
            None => {
                sqlx::query_scalar!(
                    r#"
                    INSERT INTO coupon.auth_identities
                        (user_id, provider, provider_subject, provider_profile_snapshot)
                    VALUES ($1, 'KAKAO', $2, $3)
                    RETURNING linked_at
                    "#,
                    user_id,
                    identity.provider_subject,
                    profile_snapshot(&identity),
                )
                .fetch_one(&mut *tx)
                .await?
            }
        };

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::User, "auth_identity.linked", "user")
                    .resource(user_id)
                    .actor(user_id)
                    .reason("카카오 로그인 수단 연결")
                    .metadata(serde_json::json!({ "provider": PROVIDER_KAKAO })),
            )
            .await?;

        tx.commit().await?;

        Ok(AuthLink {
            provider: PROVIDER_KAKAO.to_owned(),
            status: "ACTIVE".to_owned(),
            linked_at,
        })
    }

    /// §11.2 연결 해제, performed by the member themselves.
    ///
    /// Refused when it would be the last way in. The unlink *webhook* has no such guard —
    /// Kakao has already cut the link on their side and pretending otherwise would leave
    /// us out of step with them — but a member clicking a button in our own settings
    /// screen should not be able to lock themselves out (AUTH-002).
    pub async fn unlink(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<()> {
        let mut tx = pool.begin().await?;

        let identity = sqlx::query!(
            r#"
            SELECT id FROM coupon.auth_identities
            WHERE user_id = $1 AND provider = 'KAKAO' AND status = 'ACTIVE'
            FOR UPDATE
            "#,
            user_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::AuthLinkNotFound))?;

        let others = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM coupon.auth_identities
            WHERE user_id = $1 AND status = 'ACTIVE' AND id <> $2
            "#,
            user_id,
            identity.id,
        )
        .fetch_one(&mut *tx)
        .await?;

        if others == 0 {
            return Err(ApiError::new(ErrorCode::LastAuthLinkCannotBeRemoved));
        }

        sqlx::query!(
            r#"
            UPDATE coupon.auth_identities
            SET status = 'UNLINKED', unlinked_at = $2
            WHERE id = $1
            "#,
            identity.id,
            Utc::now(),
        )
        .execute(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::User, "auth_identity.unlinked", "user")
                    .resource(user_id)
                    .actor(user_id)
                    .reason("회원이 직접 연결 해제")
                    .metadata(serde_json::json!({ "provider": PROVIDER_KAKAO })),
            )
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Active login methods on this account.
    pub async fn links(&self, pool: &PgPool, user_id: Uuid) -> ApiResult<Vec<AuthLink>> {
        let rows = sqlx::query!(
            r#"
            SELECT provider::text AS "provider!", status::text AS "status!", linked_at
            FROM coupon.auth_identities
            WHERE user_id = $1 AND status = 'ACTIVE'
            ORDER BY linked_at
            "#,
            user_id,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| AuthLink {
                provider: row.provider,
                status: row.status,
                linked_at: row.linked_at,
            })
            .collect())
    }

    /// Kakao's 연결 해제 웹훅, applied idempotently (§9.2 마지막).
    ///
    /// `event_key` has already been derived by the route from the signed material, so a
    /// verbatim replay collides with the row this call inserts and changes nothing.
    pub async fn handle_unlink(
        &self,
        pool: &PgPool,
        event_key: &str,
        provider_subject: &str,
        payload: serde_json::Value,
    ) -> ApiResult<UnlinkAck> {
        let mut tx = pool.begin().await?;

        let first_time = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.provider_webhook_events (provider, event_type, event_key, payload)
            VALUES ('KAKAO', 'UNLINK', $1, $2)
            ON CONFLICT (provider, event_type, event_key) DO NOTHING
            RETURNING id
            "#,
            event_key,
            payload,
        )
        .fetch_optional(&mut *tx)
        .await?
        .is_some();

        if !first_time {
            tx.rollback().await?;
            return Ok(UnlinkAck {
                outcome: UnlinkOutcome::AlreadyProcessed,
            });
        }

        let identity = sqlx::query!(
            r#"
            UPDATE coupon.auth_identities
            SET status = 'UNLINKED', unlinked_at = $2
            WHERE provider = 'KAKAO' AND provider_subject = $1 AND status = 'ACTIVE'
            RETURNING user_id
            "#,
            provider_subject,
            Utc::now(),
        )
        .fetch_optional(&mut *tx)
        .await?;

        let outcome = match identity {
            Some(row) => {
                self.audit
                    .record(
                        &mut tx,
                        AuditEntry::new(ActorType::Provider, "auth_identity.unlinked", "user")
                            .resource(row.user_id)
                            .reason("카카오 연결 해제 웹훅")
                            .metadata(serde_json::json!({
                                "provider": PROVIDER_KAKAO,
                                "event_key": event_key,
                            })),
                    )
                    .await?;
                UnlinkOutcome::Applied
            }
            // Nothing active to cut: either we never knew this account, or a previous
            // event already handled it. Both are fine — the event is still recorded.
            None => UnlinkOutcome::UnknownIdentity,
        };

        tx.commit().await?;
        Ok(UnlinkAck { outcome })
    }

    /// Serialise everything that touches one Kakao `sub`, for the duration of `tx`.
    ///
    /// `SELECT ... FOR UPDATE` only locks rows that exist, and the case that actually
    /// races is the one where the identity row does *not* exist yet: two tabs finishing
    /// a first login at the same moment both find nothing and both insert. The unique
    /// index catches that and turns it into a retryable 409, which is safe but is a
    /// baffling thing to show somebody signing in for the first time. An advisory lock
    /// on the subject makes the second transaction wait and then find the winner's row,
    /// which is what it was going to do anyway.
    async fn lock_subject(tx: &mut crate::db::Tx<'_>, provider_subject: &str) -> ApiResult<()> {
        sqlx::query!(
            "SELECT pg_advisory_xact_lock(hashtext($1)::bigint)",
            format!("kakao:{provider_subject}"),
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn spend_exchange_code(
        &self,
        pool: &PgPool,
        exchange_code: &str,
    ) -> ApiResult<KakaoIdentity> {
        sessions::consume_exchange_code(
            pool,
            &self.lookup_hash,
            &self.sealer,
            PROVIDER_KAKAO,
            exchange_code,
            Utc::now(),
        )
        .await?
        .ok_or_else(|| {
            // Spent, expired, or never issued. AUTH-002 calls a reused code a security
            // event, and it is logged as one; the client is told only 보안 검증 실패.
            ApiError::new(ErrorCode::KakaoSecurityCheckFailed)
                .internal("exchange code is not live (already spent, expired, or unknown)")
        })
    }

    /// §9.2-6. `(provider, provider_subject)` → member, creating one if this Kakao
    /// account has never signed in here.
    ///
    /// Returns `(user_id, canonical firebase uid, created)`.
    async fn resolve_or_create_member(
        &self,
        pool: &PgPool,
        identity: &KakaoIdentity,
    ) -> ApiResult<(Uuid, String, bool)> {
        let mut tx = pool.begin().await?;
        Self::lock_subject(&mut tx, &identity.provider_subject).await?;

        let existing = sqlx::query!(
            r#"
            SELECT i.id AS identity_id,
                   u.id AS user_id,
                   u.firebase_uid,
                   u.status::text AS "status!",
                   i.status::text AS "identity_status!"
            FROM coupon.auth_identities i
            JOIN coupon.users u ON u.id = i.user_id
            WHERE i.provider = 'KAKAO' AND i.provider_subject = $1
            FOR UPDATE OF i
            "#,
            identity.provider_subject,
        )
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(row) = existing {
            // The account's state is read from the database on every sign-in, exactly as
            // §9.3 requires for every other request.
            ensure_can_sign_in(UserStatus::from_db(&row.status))?;

            // Signing in through Kakao again *is* the re-authentication AUTH-002 asks for
            // after an unlink, so a deactivated link comes back to life here.
            if row.identity_status != "ACTIVE" {
                sqlx::query!(
                    r#"
                    UPDATE coupon.auth_identities
                    SET status = 'ACTIVE', unlinked_at = NULL
                    WHERE id = $1
                    "#,
                    row.identity_id,
                )
                .execute(&mut *tx)
                .await?;

                self.audit
                    .record(
                        &mut tx,
                        AuditEntry::new(ActorType::User, "auth_identity.relinked", "user")
                            .resource(row.user_id)
                            .actor(row.user_id)
                            .reason("카카오 재로그인")
                            .metadata(serde_json::json!({ "provider": PROVIDER_KAKAO })),
                    )
                    .await?;
            }

            tx.commit().await?;
            return Ok((row.user_id, row.firebase_uid, false));
        }

        // A member we have never met. The canonical Firebase UID is minted here and is
        // the identifier every later token carries — including after this member links a
        // password login onto the same account.
        let firebase_uid = format!("kakao_{}", Uuid::new_v4().simple());
        let display_name = default_display_name(identity);

        let email_ciphertext = identity
            .email
            .as_deref()
            .map(|email| self.sealer.seal(email));
        let email_hash = identity
            .email
            .as_deref()
            .map(|email| self.lookup_hash.hash("user-email", email));

        // Kakao vouches for the identity itself, so there is no separate verification
        // step for the member to complete and no reason to park them in
        // `PENDING_VERIFICATION` — the state that, before this phase, was a one-way trip
        // out of every campaign audience. `email_verified_at` is set only when Kakao
        // actually says the email is verified, because that claim is about the address
        // and not about the account.
        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.users
                (firebase_uid, display_name, status, primary_email_ciphertext,
                 primary_email_lookup_hash, email_verified_at)
            VALUES ($1, $2, 'ACTIVE', $3, $4, $5)
            RETURNING id
            "#,
            firebase_uid,
            display_name,
            email_ciphertext,
            email_hash,
            identity.email_verified.then(Utc::now),
        )
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.user_roles (user_id, role)
            VALUES ($1, 'CONSUMER')
            ON CONFLICT DO NOTHING
            "#,
            user_id,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.auth_identities
                (user_id, provider, provider_subject, provider_profile_snapshot)
            VALUES ($1, 'KAKAO', $2, $3)
            "#,
            user_id,
            identity.provider_subject,
            profile_snapshot(identity),
        )
        .execute(&mut *tx)
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(ActorType::User, "user.created", "user")
                    .resource(user_id)
                    .actor(user_id)
                    .reason("카카오 최초 로그인")
                    .metadata(serde_json::json!({ "provider": PROVIDER_KAKAO })),
            )
            .await?;

        tx.commit().await?;
        Ok((user_id, firebase_uid, true))
    }

    fn mint_for(&self, firebase_uid: &str) -> ApiResult<MintedCustomToken> {
        self.custom_tokens
            .as_ref()
            .ok_or_else(custom_token::not_configured)?
            .mint(firebase_uid, Utc::now())
    }
}

/// Refuse a sign-in the account state does not allow (§9.3).
///
/// `PENDING_VERIFICATION` is allowed through for the same reason [`crate::auth::Account`]
/// allows it: the member has to be able to reach the screen that tells them what to do.
fn ensure_can_sign_in(status: UserStatus) -> ApiResult<()> {
    match status {
        UserStatus::Active | UserStatus::PendingVerification => Ok(()),
        UserStatus::Suspended => Err(ApiError::new(ErrorCode::AccountSuspended)),
        UserStatus::WithdrawalPending | UserStatus::Withdrawn => {
            Err(ApiError::new(ErrorCode::AccountWithdrawn))
        }
    }
}

/// What we keep of Kakao's profile.
///
/// Deliberately thin: an email flag and nothing else. §9.2 discards the tokens that would
/// let us fetch more, and §16.5 would rather we did not hold what we cannot use.
fn profile_snapshot(identity: &KakaoIdentity) -> serde_json::Value {
    serde_json::json!({
        "email_verified": identity.email_verified,
        "has_email": identity.email.is_some(),
    })
}

/// A name for a member who has not told us one yet.
///
/// Kakao's `id_token` carries no nickname unless the user consented to share it, and the
/// column is `NOT NULL` with a non-blank check, so this must always produce something.
fn default_display_name(identity: &KakaoIdentity) -> String {
    identity
        .email
        .as_deref()
        .and_then(|email| email.split('@').next())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.chars().take(100).collect())
        .unwrap_or_else(|| "회원".to_owned())
}

fn non_blank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// 256 bits of randomness, base64url. Used for `state`, the nonce, the PKCE verifier and
/// the exchange code — all of them values whose only job is to be unguessable.
fn random_secret() -> String {
    let mut bytes = [0u8; SECRET_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The S256 PKCE challenge for a verifier (RFC 7636 §4.2).
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Build the URL the browser is sent to (§9.2-1).
fn build_authorize_url(
    authorization_endpoint: &str,
    app: &KakaoApp,
    state: &str,
    nonce: &str,
    code_challenge: &str,
) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", app.client_id.as_str()),
        ("redirect_uri", app.redirect_uri.as_str()),
        ("scope", KAKAO_SCOPES),
        ("state", state),
        ("nonce", nonce),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
    ]
    .iter()
    .map(|(key, value)| format!("{key}={}", urlencode(value)))
    .collect::<Vec<_>>()
    .join("&");

    let separator = if authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{authorization_endpoint}{separator}{query}")
}

/// Percent-encode for a query-string value.
///
/// Hand-rolled rather than pulled in: the whole alphabet we ever encode is a redirect
/// URI and a handful of base64url secrets, and a dependency for that would be a
/// dependency to keep patched.
fn urlencode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> KakaoApp {
        KakaoApp {
            client_id: "kakao-rest-key".to_owned(),
            client_secret: None,
            redirect_uri: "https://app.ddadan.test/auth/kakao/callback".to_owned(),
        }
    }

    #[test]
    fn the_authorize_url_carries_everything_kakao_needs() {
        let url = build_authorize_url(
            "https://kauth.kakao.com/oauth/authorize",
            &app(),
            "the-state",
            "the-nonce",
            "the-challenge",
        );

        assert!(url.starts_with("https://kauth.kakao.com/oauth/authorize?"));
        for expected in [
            "response_type=code",
            "client_id=kakao-rest-key",
            "state=the-state",
            "nonce=the-nonce",
            "code_challenge=the-challenge",
            "code_challenge_method=S256",
            "scope=openid%20account_email%20profile_nickname",
            "redirect_uri=https%3A%2F%2Fapp.ddadan.test%2Fauth%2Fkakao%2Fcallback",
        ] {
            assert!(url.contains(expected), "{expected} missing from {url}");
        }
    }

    #[test]
    fn an_endpoint_that_already_has_a_query_gets_an_ampersand() {
        let url = build_authorize_url(
            "http://127.0.0.1:9000/oauth/authorize?tenant=mock",
            &app(),
            "s",
            "n",
            "c",
        );
        assert!(url.contains("?tenant=mock&response_type=code"), "{url}");
    }

    #[test]
    fn the_pkce_challenge_is_the_rfc_7636_s256_of_the_verifier() {
        // The worked example from RFC 7636 Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn secrets_are_unguessable_and_url_safe() {
        let first = random_secret();
        let second = random_secret();

        assert_ne!(first, second);
        assert_eq!(first.len(), 43, "32 bytes of base64url with no padding");
        assert!(
            first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{first} must survive a query string untouched"
        );
    }

    #[test]
    fn only_live_accounts_may_sign_in_with_kakao() {
        ensure_can_sign_in(UserStatus::Active).expect("active");
        ensure_can_sign_in(UserStatus::PendingVerification).expect("must reach the verify screen");

        assert_eq!(
            ensure_can_sign_in(UserStatus::Suspended)
                .expect_err("suspended")
                .code,
            ErrorCode::AccountSuspended
        );
        assert_eq!(
            ensure_can_sign_in(UserStatus::Withdrawn)
                .expect_err("withdrawn")
                .code,
            ErrorCode::AccountWithdrawn
        );
    }

    #[test]
    fn a_member_who_shared_no_email_still_gets_a_usable_display_name() {
        // The column is NOT NULL with a non-blank check, and AUTH-002 allows signing up
        // without an email at all.
        let anonymous = KakaoIdentity {
            provider_subject: "1".to_owned(),
            email: None,
            email_verified: false,
        };
        assert_eq!(default_display_name(&anonymous), "회원");

        let with_email = KakaoIdentity {
            email: Some("dahye@kakao.test".to_owned()),
            ..anonymous
        };
        assert_eq!(default_display_name(&with_email), "dahye");
    }

    #[test]
    fn the_profile_snapshot_keeps_no_personal_data() {
        // §16.5: hold what is needed. The email itself is already sealed on `users`;
        // copying it into a jsonb snapshot would make a second place to leak it from.
        let snapshot = profile_snapshot(&KakaoIdentity {
            provider_subject: "1".to_owned(),
            email: Some("dahye@kakao.test".to_owned()),
            email_verified: true,
        });

        assert_eq!(
            snapshot,
            serde_json::json!({ "email_verified": true, "has_email": true })
        );
        assert!(!snapshot.to_string().contains("dahye"));
    }
}
