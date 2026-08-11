//! Shared application state.
//!
//! Handlers reach dependencies through the service structs hung off [`AppState`], never
//! by touching another module's tables directly (§10.2).

use std::sync::Arc;

use sqlx::PgPool;

use crate::admin::AdminService;
use crate::admin::operations::OperationsService;
use crate::analytics::AnalyticsService;
use crate::audit::AuditService;
use crate::auth::AuthService;
use crate::campaigns::CampaignService;
use crate::catalog::CatalogService;
use crate::config::Config;
use crate::consents::ConsentService;
use crate::crypto::{LookupHash, Sealer};
use crate::error::ApiResult;
use crate::http::rate_limit::RateLimiter;
use crate::jobs::JobService;
use crate::loyalty::{PolicyService, StampService};
use crate::http::metrics::MetricsRegistry;
use crate::notifications::providers::{
    AlimtalkProvider, HttpMessageProvider, RecordingProvider, WebPushProvider,
};
use crate::notifications::{NotificationPreferenceService, NotificationService};
use crate::privacy::PrivacyService;
use crate::qr::QrService;
use crate::redemptions::RedemptionService;
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
    // Phase 3: discount campaigns, redemption, and the job queue that bulk issuance
    // cannot work without (§14).
    pub jobs: Arc<JobService>,
    pub campaigns: Arc<CampaignService>,
    pub redemptions: Arc<RedemptionService>,
    // Phase 4: notification delivery, the operational surface, analytics, and retention.
    pub notifications: Arc<NotificationService>,
    pub operations: Arc<OperationsService>,
    pub analytics: Arc<AnalyticsService>,
    pub privacy: Arc<PrivacyService>,
    /// The two §15.1 seams. Held as `dyn` so a 대행사 swap is a wiring change here and
    /// nothing else.
    pub web_push_provider: Arc<dyn WebPushProvider>,
    pub alimtalk_provider: Arc<dyn AlimtalkProvider>,
    /// Opening a push token needs the same sealer the registration used (§16.5).
    pub sealer: Arc<Sealer>,
    /// §18.4's process counters.
    pub metrics: Arc<MetricsRegistry>,
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
        let audit = Arc::new(AuditService::new());
        let stores = Arc::new(StoreService::new(
            sealer.clone(),
            lookup_hash.clone(),
            users.clone(),
            audit.clone(),
        ));

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

        let jobs = Arc::new(JobService::new());
        let notifications = Arc::new(NotificationService::new(
            jobs.clone(),
            sealer.clone(),
            lookup_hash.clone(),
        ));
        let campaigns = Arc::new(CampaignService::new(
            stores.clone(),
            catalog.clone(),
            audit.clone(),
            jobs.clone(),
        ));
        let redemptions = Arc::new(RedemptionService::new(
            stores.clone(),
            catalog.clone(),
            qr.clone(),
            audit.clone(),
            config.redemption_reservation_ttl(),
            config.redemption_void_window(),
        ));

        // §15.1: 알림톡은 `AlimtalkProvider` 인터페이스 뒤에 둬 교체 가능하게 한다. Web
        // push gets the same treatment for the same reason. With no endpoint configured the
        // recording stub runs, which `Config::validate` refuses in production.
        let web_push_provider: Arc<dyn WebPushProvider> = match &config.fcm_endpoint {
            Some(endpoint) => Arc::new(HttpMessageProvider::new(
                "fcm",
                endpoint,
                config.fcm_authorization.clone(),
            )),
            None => Arc::new(RecordingProvider::new("fcm-stub")),
        };
        let alimtalk_provider: Arc<dyn AlimtalkProvider> = match &config.alimtalk_endpoint {
            Some(endpoint) => Arc::new(HttpMessageProvider::new(
                "alimtalk",
                endpoint,
                config.alimtalk_authorization.clone(),
            )),
            None => Arc::new(RecordingProvider::new("alimtalk-stub")),
        };

        Ok(Self {
            auth: Arc::new(AuthService::new(config.clone())),
            operations: Arc::new(OperationsService::new(audit.clone())),
            analytics: Arc::new(AnalyticsService::new(config.min_cohort_size())),
            privacy: Arc::new(PrivacyService::new(
                audit.clone(),
                jobs.clone(),
                config.deletion_grace(),
            )),
            metrics: Arc::new(MetricsRegistry::new()),
            web_push_provider,
            alimtalk_provider,
            notifications,
            sealer,
            consents: Arc::new(ConsentService::new(
                lookup_hash,
                notification_preferences.clone(),
            )),
            admin: Arc::new(AdminService::new(
                audit.clone(),
                jobs.clone(),
                loyalty_stamps.clone(),
                config.admin_preview_ttl(),
            )),
            rate_limiter: Arc::new(RateLimiter::new(redis.clone())),
            wallet: Arc::new(WalletService::new()),
            jobs,
            campaigns,
            redemptions,
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
