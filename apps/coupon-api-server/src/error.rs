//! The single error type every handler returns, and its wire format (§11.1).
//!
//! An [`ErrorCode`] carries its own HTTP status and default Korean message, so a handler
//! only picks the code. That keeps the status mapping in one place where it can be
//! tested, instead of scattered across handlers.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use crate::http::request_id;

/// Every error the API can return. New variants must declare a status and a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // 400 — shape and field validation.
    InvalidRequest,
    ValidationFailed,
    IdempotencyKeyRequired,
    IdempotencyKeyInvalid,
    InvalidCursor,
    InvalidVersion,
    /// §6.1: the user declined on Kakao's consent screen. Not a failure — the app should
    /// simply return to `/login` — so it must not be reported as one.
    KakaoLoginCancelled,

    // 401 — authentication.
    Unauthenticated,
    TokenExpired,
    TokenInvalid,
    /// §15.4: a provider callback whose signature or freshness window did not hold.
    /// Deliberately one code for both, so a caller cannot probe which half failed.
    WebhookSignatureInvalid,
    /// §6.1: 카카오 오류는 취소, 일시 장애, 보안 검증 실패 셋으로만 구분한다. This is the
    /// third — a `state`/`nonce` mismatch, an `id_token` that did not verify, or a code
    /// presented twice. One code covers all of them so an attacker learns nothing about
    /// *which* check refused them; the internal detail says which, for the operator.
    KakaoSecurityCheckFailed,

    // 403 — role, account state, terms, origin.
    Forbidden,
    RoleRequired,
    AccountSuspended,
    AccountWithdrawn,
    ConsentRequired,
    ReauthenticationRequired,
    OriginNotAllowed,

    // 403 — the store itself may not act.
    StoreNotActive,
    /// §3.3: 원장 대량 보정은 요청자와 승인자가 달라야 한다.
    ApprovalSeparationRequired,

    // 404 — absent, or not owned by the caller.
    NotFound,
    StoreNotFound,
    UserNotFound,
    CatalogItemNotFound,
    LoyaltyPolicyNotFound,
    TransactionNotFound,
    CouponNotFound,
    CampaignNotFound,
    ReservationNotFound,
    NotificationNotFound,
    /// §11.2 `DELETE /me/auth-links/kakao` on an account that has no Kakao link.
    AuthLinkNotFound,
    /// §11.5: 민원·보안 사건.
    CaseNotFound,
    /// §17.3: 보존기간 설정 항목.
    RetentionPolicyNotFound,

    // 409 — state, version and quantity contention.
    Conflict,
    VersionConflict,
    IdempotencyKeyReused,
    IdempotencyRequestInProgress,
    StoreAlreadyExists,
    StoreSlugTaken,
    ReviewAlreadyPending,
    /// §15: the losing side of two scans of the same QR.
    QrAlreadyUsed,
    /// STAMP-003: an identical order arrived inside the policy's duplicate window.
    DuplicateTransactionSuspected,
    PolicyAlreadyScheduled,
    PreviewExpired,
    /// CAMPAIGN-004: the last coupon went to somebody else.
    CampaignSoldOut,
    /// §15: the losing side of two reservations of the same coupon.
    CouponNotAvailable,
    /// REDEEM-002: the two minutes ran out before the owner confirmed.
    ReservationExpired,
    /// REDEEM-002: 같은 점주 세션은 동시에 하나의 사용 예약만 가질 수 있다.
    ReservationAlreadyActive,
    /// §5.4: 주문 1건에 혜택 1개.
    OrderAlreadyDiscounted,
    /// ADMIN-002: 한 회원에게 동시에 유효한 제재는 하나다.
    SanctionAlreadyActive,
    /// AUTH-003: 이 카카오 계정은 이미 다른 내부 회원에 연결되어 있다. 자동 이전하지
    /// 않고 고객센터 본인 확인 절차로 보낸다.
    AuthLinkAlreadyClaimed,
    /// AUTH-002: 다른 로그인 수단이 없으면 마지막 연결을 스스로 끊게 두지 않는다 —
    /// 끊는 순간 다시 들어올 방법이 없어진다.
    LastAuthLinkCannotBeRemoved,

    // 422 — well-formed but the business rule says no.
    UnprocessableRequest,
    StoreNotReadyForReview,
    InvalidStateTransition,
    /// SEC-002 deliberately collapses forged, malformed and unknown tokens into one code.
    QrTokenInvalid,
    QrTokenExpired,
    NoActivePolicy,
    PolicyNotEditable,
    MinimumOrderNotMet,
    ItemNotEligible,
    DailyLimitExceeded,
    VoidWindowExpired,
    RequiresAdminReview,
    /// CAMPAIGN-006: 중지 중 선착순 요청.
    CampaignPaused,
    /// The campaign is not in a state that issues coupons at all.
    CampaignNotIssuing,
    /// CAMPAIGN-008: 이미 발급된 인스턴스에 소급하는 수정은 거부한다.
    CampaignNotEditable,
    /// CAMPAIGN-008: 이미 발급·예약된 수량 미만으로 낮출 수 없다.
    QuantityBelowIssued,
    /// The consumer is not in the campaign's audience.
    AudienceNotEligible,
    /// REDEEM-003: 아직 사용 시작 전.
    CouponNotYetUsable,
    /// REDEEM-003: 사용 종료 이후.
    CouponExpired,
    /// REDEEM-003: 사용 가능 요일·시간대 아님.
    CouponOutsideUsageWindow,
    /// §3.3: 고위험 관리자 작업은 사건 티켓을 요구한다.
    CaseReferenceRequired,
    /// §17.3 / ADMIN-006: 법정 보존 또는 분쟁 hold 가 걸린 대상은 파기하지 않는다.
    LegalHoldActive,
    /// §19: 집단 크기가 개인정보 최소 기준 미만이면 세부 구분을 제공하지 않는다.
    CohortTooSmall,

    // 429 / 503.
    RateLimited,
    ServiceUnavailable,
    DependencyUnavailable,
}

