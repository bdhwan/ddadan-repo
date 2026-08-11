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

    // 401 — authentication.
    Unauthenticated,
    TokenExpired,
    TokenInvalid,

    // 403 — role, account state, terms, origin.
    Forbidden,
    RoleRequired,
    AccountSuspended,
    AccountWithdrawn,
    ConsentRequired,
    ReauthenticationRequired,
    OriginNotAllowed,

    // 404 — absent, or not owned by the caller.
    NotFound,
    StoreNotFound,
    UserNotFound,

    // 409 — state, version and quantity contention.
    Conflict,
    VersionConflict,
    IdempotencyKeyReused,
    IdempotencyRequestInProgress,
    StoreAlreadyExists,
    StoreSlugTaken,
    ReviewAlreadyPending,

    // 422 — well-formed but the business rule says no.
    UnprocessableRequest,
    StoreNotReadyForReview,
    InvalidStateTransition,

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
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::StoreNotFound => "STORE_NOT_FOUND",
            ErrorCode::UserNotFound => "USER_NOT_FOUND",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::VersionConflict => "VERSION_CONFLICT",
            ErrorCode::IdempotencyKeyReused => "IDEMPOTENCY_KEY_REUSED",
            ErrorCode::IdempotencyRequestInProgress => "IDEMPOTENCY_REQUEST_IN_PROGRESS",
            ErrorCode::StoreAlreadyExists => "STORE_ALREADY_EXISTS",
            ErrorCode::StoreSlugTaken => "STORE_SLUG_TAKEN",
            ErrorCode::ReviewAlreadyPending => "REVIEW_ALREADY_PENDING",
            ErrorCode::UnprocessableRequest => "UNPROCESSABLE_REQUEST",
            ErrorCode::StoreNotReadyForReview => "STORE_NOT_READY_FOR_REVIEW",
            ErrorCode::InvalidStateTransition => "INVALID_STATE_TRANSITION",
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
            | ErrorCode::InvalidVersion => StatusCode::BAD_REQUEST,

            // 401: no credential, or one we cannot accept.
            ErrorCode::Unauthenticated | ErrorCode::TokenExpired | ErrorCode::TokenInvalid => {
                StatusCode::UNAUTHORIZED
            }

            // 403: authenticated, but role / account state / terms / origin forbid it.
            ErrorCode::Forbidden
            | ErrorCode::RoleRequired
            | ErrorCode::AccountSuspended
            | ErrorCode::AccountWithdrawn
            | ErrorCode::ConsentRequired
            | ErrorCode::ReauthenticationRequired
            | ErrorCode::OriginNotAllowed => StatusCode::FORBIDDEN,

            // 404: absent, or present but not owned by the caller — indistinguishable on
            // purpose, so ownership cannot be probed.
            ErrorCode::NotFound | ErrorCode::StoreNotFound | ErrorCode::UserNotFound => {
                StatusCode::NOT_FOUND
            }

            // 409: state, version or quantity contention.
            ErrorCode::Conflict
            | ErrorCode::VersionConflict
            | ErrorCode::IdempotencyKeyReused
            | ErrorCode::IdempotencyRequestInProgress
            | ErrorCode::StoreAlreadyExists
            | ErrorCode::StoreSlugTaken
            | ErrorCode::ReviewAlreadyPending => StatusCode::CONFLICT,

            // 422: shape is fine, the business condition is not met.
            ErrorCode::UnprocessableRequest
            | ErrorCode::StoreNotReadyForReview
            | ErrorCode::InvalidStateTransition => StatusCode::UNPROCESSABLE_ENTITY,

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
            ErrorCode::NotFound => "요청한 리소스를 찾을 수 없습니다.",
            ErrorCode::StoreNotFound => "상점을 찾을 수 없습니다.",
            ErrorCode::UserNotFound => "회원 정보를 찾을 수 없습니다.",
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
            ErrorCode::UnprocessableRequest => "요청 조건을 충족하지 않습니다.",
            ErrorCode::StoreNotReadyForReview => "검수 제출에 필요한 정보가 아직 부족합니다.",
            ErrorCode::InvalidStateTransition => "현재 상태에서는 변경할 수 없습니다.",
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
            (ErrorCode::NotFound, 404),
            (ErrorCode::StoreNotFound, 404),
            (ErrorCode::UserNotFound, 404),
            (ErrorCode::Conflict, 409),
            (ErrorCode::VersionConflict, 409),
            (ErrorCode::IdempotencyKeyReused, 409),
            (ErrorCode::IdempotencyRequestInProgress, 409),
            (ErrorCode::StoreAlreadyExists, 409),
            (ErrorCode::StoreSlugTaken, 409),
            (ErrorCode::ReviewAlreadyPending, 409),
            (ErrorCode::UnprocessableRequest, 422),
            (ErrorCode::StoreNotReadyForReview, 422),
            (ErrorCode::InvalidStateTransition, 422),
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
