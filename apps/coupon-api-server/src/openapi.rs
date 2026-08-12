//! OpenAPI specification (§10.1).
//!
//! `cargo run --bin coupon-api -- --dump-openapi` writes `openapi.json` next to this
//! crate. The Angular apps generate their TypeScript DTOs from that file, so the schema
//! here is the contract between the server and all three front ends.

use axum::Router;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::state::AppState;

/// Where `--dump-openapi` writes, relative to the crate root.
pub const SPEC_FILE: &str = "openapi.json";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "DDADAN Coupon API",
        version = "0.1.0",
        description = "상점별 쿠폰·도장 발급 시스템 API. \
                       모든 경로는 `/api/coupon/v1` 아래에 있으며, JSON 필드는 snake_case, \
                       식별자는 UUID 문자열, 시각은 RFC 3339 UTC, 금액은 원 단위 정수입니다.",
    ),
    servers(
        (url = "http://localhost:7810", description = "Local development"),
    ),
    paths(
        crate::http::health::live,
        crate::http::health::ready,
        crate::auth::kakao::routes::authorize,
        crate::auth::kakao::routes::callback,
        crate::auth::kakao::routes::exchange,
        crate::auth::kakao::routes::unlink_webhook,
        crate::auth::kakao::routes::list_auth_links,
        crate::auth::kakao::routes::link_kakao,
        crate::auth::kakao::routes::unlink_kakao,
        crate::users::routes::bootstrap,
        crate::users::routes::get_me,
        crate::users::routes::patch_me,
        crate::users::routes::get_roles,
        crate::consents::routes::get_consents,
        crate::consents::routes::post_consents,
        crate::stores::routes::get_store,
        crate::stores::routes::create_store,
        crate::stores::routes::patch_store,
        crate::stores::routes::submit_review,
        crate::catalog::routes::list_items,
        crate::catalog::routes::create_item,
        crate::catalog::routes::patch_item,
        crate::catalog::routes::list_categories,
        crate::catalog::routes::create_category,
        crate::catalog::routes::patch_category,
        crate::loyalty::routes::list_policies,
        crate::loyalty::routes::create_policy,
        crate::loyalty::routes::patch_policy,
        crate::loyalty::routes::publish_policy,
        crate::loyalty::routes::resolve_scan,
        crate::loyalty::routes::preview_stamp_transaction,
        crate::loyalty::routes::confirm_stamp_transaction,
        crate::loyalty::routes::void_stamp_transaction,
        crate::qr::routes::issue_qr_token,
        crate::wallet::routes::get_stamps,
        crate::wallet::routes::list_coupons,
        crate::wallet::routes::get_coupon,
        crate::admin::routes::get_transaction,
        crate::admin::routes::preview_adjustment,
        crate::campaigns::routes::list_campaigns,
        crate::campaigns::routes::get_campaign,
        crate::campaigns::routes::create_campaign,
        crate::campaigns::routes::patch_campaign,
        crate::campaigns::routes::estimate_campaign,
        crate::campaigns::routes::publish_campaign,
        crate::campaigns::routes::pause_campaign,
        crate::campaigns::routes::resume_campaign,
        crate::campaigns::routes::cancel_campaign,
        crate::campaigns::routes::claim_campaign,
        crate::redemptions::routes::preview_redemption,
        crate::redemptions::routes::confirm_redemption,
        crate::redemptions::routes::cancel_redemption,
        crate::admin::routes::approve_adjustment,
        crate::admin::routes::emergency_stop_campaign,
        crate::admin::routes::revoke_campaign_coupons,
        crate::admin::routes::list_jobs,
        crate::admin::routes::get_job,
        crate::admin::routes::retry_job,
        // Phase 4 — 알림·운영 (§15, §11.5, §19, §17.3).
        crate::notifications::routes::list_notifications,
        crate::notifications::routes::update_notifications,
        crate::notifications::routes::list_push_subscriptions,
        crate::notifications::routes::register_push_subscription,
        crate::notifications::routes::delete_push_subscription,
        crate::notifications::routes::provider_callback,
        crate::analytics::routes::get_analytics,
        crate::admin::routes::list_cases,
        crate::admin::routes::get_case,
        crate::admin::routes::create_case,
        crate::admin::routes::patch_case,
        crate::admin::routes::suspend_user,
        crate::admin::routes::revoke_sessions,
        crate::admin::routes::list_audit_logs,
        crate::admin::routes::list_store_reviews,
        crate::admin::routes::decide_store_review,
        crate::admin::routes::get_metrics,
        crate::privacy::routes::list_retention_policies,
        crate::privacy::routes::patch_retention_policy,
        crate::privacy::routes::list_erasures,
        crate::privacy::routes::request_erasure,
        crate::privacy::routes::reapply_erasures,
    ),
    components(schemas(
        crate::error::ErrorEnvelope,
        crate::error::ErrorBody,
        crate::error::FieldError,
        crate::http::health::LiveResponse,
        crate::http::health::ReadyResponse,
        crate::http::health::ComponentHealth,
        crate::auth::kakao::AuthorizeStart,
        crate::auth::kakao::CallbackResult,
        crate::auth::kakao::SignInResult,
        crate::auth::kakao::AuthLink,
        crate::auth::kakao::UnlinkAck,
        crate::auth::kakao::UnlinkOutcome,
        crate::auth::kakao::routes::ExchangeRequest,
        crate::auth::kakao::routes::UnlinkWebhookBody,
        crate::users::UserProfile,
        crate::users::UserStatus,
        crate::users::AccountRole,
        crate::users::RoleGrant,
        crate::users::RolesResponse,
        crate::users::BootstrapRequest,
        crate::users::UpdateProfileRequest,
        crate::consents::ConsentScope,
        crate::consents::ConsentAction,
        crate::consents::ConsentState,
        crate::consents::ConsentChange,
        crate::consents::ConsentsResponse,
        crate::consents::UpdateConsentsRequest,
        crate::stores::StoreStatus,
        crate::stores::ReviewStatus,
        crate::stores::StoreResponse,
        crate::stores::StoreReviewResponse,
        crate::stores::CreateStoreRequest,
        crate::stores::UpdateStoreRequest,
        crate::stores::BusinessProfileInput,
        crate::stores::SubmitReviewRequest,
        crate::catalog::ResourceStatus,
        crate::catalog::CatalogCategory,
        crate::catalog::CatalogItem,
        crate::catalog::CatalogCategoriesResponse,
        crate::catalog::CatalogItemsResponse,
        crate::catalog::CreateCategoryRequest,
        crate::catalog::UpdateCategoryRequest,
        crate::catalog::CreateItemRequest,
        crate::catalog::UpdateItemRequest,
        crate::catalog::OrderItemInput,
        crate::loyalty::PolicyStatus,
        crate::loyalty::BenefitType,
        crate::loyalty::PolicyRules,
        crate::loyalty::RewardDefinition,
        crate::loyalty::LoyaltyPolicy,
        crate::loyalty::LoyaltyPoliciesResponse,
        crate::loyalty::CreatePolicyRequest,
        crate::loyalty::UpdatePolicyRequest,
        crate::loyalty::PublishPolicyRequest,
        crate::loyalty::ScanRequest,
        crate::loyalty::ScanResolution,
        crate::loyalty::StampPreviewRequest,
        crate::loyalty::StampPreview,
        crate::loyalty::ConfirmStampRequest,
        crate::loyalty::StampTransaction,
        crate::loyalty::StampTransactionStatus,
        crate::loyalty::VoidStampRequest,
        crate::loyalty::OrderInput,
        crate::loyalty::PolicySummary,
        crate::loyalty::PreviewIssue,
        crate::loyalty::PseudonymousCustomer,
        crate::loyalty::StampBoard,
        crate::loyalty::IssuedReward,
        crate::qr::QrTokenResponse,
        crate::wallet::CouponStatus,
        crate::wallet::WalletFilter,
        crate::wallet::WalletCoupon,
        crate::wallet::WalletCouponDetail,
        crate::wallet::CouponStatusEvent,
        crate::wallet::WalletStampBoard,
        crate::wallet::WalletStampsResponse,
        crate::admin::AdminTransactionDetail,
        crate::admin::AdminLedgerEntry,
        crate::admin::AdminRewardSummary,
        crate::admin::TimelineEvent,
        crate::admin::AdjustmentType,
        crate::admin::AdjustmentPreviewRequest,
        crate::admin::AdjustmentPreview,
        crate::admin::ProposedLedgerEntry,
        crate::admin::ApproveAdjustmentRequest,
        crate::admin::ApprovedAdjustment,
        crate::admin::routes::ReasonRequest,
        crate::admin::routes::RevokeJobRequest,
        crate::campaigns::Campaign,
        crate::campaigns::CampaignStatus,
        crate::campaigns::IssueMode,
        crate::campaigns::RevokePolicy,
        crate::campaigns::TotalQuantity,
        crate::campaigns::AudienceType,
        crate::campaigns::AudienceCriteria,
        crate::campaigns::CreateCampaignRequest,
        crate::campaigns::UpdateCampaignRequest,
        crate::campaigns::CancelCampaignRequest,
        crate::campaigns::PauseCampaignRequest,
        crate::campaigns::PublishEstimate,
        crate::campaigns::PublishedCampaign,
        crate::campaigns::ClaimedCoupon,
        crate::redemptions::Benefit,
        crate::redemptions::CouponConditions,
        crate::redemptions::LocalTimeRange,
        crate::redemptions::Discount,
        crate::redemptions::FreeItemAward,
        crate::redemptions::Reservation,
        crate::redemptions::ReservationRequest,
        crate::redemptions::ReservationStatus,
        crate::redemptions::Redemption,
        crate::redemptions::RedemptionStatus,
        crate::redemptions::ConfirmRedemptionRequest,
        crate::redemptions::CancelRedemptionRequest,
        crate::jobs::JobStatus,
        crate::jobs::JobType,
        crate::jobs::JobQuery,
        crate::jobs::JobSummary,
        crate::jobs::JobDetail,
        crate::jobs::JobAttempt,
        crate::jobs::EnqueuedJob,
        // Phase 4.
        crate::notifications::Notification,
        crate::notifications::NotificationChannel,
        crate::notifications::NotificationEvent,
        crate::notifications::DeliveryStatus,
        crate::notifications::NotificationAction,
        crate::notifications::UpdateNotificationsRequest,
        crate::notifications::NotificationUpdateResult,
        crate::notifications::PushSubscription,
        crate::notifications::PushSubscriptionsResponse,
        crate::notifications::RegisterPushSubscriptionRequest,
        crate::notifications::policy::NotificationPurpose,
        crate::notifications::policy::SuppressionReason,
        crate::notifications::delivery::CallbackOutcome,
        crate::notifications::routes::ProviderCallbackBody,
        crate::notifications::routes::CallbackAck,
        crate::analytics::AggregationState,
        crate::analytics::DailyCounts,
        crate::analytics::DailyMetrics,
        crate::analytics::AnalyticsResponse,
        crate::admin::operations::AdminCase,
        crate::admin::operations::CreateCaseRequest,
        crate::admin::operations::UpdateCaseRequest,
        crate::admin::operations::SuspendUserRequest,
        crate::admin::operations::UserSanction,
        crate::admin::operations::RevokeSessionsRequest,
        crate::admin::operations::SessionRevocation,
        crate::admin::operations::AuditLogEntry,
        crate::admin::routes::ReviewDecisionRequest,
        crate::stores::StoreReviewQueueEntry,
        crate::privacy::RetentionPolicy,
        crate::privacy::RetentionPoliciesResponse,
        crate::privacy::UpdateRetentionPolicyRequest,
        crate::privacy::RequestErasureRequest,
        crate::privacy::ErasureRecord,
        crate::privacy::ErasuresResponse,
        crate::privacy::ReapplyResult,
        crate::http::metrics::ProcessMetrics,
        crate::http::metrics::QueueMetrics,
        crate::http::metrics::NotificationMetrics,
        crate::http::metrics::OperationalMetrics,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness and readiness probes"),
        (name = "users", description = "Account bootstrap, profile and roles"),
        (name = "consents", description = "Terms and channel consent"),
        (name = "stores", description = "Store draft, edit and review submission"),
        (name = "catalog", description = "품목·카테고리 (§8.3)"),
        (name = "loyalty", description = "도장 정책 버전, 스캔, 적립 원장 (§8.1, §13.1)"),
        (name = "qr", description = "회전형 QR 발급 (§16.2)"),
        (name = "wallet", description = "소비자 지갑: 도장판과 리워드 쿠폰 (§6.2)"),
        (name = "campaigns", description = "할인 캠페인 작성·게시·선착순 발급 (§8.2, §11.4, §13.2)"),
        (name = "redemptions", description = "쿠폰 사용 예약·승인·취소 (§13.3, §8.6)"),
        (name = "admin", description = "거래 탐색, 보정, 캠페인 긴급 중단, 작업 큐, 민원·제재·감사 (§11.5)"),
        (name = "notifications", description = "앱 내 알림, Web Push 구독, 제공자 콜백 (§15)"),
        (name = "analytics", description = "점주 기간별 지표와 개인정보 최소 기준 (§6.3, §19)"),
    ),
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "firebase",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some("Firebase ID token"))
                    .build(),
            ),
        );
    }
}