impl ErrorCode {
    /// The wire value. Stable: clients branch on it.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidRequest => "INVALID_REQUEST",
            ErrorCode::ValidationFailed => "VALIDATION_FAILED",
            ErrorCode::IdempotencyKeyRequired => "IDEMPOTENCY_KEY_REQUIRED",
            ErrorCode::IdempotencyKeyInvalid => "IDEMPOTENCY_KEY_INVALID",
            ErrorCode::InvalidCursor => "INVALID_CURSOR",
            ErrorCode::InvalidVersion => "INVALID_VERSION",
            ErrorCode::KakaoLoginCancelled => "KAKAO_LOGIN_CANCELLED",
            ErrorCode::KakaoSecurityCheckFailed => "KAKAO_SECURITY_CHECK_FAILED",
            ErrorCode::Unauthenticated => "UNAUTHENTICATED",
            ErrorCode::TokenExpired => "TOKEN_EXPIRED",
            ErrorCode::TokenInvalid => "TOKEN_INVALID",
            ErrorCode::Forbidden => "FORBIDDEN",
            ErrorCode::RoleRequired => "ROLE_REQUIRED",
            ErrorCode::AccountSuspended => "ACCOUNT_SUSPENDED",
            ErrorCode::AccountWithdrawn => "ACCOUNT_WITHDRAWN",
            ErrorCode::ConsentRequired => "CONSENT_REQUIRED",
            ErrorCode::ReauthenticationRequired => "REAUTHENTICATION_REQUIRED",
            ErrorCode::OriginNotAllowed => "ORIGIN_NOT_ALLOWED",
            ErrorCode::StoreNotActive => "STORE_NOT_ACTIVE",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::StoreNotFound => "STORE_NOT_FOUND",
            ErrorCode::UserNotFound => "USER_NOT_FOUND",
            ErrorCode::CatalogItemNotFound => "CATALOG_ITEM_NOT_FOUND",
            ErrorCode::LoyaltyPolicyNotFound => "LOYALTY_POLICY_NOT_FOUND",
            ErrorCode::TransactionNotFound => "TRANSACTION_NOT_FOUND",
            ErrorCode::CouponNotFound => "COUPON_NOT_FOUND",
            ErrorCode::CampaignNotFound => "CAMPAIGN_NOT_FOUND",
            ErrorCode::ReservationNotFound => "RESERVATION_NOT_FOUND",
            ErrorCode::ApprovalSeparationRequired => "APPROVAL_SEPARATION_REQUIRED",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::VersionConflict => "VERSION_CONFLICT",
            ErrorCode::IdempotencyKeyReused => "IDEMPOTENCY_KEY_REUSED",
            ErrorCode::IdempotencyRequestInProgress => "IDEMPOTENCY_REQUEST_IN_PROGRESS",
            ErrorCode::StoreAlreadyExists => "STORE_ALREADY_EXISTS",
            ErrorCode::StoreSlugTaken => "STORE_SLUG_TAKEN",
            ErrorCode::ReviewAlreadyPending => "REVIEW_ALREADY_PENDING",
            ErrorCode::QrAlreadyUsed => "QR_ALREADY_USED",
            ErrorCode::DuplicateTransactionSuspected => "DUPLICATE_TRANSACTION_SUSPECTED",
            ErrorCode::PolicyAlreadyScheduled => "POLICY_ALREADY_SCHEDULED",
            ErrorCode::PreviewExpired => "PREVIEW_EXPIRED",
            ErrorCode::CampaignSoldOut => "CAMPAIGN_SOLD_OUT",
            ErrorCode::CouponNotAvailable => "COUPON_NOT_AVAILABLE",
            ErrorCode::ReservationExpired => "RESERVATION_EXPIRED",
            ErrorCode::ReservationAlreadyActive => "RESERVATION_ALREADY_ACTIVE",
            ErrorCode::OrderAlreadyDiscounted => "ORDER_ALREADY_DISCOUNTED",
            ErrorCode::UnprocessableRequest => "UNPROCESSABLE_REQUEST",
            ErrorCode::StoreNotReadyForReview => "STORE_NOT_READY_FOR_REVIEW",
            ErrorCode::InvalidStateTransition => "INVALID_STATE_TRANSITION",
            ErrorCode::QrTokenInvalid => "QR_TOKEN_INVALID",
            ErrorCode::QrTokenExpired => "QR_TOKEN_EXPIRED",
            ErrorCode::NoActivePolicy => "NO_ACTIVE_POLICY",
            ErrorCode::PolicyNotEditable => "POLICY_NOT_EDITABLE",
            ErrorCode::MinimumOrderNotMet => "MINIMUM_ORDER_NOT_MET",
            ErrorCode::ItemNotEligible => "ITEM_NOT_ELIGIBLE",
            ErrorCode::DailyLimitExceeded => "DAILY_LIMIT_EXCEEDED",
            ErrorCode::VoidWindowExpired => "VOID_WINDOW_EXPIRED",
            ErrorCode::RequiresAdminReview => "REQUIRES_ADMIN_REVIEW",
            ErrorCode::CampaignPaused => "CAMPAIGN_PAUSED",
            ErrorCode::CampaignNotIssuing => "CAMPAIGN_NOT_ISSUING",
            ErrorCode::CampaignNotEditable => "CAMPAIGN_NOT_EDITABLE",
            ErrorCode::QuantityBelowIssued => "QUANTITY_BELOW_ISSUED",
            ErrorCode::AudienceNotEligible => "AUDIENCE_NOT_ELIGIBLE",
            ErrorCode::CouponNotYetUsable => "COUPON_NOT_YET_USABLE",
            ErrorCode::CouponExpired => "COUPON_EXPIRED",
            ErrorCode::CouponOutsideUsageWindow => "COUPON_OUTSIDE_USAGE_WINDOW",
            ErrorCode::WebhookSignatureInvalid => "WEBHOOK_SIGNATURE_INVALID",
            ErrorCode::NotificationNotFound => "NOTIFICATION_NOT_FOUND",
            ErrorCode::AuthLinkNotFound => "AUTH_LINK_NOT_FOUND",
            ErrorCode::CaseNotFound => "CASE_NOT_FOUND",
            ErrorCode::RetentionPolicyNotFound => "RETENTION_POLICY_NOT_FOUND",
            ErrorCode::SanctionAlreadyActive => "SANCTION_ALREADY_ACTIVE",
            ErrorCode::AuthLinkAlreadyClaimed => "AUTH_LINK_ALREADY_CLAIMED",
            ErrorCode::LastAuthLinkCannotBeRemoved => "LAST_AUTH_LINK_CANNOT_BE_REMOVED",
            ErrorCode::CaseReferenceRequired => "CASE_REFERENCE_REQUIRED",
            ErrorCode::LegalHoldActive => "LEGAL_HOLD_ACTIVE",
            ErrorCode::CohortTooSmall => "COHORT_TOO_SMALL",
            ErrorCode::RateLimited => "RATE_LIMITED",
            ErrorCode::ServiceUnavailable => "SERVICE_UNAVAILABLE",
            ErrorCode::DependencyUnavailable => "DEPENDENCY_UNAVAILABLE",
        }
    }

    /// HTTP status mapping (§11.1). This is the authoritative table.
    pub fn status(self) -> StatusCode {
        match self {
            // 400: the request could not be parsed or a field is malformed.
            ErrorCode::InvalidRequest
            | ErrorCode::ValidationFailed
            | ErrorCode::IdempotencyKeyRequired
            | ErrorCode::IdempotencyKeyInvalid
            | ErrorCode::InvalidCursor
            | ErrorCode::InvalidVersion
            | ErrorCode::KakaoLoginCancelled => StatusCode::BAD_REQUEST,

            // 401: no credential, or one we cannot accept.
            ErrorCode::Unauthenticated
            | ErrorCode::TokenExpired
            | ErrorCode::TokenInvalid
            | ErrorCode::WebhookSignatureInvalid
            | ErrorCode::KakaoSecurityCheckFailed => StatusCode::UNAUTHORIZED,

            // 403: authenticated, but role / account state / terms / origin forbid it.
            ErrorCode::Forbidden
            | ErrorCode::RoleRequired
            | ErrorCode::AccountSuspended
            | ErrorCode::AccountWithdrawn
            | ErrorCode::ConsentRequired
            | ErrorCode::ReauthenticationRequired
            | ErrorCode::OriginNotAllowed
            | ErrorCode::StoreNotActive
            | ErrorCode::ApprovalSeparationRequired => StatusCode::FORBIDDEN,

            // 404: absent, or present but not owned by the caller — indistinguishable on
            // purpose, so ownership cannot be probed.
            ErrorCode::NotFound
            | ErrorCode::StoreNotFound
            | ErrorCode::UserNotFound
            | ErrorCode::CatalogItemNotFound
            | ErrorCode::LoyaltyPolicyNotFound
            | ErrorCode::TransactionNotFound
            | ErrorCode::CouponNotFound
            | ErrorCode::CampaignNotFound
            | ErrorCode::ReservationNotFound
            | ErrorCode::NotificationNotFound
            | ErrorCode::AuthLinkNotFound
            | ErrorCode::CaseNotFound
            | ErrorCode::RetentionPolicyNotFound => StatusCode::NOT_FOUND,

            // 409: state, version or quantity contention.
            ErrorCode::Conflict
            | ErrorCode::VersionConflict
            | ErrorCode::IdempotencyKeyReused
            | ErrorCode::IdempotencyRequestInProgress
            | ErrorCode::StoreAlreadyExists
            | ErrorCode::StoreSlugTaken
            | ErrorCode::ReviewAlreadyPending
            | ErrorCode::QrAlreadyUsed
            | ErrorCode::DuplicateTransactionSuspected
            | ErrorCode::PolicyAlreadyScheduled
            | ErrorCode::PreviewExpired
            | ErrorCode::CampaignSoldOut
            | ErrorCode::CouponNotAvailable
            | ErrorCode::ReservationExpired
            | ErrorCode::ReservationAlreadyActive
            | ErrorCode::OrderAlreadyDiscounted
            | ErrorCode::SanctionAlreadyActive
            | ErrorCode::AuthLinkAlreadyClaimed
            | ErrorCode::LastAuthLinkCannotBeRemoved => StatusCode::CONFLICT,

            // 422: shape is fine, the business condition is not met.
            ErrorCode::UnprocessableRequest
            | ErrorCode::StoreNotReadyForReview
            | ErrorCode::InvalidStateTransition
            | ErrorCode::QrTokenInvalid
            | ErrorCode::QrTokenExpired
            | ErrorCode::NoActivePolicy
            | ErrorCode::PolicyNotEditable
            | ErrorCode::MinimumOrderNotMet
            | ErrorCode::ItemNotEligible
            | ErrorCode::DailyLimitExceeded
            | ErrorCode::VoidWindowExpired
            | ErrorCode::RequiresAdminReview
            | ErrorCode::CampaignPaused
            | ErrorCode::CampaignNotIssuing
            | ErrorCode::CampaignNotEditable
            | ErrorCode::QuantityBelowIssued
            | ErrorCode::AudienceNotEligible
            | ErrorCode::CouponNotYetUsable
            | ErrorCode::CouponExpired
            | ErrorCode::CouponOutsideUsageWindow
            | ErrorCode::CaseReferenceRequired
            | ErrorCode::LegalHoldActive
            | ErrorCode::CohortTooSmall => StatusCode::UNPROCESSABLE_ENTITY,

            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,

            ErrorCode::ServiceUnavailable | ErrorCode::DependencyUnavailable => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }

    /// Whether an unchanged retry can plausibly succeed.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::RateLimited
                | ErrorCode::ServiceUnavailable
                | ErrorCode::DependencyUnavailable
                | ErrorCode::IdempotencyRequestInProgress
                | ErrorCode::VersionConflict
        )
    }

    /// User-facing Korean default. Handlers override it when they can say more.
    pub fn default_message(self) -> &'static str {
        match self {
            ErrorCode::InvalidRequest => "요청 형식이 올바르지 않습니다.",
            ErrorCode::ValidationFailed => "입력값을 확인해 주세요.",
            ErrorCode::IdempotencyKeyRequired => "Idempotency-Key 헤더가 필요합니다.",
            ErrorCode::IdempotencyKeyInvalid => "Idempotency-Key 는 UUID 여야 합니다.",
            ErrorCode::InvalidCursor => "잘못된 페이지 커서입니다.",
            ErrorCode::InvalidVersion => "잘못된 버전 값입니다.",
            ErrorCode::KakaoLoginCancelled => "카카오 로그인이 취소되었습니다.",
            ErrorCode::KakaoSecurityCheckFailed => {
                "카카오 로그인의 보안 검증에 실패했습니다. 처음부터 다시 시도해 주세요."
            }
            ErrorCode::Unauthenticated => "로그인이 필요합니다.",
            ErrorCode::TokenExpired => "로그인이 만료되었습니다. 다시 로그인해 주세요.",
            ErrorCode::TokenInvalid => "인증 정보를 확인할 수 없습니다.",
            ErrorCode::Forbidden => "이 작업을 수행할 권한이 없습니다.",
            ErrorCode::RoleRequired => "필요한 권한이 없습니다.",
            ErrorCode::AccountSuspended => "이용이 제한된 계정입니다.",
            ErrorCode::AccountWithdrawn => "탈퇴한 계정입니다.",
            ErrorCode::ConsentRequired => "필수 약관에 동의해야 이용할 수 있습니다.",
            ErrorCode::ReauthenticationRequired => "보안을 위해 다시 로그인해 주세요.",
            ErrorCode::OriginNotAllowed => "허용되지 않은 요청 출처입니다.",
            ErrorCode::StoreNotActive => "현재 상점 상태에서는 처리할 수 없습니다.",
            ErrorCode::NotFound => "요청한 리소스를 찾을 수 없습니다.",
            ErrorCode::StoreNotFound => "상점을 찾을 수 없습니다.",
            ErrorCode::UserNotFound => "회원 정보를 찾을 수 없습니다.",
            ErrorCode::CatalogItemNotFound => "품목을 찾을 수 없습니다.",
            ErrorCode::LoyaltyPolicyNotFound => "도장 정책을 찾을 수 없습니다.",
            ErrorCode::TransactionNotFound => "거래를 찾을 수 없습니다.",
            ErrorCode::CouponNotFound => "쿠폰을 찾을 수 없습니다.",
            ErrorCode::CampaignNotFound => "캠페인을 찾을 수 없습니다.",
            ErrorCode::ReservationNotFound => "사용 예약을 찾을 수 없습니다.",
            ErrorCode::AuthLinkNotFound => "연결된 카카오 계정이 없습니다.",
            ErrorCode::AuthLinkAlreadyClaimed => {
                "이미 다른 회원에 연결된 카카오 계정입니다. 고객센터 본인 확인이 필요합니다."
            }
            ErrorCode::LastAuthLinkCannotBeRemoved => {
                "마지막 로그인 수단은 해제할 수 없습니다. 다른 로그인 수단을 먼저 추가해 주세요."
            }
            ErrorCode::ApprovalSeparationRequired => {
                "요청자와 승인자가 같을 수 없습니다. 다른 관리자의 승인이 필요합니다."
            }
            ErrorCode::Conflict => "현재 상태에서는 처리할 수 없습니다.",
            ErrorCode::VersionConflict => {
                "다른 곳에서 먼저 수정되었습니다. 새로고침 후 다시 시도해 주세요."
            }
            ErrorCode::IdempotencyKeyReused => {
                "같은 Idempotency-Key 로 다른 요청을 보낼 수 없습니다."
            }
            ErrorCode::IdempotencyRequestInProgress => {
                "같은 요청을 처리하고 있습니다. 잠시 후 다시 시도해 주세요."
            }
            ErrorCode::StoreAlreadyExists => "계정당 상점은 하나만 만들 수 있습니다.",
            ErrorCode::StoreSlugTaken => "이미 사용 중인 상점 주소입니다.",
            ErrorCode::ReviewAlreadyPending => "이미 검수 대기 중입니다.",
            ErrorCode::QrAlreadyUsed => "이미 사용된 QR 입니다. 새 QR 을 받아 주세요.",
            ErrorCode::DuplicateTransactionSuspected => {
                "조금 전 같은 조건의 거래가 있었습니다. 별도 주문이면 주문번호를 입력해 주세요."
            }
            ErrorCode::PolicyAlreadyScheduled => "이미 예약된 다음 버전이 있습니다.",
            ErrorCode::PreviewExpired => "미리보기가 만료되었습니다. 다시 조회해 주세요.",
            ErrorCode::CampaignSoldOut => "준비된 쿠폰이 모두 소진되었습니다.",
            ErrorCode::CouponNotAvailable => "현재 사용할 수 없는 쿠폰입니다.",
            ErrorCode::ReservationExpired => {
                "예약 시간이 지났습니다. 쿠폰을 다시 확인해 주세요."
            }
            ErrorCode::ReservationAlreadyActive => {
                "진행 중인 사용 예약이 있습니다. 먼저 마무리해 주세요."
            }
            ErrorCode::OrderAlreadyDiscounted => {
                "이 주문에는 이미 혜택이 사용되었습니다. 주문 1건에 혜택 1개만 사용할 수 있습니다."
            }
            ErrorCode::UnprocessableRequest => "요청 조건을 충족하지 않습니다.",
            ErrorCode::StoreNotReadyForReview => "검수 제출에 필요한 정보가 아직 부족합니다.",
            ErrorCode::InvalidStateTransition => "현재 상태에서는 변경할 수 없습니다.",
            // SEC-002: one message for forged, malformed, unknown and wrong-audience
            // tokens, so a probe learns nothing from the difference.
            ErrorCode::QrTokenInvalid => "QR 을 확인할 수 없습니다. 새 QR 을 받아 주세요.",
            ErrorCode::QrTokenExpired => "QR 이 만료되었습니다. 새 QR 을 받아 주세요.",
            ErrorCode::NoActivePolicy => "활성화된 도장 정책이 없습니다.",
            ErrorCode::PolicyNotEditable => {
                "활성 정책은 수정할 수 없습니다. 새 버전을 만들어 주세요."
            }
            ErrorCode::MinimumOrderNotMet => "최소 주문 금액에 미치지 않습니다.",
            ErrorCode::ItemNotEligible => "적립 대상 품목이 포함되어 있지 않습니다.",
            ErrorCode::DailyLimitExceeded => "오늘의 적립 한도를 모두 사용했습니다.",
            ErrorCode::VoidWindowExpired => "취소 가능 시간이 지났습니다.",
            ErrorCode::RequiresAdminReview => {
                "점주가 직접 처리할 수 없는 거래입니다. 고객센터 문의가 필요합니다."
            }
            ErrorCode::CampaignPaused => "지금은 쿠폰을 받을 수 없습니다. 잠시 후 다시 확인해 주세요.",
            ErrorCode::CampaignNotIssuing => "지금은 발급 중인 캠페인이 아닙니다.",
            ErrorCode::CampaignNotEditable => {
                "이미 발급된 쿠폰에 영향을 주는 항목은 수정할 수 없습니다. 새 캠페인을 만들어 주세요."
            }
            ErrorCode::QuantityBelowIssued => {
                "이미 발급·예약된 수량보다 적게 줄일 수 없습니다."
            }
            ErrorCode::AudienceNotEligible => "이 캠페인의 대상이 아닙니다.",
            ErrorCode::CouponNotYetUsable => "아직 사용 시작 전인 쿠폰입니다.",
            ErrorCode::CouponExpired => "사용 기간이 지난 쿠폰입니다.",
            ErrorCode::CouponOutsideUsageWindow => {
                "지금은 사용할 수 없는 쿠폰입니다. 사용 가능한 요일·시간을 확인해 주세요."
            }
            ErrorCode::WebhookSignatureInvalid => "콜백 서명을 확인할 수 없습니다.",
            ErrorCode::NotificationNotFound => "알림을 찾을 수 없습니다.",
            ErrorCode::CaseNotFound => "사건을 찾을 수 없습니다.",
            ErrorCode::RetentionPolicyNotFound => "보존 정책을 찾을 수 없습니다.",
            ErrorCode::SanctionAlreadyActive => "이미 진행 중인 제재가 있습니다.",
            ErrorCode::CaseReferenceRequired => "이 작업에는 사건 티켓이 필요합니다.",
            ErrorCode::LegalHoldActive => "법정 보존 또는 분쟁 보존이 적용된 대상입니다.",
            ErrorCode::CohortTooSmall => "대상 인원이 적어 세부 지표를 제공하지 않습니다.",
            ErrorCode::RateLimited => "요청이 너무 잦습니다. 잠시 후 다시 시도해 주세요.",
            ErrorCode::ServiceUnavailable => "일시적인 오류입니다. 잠시 후 다시 시도해 주세요.",
            ErrorCode::DependencyUnavailable => "일시적인 오류입니다. 잠시 후 다시 시도해 주세요.",
        }
    }
}

