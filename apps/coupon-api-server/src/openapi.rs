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
    ),
    components(schemas(
        crate::error::ErrorEnvelope,
        crate::error::ErrorBody,
        crate::error::FieldError,
        crate::http::health::LiveResponse,
        crate::http::health::ReadyResponse,
        crate::http::health::ComponentHealth,
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
        (name = "admin", description = "거래 탐색과 보정 미리보기 (§11.5)"),
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