/// Serialised spec, pretty-printed so a diff of the checked-in file is readable.
pub fn spec_json() -> String {
    ApiDoc::openapi()
        .to_pretty_json()
        .expect("the OpenAPI document is always serialisable")
}

/// Swagger UI, served alongside the API for local exploration.
pub fn swagger_router() -> Router<AppState> {
    SwaggerUi::new("/api/coupon/v1/docs")
        .url("/api/coupon/v1/openapi.json", ApiDoc::openapi())
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_describes_the_phase_one_surface() {
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");

        let paths = spec["paths"].as_object().expect("paths object");
        for path in [
            "/api/coupon/v1/health/live",
            "/api/coupon/v1/health/ready",
            "/api/coupon/v1/users/bootstrap",
            "/api/coupon/v1/me",
            "/api/coupon/v1/me/roles",
            "/api/coupon/v1/me/consents",
            "/api/coupon/v1/owner/store",
            "/api/coupon/v1/owner/store/submit-review",
        ] {
            assert!(paths.contains_key(path), "{path} must be documented");
        }
    }

    #[test]
    fn the_spec_describes_the_phase_two_surface() {
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let paths = spec["paths"].as_object().expect("paths object");

        // Every §11.3/§11.4/§11.5 path this phase implements. The Angular clients are
        // generated from this file, so a missing path is a missing feature downstream.
        for path in [
            "/api/coupon/v1/owner/catalog/items",
            "/api/coupon/v1/owner/catalog/items/{item_id}",
            "/api/coupon/v1/owner/catalog/categories",
            "/api/coupon/v1/owner/loyalty-policies",
            "/api/coupon/v1/owner/loyalty-policies/{policy_id}",
            "/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish",
            "/api/coupon/v1/owner/scan/resolve",
            "/api/coupon/v1/owner/stamp-transactions/preview",
            "/api/coupon/v1/owner/stamp-transactions",
            "/api/coupon/v1/owner/stamp-transactions/{transaction_id}/void",
            "/api/coupon/v1/me/qr-tokens",
            "/api/coupon/v1/me/wallet/stamps",
            "/api/coupon/v1/me/wallet/coupons",
            "/api/coupon/v1/me/wallet/coupons/{coupon_id}",
            "/api/coupon/v1/admin/transactions/{transaction_id}",
            "/api/coupon/v1/admin/adjustments/preview",
        ] {
            assert!(paths.contains_key(path), "{path} must be documented");
        }
    }

    #[test]
    fn the_spec_describes_the_phase_three_surface() {
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let paths = spec["paths"].as_object().expect("paths object");

        // §11.3's claim, all of §11.4's campaign and redemption rows, and the §11.5
        // administrative surface Phase 3 adds.
        for path in [
            "/api/coupon/v1/owner/campaigns",
            "/api/coupon/v1/owner/campaigns/{campaign_id}",
            "/api/coupon/v1/owner/campaigns/{campaign_id}/publish",
            "/api/coupon/v1/owner/campaigns/{campaign_id}/pause",
            "/api/coupon/v1/owner/campaigns/{campaign_id}/resume",
            "/api/coupon/v1/owner/campaigns/{campaign_id}/cancel",
            "/api/coupon/v1/campaigns/{campaign_id}/claims",
            "/api/coupon/v1/owner/redemptions/preview",
            "/api/coupon/v1/owner/redemptions/{reservation_id}/confirm",
            "/api/coupon/v1/owner/redemptions/{reservation_id}/cancel",
            "/api/coupon/v1/admin/adjustments",
            "/api/coupon/v1/admin/campaigns/{campaign_id}/emergency-stop",
            "/api/coupon/v1/admin/campaigns/{campaign_id}/revoke-job",
            "/api/coupon/v1/admin/jobs",
            "/api/coupon/v1/admin/jobs/{job_id}",
            "/api/coupon/v1/admin/jobs/{job_id}/retry",
        ] {
            assert!(paths.contains_key(path), "{path} must be documented");
        }
    }

    #[test]
    fn the_spec_describes_the_phase_four_surface() {
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let paths = spec["paths"].as_object().expect("paths object");

        // §15's consumer surface, §11.5's remaining administrative rows, §6.3's dashboard
        // and §17.3's retention controls.
        for path in [
            "/api/coupon/v1/me/notifications",
            "/api/coupon/v1/me/push-subscriptions",
            "/api/coupon/v1/me/push-subscriptions/{subscription_id}",
            "/api/coupon/v1/notifications/callbacks/{provider}",
            "/api/coupon/v1/owner/analytics",
            "/api/coupon/v1/admin/cases",
            "/api/coupon/v1/admin/cases/{case_id}",
            "/api/coupon/v1/admin/users/{user_id}/suspend",
            "/api/coupon/v1/admin/users/{user_id}/revoke-sessions",
            "/api/coupon/v1/admin/audit-logs",
            "/api/coupon/v1/admin/store-reviews",
            "/api/coupon/v1/admin/store-reviews/{review_id}/decision",
            "/api/coupon/v1/admin/metrics",
            "/api/coupon/v1/admin/retention-policies",
            "/api/coupon/v1/admin/privacy/erasures",
        ] {
            assert!(paths.contains_key(path), "{path} must be documented");
        }
    }

    #[test]
    fn the_campaign_list_is_paginated_in_the_contract() {
        // §11.1: 목록은 커서 페이지네이션. A generated client that cannot page cannot be
        // fixed without a breaking change, so the shape is asserted here.
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let parameters = spec["paths"]["/api/coupon/v1/owner/campaigns"]["get"]["parameters"]
            .as_array()
            .expect("query parameters");

        let names: Vec<&str> = parameters
            .iter()
            .filter_map(|parameter| parameter["name"].as_str())
            .collect();
        assert!(names.contains(&"limit"), "{names:?}");
        assert!(names.contains(&"cursor"), "{names:?}");
    }

    #[test]
    fn a_delivery_status_is_never_reported_as_a_read_receipt() {
        // §15.4: DELIVERED 는 제공자 정의를 따르며 사용자가 읽었다는 뜻이 아니다. The
        // contract must not offer a `READ` state for a client to misinterpret.
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let rendered = spec["components"]["schemas"]["DeliveryStatus"].to_string();

        assert!(rendered.contains("DELIVERED"), "{rendered}");
        assert!(rendered.contains("SUPPRESSED"), "{rendered}");
        assert!(!rendered.contains("READ"), "{rendered}");
    }

    #[test]
    fn unlimited_quantity_is_a_distinct_shape_in_the_contract() {
        // §8.4: 총수량 무제한은 운영 상한을 가진 별도 표현이다. A generated client must
        // not be able to express "unlimited" as a null.
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let schema = &spec["components"]["schemas"]["TotalQuantity"];

        let rendered = schema.to_string();
        assert!(rendered.contains("LIMITED"), "{rendered}");
        assert!(rendered.contains("UNLIMITED"), "{rendered}");
        assert!(rendered.contains("operational_cap"), "{rendered}");
    }

    #[test]
    fn the_qr_contract_never_promises_a_raw_nonce() {
        // §16.2: the nonce travels inside the signed token and nowhere else. If it ever
        // appeared as its own response field, every client would be tempted to store it.
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let properties = spec["components"]["schemas"]["QrTokenResponse"]["properties"]
            .as_object()
            .expect("QrTokenResponse properties");

        assert!(properties.contains_key("token"));
        assert!(properties.contains_key("fallback_code"));
        assert!(!properties.contains_key("nonce"));
    }

    #[test]
    fn the_error_envelope_is_part_of_the_contract() {
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");
        let schemas = spec["components"]["schemas"]
            .as_object()
            .expect("schemas object");

        assert!(schemas.contains_key("ErrorEnvelope"));
        assert!(schemas.contains_key("FieldError"));
    }

    #[test]
    fn bearer_auth_is_declared() {
        let spec: serde_json::Value =
            serde_json::from_str(&spec_json()).expect("spec is valid JSON");

        assert_eq!(
            spec["components"]["securitySchemes"]["firebase"]["scheme"],
            "bearer"
        );
    }
}