/// One invalid field, for a form to highlight.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FieldError {
    pub field: String,
    pub code: String,
    pub message: String,
}

impl FieldError {
    pub fn new(
        field: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub field_errors: Vec<FieldError>,
    /// Server-side detail. Logged, never serialised to the client.
    pub internal: Option<String>,
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            message: code.default_message().to_owned(),
            field_errors: Vec::new(),
            internal: None,
        }
    }

    pub fn with_message(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::new(code)
        }
    }

    pub fn with_fields(code: ErrorCode, field_errors: Vec<FieldError>) -> Self {
        Self {
            field_errors,
            ..Self::new(code)
        }
    }

    /// Attach detail that belongs in the log but not in the response.
    pub fn internal(mut self, detail: impl Into<String>) -> Self {
        self.internal = Some(detail.into());
        self
    }

    pub fn status(&self) -> StatusCode {
        self.code.status()
    }

    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound)
    }

    pub fn unauthenticated() -> Self {
        Self::new(ErrorCode::Unauthenticated)
    }

    pub fn forbidden() -> Self {
        Self::new(ErrorCode::Forbidden)
    }
}

/// Wire shape of a failure. Matches the example in §11.1 exactly.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub field_errors: Vec<FieldError>,
    pub retryable: bool,
    pub request_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();

        // 5xx means we broke something; record enough to debug it. 4xx is the client's
        // problem and stays at debug so a scanner cannot flood the error log.
        if status.is_server_error() {
            tracing::error!(
                error.code = self.code.as_str(),
                error.internal = self.internal.as_deref().unwrap_or_default(),
                "request failed"
            );
        } else {
            tracing::debug!(
                error.code = self.code.as_str(),
                error.internal = self.internal.as_deref().unwrap_or_default(),
                "request rejected"
            );
        }

        let body = ErrorEnvelope {
            error: ErrorBody {
                code: self.code.as_str().to_owned(),
                message: self.message,
                field_errors: self.field_errors,
                retryable: self.code.retryable(),
                request_id: request_id::current().unwrap_or_default(),
            },
        };

        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::RowNotFound => ApiError::not_found(),
            sqlx::Error::PoolTimedOut => {
                ApiError::new(ErrorCode::DependencyUnavailable).internal("postgres pool timed out")
            }
            // 23505 unique_violation / 40001 serialization_failure: real contention, so
            // the client can retry rather than treat it as a bug.
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                ApiError::new(ErrorCode::Conflict).internal(format!("unique violation: {db}"))
            }
            sqlx::Error::Database(db) if db.code().as_deref() == Some("40001") => {
                ApiError::new(ErrorCode::VersionConflict)
                    .internal(format!("serialization failure: {db}"))
            }
            _ => ApiError::new(ErrorCode::ServiceUnavailable).internal(error.to_string()),
        }
    }
}

