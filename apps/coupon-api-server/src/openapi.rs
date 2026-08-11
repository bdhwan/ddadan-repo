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
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness and readiness probes"),
        (name = "users", description = "Account bootstrap, profile and roles"),
        (name = "consents", description = "Terms and channel consent"),
        (name = "stores", description = "Store draft, edit and review submission"),
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
