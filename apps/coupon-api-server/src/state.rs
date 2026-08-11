//! Shared application state.
//!
//! Handlers reach dependencies through the service structs hung off [`AppState`], never
//! by touching another module's tables directly (§10.2).

use std::sync::Arc;

use sqlx::PgPool;

use crate::admin::AdminService;
use crate::audit::AuditService;
use crate::auth::AuthService;
use crate::catalog::CatalogService;
use crate::config::Config;
use crate::consents::ConsentService;
use crate::crypto::{LookupHash, Sealer};
use crate::error::ApiResult;
use crate::http::rate_limit::RateLimiter;
use crate::loyalty::{PolicyService, StampService};
use crate::notifications::NotificationPreferenceService;
use crate::qr::QrService;
use crate::stores::StoreService;
use crate::users::UserService;
use crate::wallet::WalletService;

/// Optional Redis handle. Redis is a transport and cache, so its absence degrades
/// features rather than failing readiness (§18.2).
#[derive(Clone)]
pub struct RedisHandle {
    pub client: redis::Client,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: PgPool,
    pub redis: Option<RedisHandle>,
    pub auth: Arc<AuthService>,
    pub users: Arc<UserService>,
    pub consents: Arc<ConsentService>,
    pub stores: Arc<StoreService>,
    pub notification_preferences: Arc<NotificationPreferenceService>,
    // Phase 2.
    pub audit: Arc<AuditService>,
    pub catalog: Arc<CatalogService>,
    pub qr: Arc<QrService>,
    pub loyalty_policies: Arc<PolicyService>,
    pub loyalty_stamps: Arc<StampService>,
    pub wallet: Arc<WalletService>,
    pub admin: Arc<AdminService>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    /// Build the service graph.
    ///
    /// Fallible because the QR signer parses a key: a malformed signing key must stop the
    /// process at boot rather than turn every scan into a 500 (§16.2).
    pub fn new(
        config: Arc<Config>,
        pool: PgPool,
        redis: Option<RedisHandle>,
        sealer: Sealer,
        lookup_hash: LookupHash,
    ) -> ApiResult<Self> {
        let sealer = Arc::new(sealer);
        let lookup_hash = Arc::new(lookup_hash);
        let notification_preferences = Arc::new(NotificationPreferenceService::new());
        let users = Arc::new(UserService::new(sealer.clone(), lookup_hash.clone()));
        let stores = Arc::new(StoreService::new(
            sealer,
            lookup_hash.clone(),
            users.clone(),
        ));

        let audit = Arc::new(AuditService::new());
        let catalog = Arc::new(CatalogService::new());
        let qr = Arc::new(QrService::new(config.clone(), lookup_hash.clone())?);
        let loyalty_policies = Arc::new(PolicyService::new(catalog.clone()));
        let loyalty_stamps = Arc::new(StampService::new(
            stores.clone(),
            loyalty_policies.clone(),
            catalog.clone(),
            qr.clone(),
            audit.clone(),
            lookup_hash.clone(),
            config.stamp_void_window(),
        ));

        Ok(Self {
            auth: Arc::new(AuthService::new(config.clone())),
            consents: Arc::new(ConsentService::new(
                lookup_hash,
                notification_preferences.clone(),
            )),
            admin: Arc::new(AdminService::new(audit.clone(), config.admin_preview_ttl())),
            rate_limiter: Arc::new(RateLimiter::new(redis.clone())),
            wallet: Arc::new(WalletService::new()),
            audit,
            catalog,
            qr,
            loyalty_policies,
            loyalty_stamps,
            stores,
            users,
            notification_preferences,
            config,
            pool,
            redis,
        })
    }
}