impl From<validator::ValidationErrors> for ApiError {
    fn from(errors: validator::ValidationErrors) -> Self {
        let mut field_errors = Vec::new();
        for (field, errors) in errors.field_errors() {
            for error in errors {
                let message = error
                    .message
                    .as_ref()
                    .map(|message| message.to_string())
                    .unwrap_or_else(|| ErrorCode::ValidationFailed.default_message().to_owned());
                field_errors.push(FieldError::new(
                    field.to_string(),
                    error.code.to_uppercase(),
                    message,
                ));
            }
        }
        ApiError::with_fields(ErrorCode::ValidationFailed, field_errors)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        ApiError::new(ErrorCode::ServiceUnavailable).internal(format!("{error:#}"))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The §11.1 status table, asserted directly. If a code moves buckets, this fails.
    #[test]
    fn error_codes_map_to_the_documented_http_statuses() {
        let cases = [
            (ErrorCode::InvalidRequest, 400),
            (ErrorCode::ValidationFailed, 400),
            (ErrorCode::IdempotencyKeyRequired, 400),
            (ErrorCode::IdempotencyKeyInvalid, 400),
            (ErrorCode::InvalidCursor, 400),
            (ErrorCode::InvalidVersion, 400),
            // §6.1 splits Kakao failures three ways and no further: 취소 is the user's
            // choice (400), 보안 검증 실패 is a refusal (401), 일시 장애 is DEPENDENCY_
            // UNAVAILABLE (503).
            (ErrorCode::KakaoLoginCancelled, 400),
            (ErrorCode::KakaoSecurityCheckFailed, 401),
            (ErrorCode::Unauthenticated, 401),
            (ErrorCode::TokenExpired, 401),
            (ErrorCode::TokenInvalid, 401),
            (ErrorCode::Forbidden, 403),
            (ErrorCode::RoleRequired, 403),
            (ErrorCode::AccountSuspended, 403),
            (ErrorCode::AccountWithdrawn, 403),
            (ErrorCode::ConsentRequired, 403),
            (ErrorCode::ReauthenticationRequired, 403),
            (ErrorCode::OriginNotAllowed, 403),
            (ErrorCode::StoreNotActive, 403),
            (ErrorCode::NotFound, 404),
            (ErrorCode::StoreNotFound, 404),
            (ErrorCode::UserNotFound, 404),
            (ErrorCode::CatalogItemNotFound, 404),
            (ErrorCode::LoyaltyPolicyNotFound, 404),
            (ErrorCode::TransactionNotFound, 404),
            (ErrorCode::CouponNotFound, 404),
            (ErrorCode::CampaignNotFound, 404),
            (ErrorCode::ReservationNotFound, 404),
            (ErrorCode::AuthLinkNotFound, 404),
            (ErrorCode::AuthLinkAlreadyClaimed, 409),
            (ErrorCode::LastAuthLinkCannotBeRemoved, 409),
            (ErrorCode::ApprovalSeparationRequired, 403),
            (ErrorCode::Conflict, 409),
            (ErrorCode::VersionConflict, 409),
            (ErrorCode::IdempotencyKeyReused, 409),
            (ErrorCode::IdempotencyRequestInProgress, 409),
            (ErrorCode::StoreAlreadyExists, 409),
            (ErrorCode::StoreSlugTaken, 409),
            (ErrorCode::ReviewAlreadyPending, 409),
            (ErrorCode::QrAlreadyUsed, 409),
            (ErrorCode::DuplicateTransactionSuspected, 409),
            (ErrorCode::PolicyAlreadyScheduled, 409),
            (ErrorCode::PreviewExpired, 409),
            // §15 동시성 결정표 fixes these three: the loser of a claim, of a reservation,
            // and of a confirm-versus-expiry race each get a 409 with its own code.
            (ErrorCode::CampaignSoldOut, 409),
            (ErrorCode::CouponNotAvailable, 409),
            (ErrorCode::ReservationExpired, 409),
            (ErrorCode::ReservationAlreadyActive, 409),
            (ErrorCode::OrderAlreadyDiscounted, 409),
            (ErrorCode::UnprocessableRequest, 422),
            (ErrorCode::StoreNotReadyForReview, 422),
            (ErrorCode::InvalidStateTransition, 422),
            (ErrorCode::QrTokenInvalid, 422),
            (ErrorCode::QrTokenExpired, 422),
            (ErrorCode::NoActivePolicy, 422),
            (ErrorCode::PolicyNotEditable, 422),
            (ErrorCode::MinimumOrderNotMet, 422),
            (ErrorCode::ItemNotEligible, 422),
            (ErrorCode::DailyLimitExceeded, 422),
            (ErrorCode::VoidWindowExpired, 422),
            (ErrorCode::RequiresAdminReview, 422),
            (ErrorCode::CampaignPaused, 422),
            (ErrorCode::CampaignNotIssuing, 422),
            (ErrorCode::CampaignNotEditable, 422),
            (ErrorCode::QuantityBelowIssued, 422),
            (ErrorCode::AudienceNotEligible, 422),
            (ErrorCode::CouponNotYetUsable, 422),
            (ErrorCode::CouponExpired, 422),
            (ErrorCode::CouponOutsideUsageWindow, 422),
            (ErrorCode::RateLimited, 429),
            (ErrorCode::ServiceUnavailable, 503),
            (ErrorCode::DependencyUnavailable, 503),
        ];

        for (code, expected) in cases {
            assert_eq!(
                code.status().as_u16(),
                expected,
                "{} should map to {expected}",
                code.as_str()
            );
        }
    }

    #[test]
    fn only_transient_failures_are_marked_retryable() {
        assert!(ErrorCode::RateLimited.retryable());
        assert!(ErrorCode::ServiceUnavailable.retryable());
        assert!(ErrorCode::IdempotencyRequestInProgress.retryable());

        assert!(!ErrorCode::ValidationFailed.retryable());
        assert!(!ErrorCode::IdempotencyKeyReused.retryable());
        assert!(!ErrorCode::StoreAlreadyExists.retryable());
    }

    #[test]
    fn error_body_serialises_to_the_documented_envelope() {
        let body = ErrorEnvelope {
            error: ErrorBody {
                code: ErrorCode::NotFound.as_str().to_owned(),
                message: "현재 사용할 수 없는 쿠폰입니다.".to_owned(),
                field_errors: Vec::new(),
                retryable: false,
                request_id: "req_test".to_owned(),
            },
        };

        let json = serde_json::to_value(&body).expect("serialises");
        let error = json.get("error").expect("error envelope");
        assert_eq!(error["code"], "NOT_FOUND");
        assert_eq!(error["retryable"], false);
        assert_eq!(error["request_id"], "req_test");
        assert!(error["field_errors"].is_array());
    }

    #[test]
    fn internal_detail_never_reaches_the_client() {
        let error = ApiError::new(ErrorCode::ServiceUnavailable).internal("postgres is on fire");
        let json = serde_json::to_string(&ErrorEnvelope {
            error: ErrorBody {
                code: error.code.as_str().to_owned(),
                message: error.message.clone(),
                field_errors: error.field_errors.clone(),
                retryable: error.code.retryable(),
                request_id: String::new(),
            },
        })
        .expect("serialises");

        assert!(!json.contains("postgres is on fire"));
    }
}
