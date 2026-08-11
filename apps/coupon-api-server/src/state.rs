//! Shared application state.
//!
//! Handlers reach dependencies through the service structs hung off [`AppState`], never
//! by touching another module's tables directly (§10.2).

use std::sync::Arc;

use sqlx::PgPool;

use crate::auth::AuthService;
use crate::config::Config;
use crate::consents::ConsentService;
use crate::crypto::{LookupHash, Sealer};
use crate::notifications::NotificationPreferenceService;
use crate::stores::StoreService;
use crate::users::UserService;

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
}

impl AppState {
    pub fn new(
        config: Arc<Config>,
        pool: PgPool,
        redis: Option<RedisHandle>,
        sealer: Sealer,
        lookup_hash: LookupHash,
    ) -> Self {
        let sealer = Arc::new(sealer);
        let lookup_hash = Arc::new(lookup_hash);
        let notification_preferences = Arc::new(NotificationPreferenceService::new());
        let users = Arc::new(UserService::new(sealer.clone(), lookup_hash.clone()));

        Self {
            auth: Arc::new(AuthService::new(config.clone())),
            consents: Arc::new(ConsentService::new(
                lookup_hash.clone(),
                notification_preferences.clone(),
            )),
            stores: Arc::new(StoreService::new(sealer, lookup_hash, users.clone())),
            users,
            notification_preferences,
            config,
            pool,
            redis,
        }
    }
}
