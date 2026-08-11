//! Store lifecycle, business identity and review (§10.2 `stores`, §12.2).
//!
//! Phase 1 covers the owner-facing half: draft a store, edit it, submit it for review.
//! Approval lives in `admin`.
//!
//! The one-store-per-owner rule (§12.6-1) is enforced by a partial unique index on
//! `stores(owner_user_id) WHERE status <> 'CLOSED'`, so the check survives a race that
//! an application-level lookup would lose.

pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::crypto::{LookupHash, Sealer};
use crate::db::changed_one_row;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::http::concurrency;
use crate::users::{AccountRole, UserService};

pub use routes::owner_store_router;

/// `coupon.store_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StoreStatus {
    Draft,
    PendingReview,
    Active,
    Suspended,
    Closed,
}

impl StoreStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            StoreStatus::Draft => "DRAFT",
            StoreStatus::PendingReview => "PENDING_REVIEW",
            StoreStatus::Active => "ACTIVE",
            StoreStatus::Suspended => "SUSPENDED",
            StoreStatus::Closed => "CLOSED",
        }
    }

    /// An unknown status is treated as suspended rather than assumed operable.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "DRAFT" => StoreStatus::Draft,
            "PENDING_REVIEW" => StoreStatus::PendingReview,
            "ACTIVE" => StoreStatus::Active,
            "CLOSED" => StoreStatus::Closed,
            _ => StoreStatus::Suspended,
        }
    }

    /// Whether the owner may still edit the store's public details.
    pub fn is_editable(self) -> bool {
        matches!(self, StoreStatus::Draft | StoreStatus::Active)
    }
}

/// `coupon.review_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewStatus {
    Pending,
    Approved,
    ChangesRequested,
    Rejected,
    Cancelled,
}

impl ReviewStatus {
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "APPROVED" => ReviewStatus::Approved,
            "CHANGES_REQUESTED" => ReviewStatus::ChangesRequested,
            "REJECTED" => ReviewStatus::Rejected,
            "CANCELLED" => ReviewStatus::Cancelled,
            _ => ReviewStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StoreResponse {
    pub id: Uuid,
    pub status: StoreStatus,
    pub name: String,
    pub slug: String,
    pub business_type: Option<String>,
    pub description: Option<String>,
    pub contact_phone: Option<String>,
    pub address: serde_json::Value,
    pub timezone: String,
    /// Local time a business day rolls over, `HH:MM:SS`.
    pub business_day_cutoff: String,
    pub business_hours: serde_json::Value,
    /// Whether the encrypted business-identity record has been filled in.
    pub business_profile_complete: bool,
    pub latest_review: Option<StoreReviewResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StoreReviewResponse {
    pub id: Uuid,
    pub status: ReviewStatus,
    pub submitted_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    /// Reason shown to the owner. Internal reviewer notes are never returned here.
    pub public_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct CreateStoreRequest {
    #[validate(length(min = 1, max = 200, message = "상점 이름은 1~200자여야 합니다."))]
    pub name: String,
    /// Public URL segment. Validated by [`ensure_valid_slug`], which mirrors the
    /// `ck_stores_slug_format` CHECK constraint.
    pub slug: String,
    #[validate(length(max = 100))]
    pub business_type: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    #[validate(length(max = 32))]
    pub contact_phone: Option<String>,
    pub address: Option<serde_json::Value>,
    #[validate(length(max = 64))]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct UpdateStoreRequest {
    #[validate(length(min = 1, max = 200, message = "상점 이름은 1~200자여야 합니다."))]
    pub name: Option<String>,
    #[validate(length(max = 100))]
    pub business_type: Option<String>,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    #[validate(length(max = 32))]
    pub contact_phone: Option<String>,
    pub address: Option<serde_json::Value>,
    pub business_hours: Option<serde_json::Value>,
    #[validate(length(max = 64))]
    pub timezone: Option<String>,
    /// `HH:MM` or `HH:MM:SS` local time.
    pub business_day_cutoff: Option<String>,
    /// Business identity, required before review. Encrypted at rest (§16.5).
    #[validate(nested)]
    pub business_profile: Option<BusinessProfileInput>,
    /// Expected `version`; may also be supplied as `If-Match`.
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct BusinessProfileInput {
    /// 사업자등록번호. Stored encrypted plus a keyed hash for duplicate detection.
    #[validate(length(min = 10, max = 32, message = "사업자등록번호를 확인해 주세요."))]
    pub registration_no: String,
    #[validate(length(min = 1, max = 100, message = "대표자명을 입력해 주세요."))]
    pub representative_name: String,
    #[validate(length(max = 300))]
    pub business_address: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema, Validate)]
pub struct SubmitReviewRequest {
    /// Optional note for the reviewer.
    #[validate(length(max = 1000))]
    pub note: Option<String>,
}

pub struct StoreService {
    sealer: Arc<Sealer>,
    lookup_hash: Arc<LookupHash>,
    users: Arc<UserService>,
}

impl StoreService {
    pub fn new(sealer: Arc<Sealer>, lookup_hash: Arc<LookupHash>, users: Arc<UserService>) -> Self {
        Self {
            sealer,
            lookup_hash,
            users,
        }
    }

    /// Create the caller's store draft and grant them the store-owner role.
    pub async fn create(
        &self,
        pool: &PgPool,
        owner_user_id: Uuid,
        request: &CreateStoreRequest,
    ) -> ApiResult<StoreResponse> {
        ensure_valid_slug(&request.slug)?;

        let mut tx = pool.begin().await?;

        let contact_phone_ciphertext = request
            .contact_phone
            .as_deref()
            .map(|phone| self.sealer.seal(phone));
        let contact_phone_hash = request
            .contact_phone
            .as_deref()
            .map(|phone| self.lookup_hash.hash("store-phone", phone));

        let store_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.stores
                (owner_user_id, status, name, slug, business_type, description,
                 contact_phone_ciphertext, contact_phone_lookup_hash, address_snapshot,
                 timezone)
            VALUES ($1, 'DRAFT', $2, $3, $4, $5, $6, $7,
                    COALESCE($8::jsonb, '{}'::jsonb),
                    COALESCE($9, 'Asia/Seoul'))
            RETURNING id
            "#,
            owner_user_id,
            request.name.trim(),
            request.slug.trim(),
            request.business_type.as_deref(),
            request.description.as_deref(),
            contact_phone_ciphertext,
            contact_phone_hash,
            request.address.clone(),
            request.timezone.as_deref(),
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(map_store_write_error)?;

        self.users
            .grant_role(&mut tx, owner_user_id, AccountRole::StoreOwner)
            .await?;

        tx.commit().await?;

        self.find_by_owner(pool, owner_user_id)
            .await?
            .ok_or_else(|| {
                ApiError::new(ErrorCode::ServiceUnavailable)
                    .internal(format!("store {store_id} vanished after insert"))
            })
    }

    /// The caller's store. `None` once it is closed, since a closed store frees the
    /// owner slot.
    pub async fn find_by_owner(
        &self,
        pool: &PgPool,
        owner_user_id: Uuid,
    ) -> ApiResult<Option<StoreResponse>> {
        let row = sqlx::query!(
            r#"
            SELECT
                s.id,
                s.status::text AS "status!",
                s.name,
                s.slug,
                s.business_type,
                s.description,
                s.contact_phone_ciphertext,
                s.address_snapshot,
                s.timezone,
                to_char(s.business_day_cutoff, 'HH24:MI:SS') AS "business_day_cutoff!",
                s.business_hours,
                s.created_at,
                s.updated_at,
                s.version,
                (p.store_id IS NOT NULL) AS "business_profile_complete!",
                r.id AS "review_id?",
                r.status::text AS "review_status?",
                r.submitted_at AS "review_submitted_at?",
                r.decided_at AS "review_decided_at?",
                r.public_reason AS "review_public_reason?"
            FROM coupon.stores s
            LEFT JOIN coupon.store_business_profiles p ON p.store_id = s.id
            LEFT JOIN LATERAL (
                SELECT id, status, submitted_at, decided_at, public_reason
                FROM coupon.store_reviews
                WHERE store_id = s.id
                ORDER BY submitted_at DESC
                LIMIT 1
            ) r ON true
            WHERE s.owner_user_id = $1 AND s.status <> 'CLOSED'
            "#,
            owner_user_id,
        )
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|row| StoreResponse {
            id: row.id,
            status: StoreStatus::from_db(&row.status),
            name: row.name,
            slug: row.slug,
            business_type: row.business_type,
            description: row.description,
            contact_phone: row
                .contact_phone_ciphertext
                .as_deref()
                .and_then(|sealed| self.sealer.open(sealed)),
            address: row.address_snapshot,
            timezone: row.timezone,
            business_day_cutoff: row.business_day_cutoff,
            business_hours: row.business_hours,
            business_profile_complete: row.business_profile_complete,
            latest_review: row.review_id.map(|id| StoreReviewResponse {
                id,
                status: row
                    .review_status
                    .as_deref()
                    .map(ReviewStatus::from_db)
                    .unwrap_or(ReviewStatus::Pending),
                submitted_at: row.review_submitted_at.unwrap_or(row.created_at),
                decided_at: row.review_decided_at,
                public_reason: row.review_public_reason,
            }),
            created_at: row.created_at,
            updated_at: row.updated_at,
            version: row.version,
        }))
    }

    /// Apply an owner's edit under optimistic concurrency.
    pub async fn update(
        &self,
        pool: &PgPool,
        owner_user_id: Uuid,
        request: &UpdateStoreRequest,
        expected_version: Option<i64>,
    ) -> ApiResult<StoreResponse> {
        let current = self
            .find_by_owner(pool, owner_user_id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::StoreNotFound))?;

        // A store under review is frozen: editing it would invalidate the snapshot the
        // reviewer is looking at.
        if !current.status.is_editable() {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                match current.status {
                    StoreStatus::PendingReview => "검수 중에는 상점 정보를 수정할 수 없습니다.",
                    _ => "현재 상태에서는 상점 정보를 수정할 수 없습니다.",
                },
            ));
        }

        let cutoff = request
            .business_day_cutoff
            .as_deref()
            .map(normalise_time_of_day)
            .transpose()?;

        let contact_phone_ciphertext = request
            .contact_phone
            .as_deref()
            .map(|phone| self.sealer.seal(phone));
        let contact_phone_hash = request
            .contact_phone
            .as_deref()
            .map(|phone| self.lookup_hash.hash("store-phone", phone));

        let mut tx = pool.begin().await?;

        let result = sqlx::query!(
            r#"
            UPDATE coupon.stores
            SET name = COALESCE($3, name),
                business_type = COALESCE($4, business_type),
                description = COALESCE($5, description),
                contact_phone_ciphertext = COALESCE($6, contact_phone_ciphertext),
                contact_phone_lookup_hash = COALESCE($7, contact_phone_lookup_hash),
                address_snapshot = COALESCE($8::jsonb, address_snapshot),
                business_hours = COALESCE($9::jsonb, business_hours),
                timezone = COALESCE($10, timezone),
                business_day_cutoff = COALESCE($11::text::time, business_day_cutoff)
            WHERE id = $1
              AND owner_user_id = $2
              AND ($12::bigint IS NULL OR version = $12)
            "#,
            current.id,
            owner_user_id,
            request.name.as_deref().map(str::trim),
            request.business_type.as_deref(),
            request.description.as_deref(),
            contact_phone_ciphertext,
            contact_phone_hash,
            request.address.clone(),
            request.business_hours.clone(),
            request.timezone.as_deref(),
            cutoff,
            expected_version,
        )
        .execute(&mut *tx)
        .await
        .map_err(map_store_write_error)?;

        if !changed_one_row(&result) {
            // The store was found a moment ago, so zero rows means a lost race.
            concurrency::ensure_updated(result.rows_affected(), true)?;
        }

        if let Some(profile) = &request.business_profile {
            self.upsert_business_profile(&mut tx, current.id, profile)
                .await?;
        }

        tx.commit().await?;

        self.find_by_owner(pool, owner_user_id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::StoreNotFound))
    }

    async fn upsert_business_profile(
        &self,
        tx: &mut crate::db::Tx<'_>,
        store_id: Uuid,
        profile: &BusinessProfileInput,
    ) -> ApiResult<()> {
        // Digits only, so `123-45-67890` and `1234567890` hash to the same value and
        // duplicate registrations are actually detectable.
        let normalised: String = profile
            .registration_no
            .chars()
            .filter(char::is_ascii_digit)
            .collect();

        if normalised.len() != 10 {
            return Err(ApiError::with_message(
                ErrorCode::ValidationFailed,
                "사업자등록번호는 숫자 10자리여야 합니다.",
            ));
        }

        sqlx::query!(
            r#"
            INSERT INTO coupon.store_business_profiles
                (store_id, registration_no_ciphertext, registration_no_lookup_hash,
                 representative_name_ciphertext, business_address_ciphertext)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (store_id) DO UPDATE SET
                registration_no_ciphertext = EXCLUDED.registration_no_ciphertext,
                registration_no_lookup_hash = EXCLUDED.registration_no_lookup_hash,
                representative_name_ciphertext = EXCLUDED.representative_name_ciphertext,
                business_address_ciphertext = EXCLUDED.business_address_ciphertext
            "#,
            store_id,
            self.sealer.seal(&normalised),
            self.lookup_hash.hash("store-registration-no", &normalised),
            self.sealer.seal(profile.representative_name.trim()),
            profile
                .business_address
                .as_deref()
                .map(|address| self.sealer.seal(address)),
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    /// Submit the store for review: freeze a snapshot, queue it, and move the store to
    /// `PENDING_REVIEW`.
    pub async fn submit_for_review(
        &self,
        pool: &PgPool,
        owner_user_id: Uuid,
        request: &SubmitReviewRequest,
    ) -> ApiResult<StoreResponse> {
        let store = self
            .find_by_owner(pool, owner_user_id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::StoreNotFound))?;

        ensure_submittable(&store)?;

        let snapshot = submission_snapshot(&store, request.note.as_deref());
        let mut tx = pool.begin().await?;

        // Guarded by `uq_store_reviews_pending_store`, so two tabs cannot both queue it.
        sqlx::query!(
            r#"
            INSERT INTO coupon.store_reviews (store_id, status, submission_snapshot)
            VALUES ($1, 'PENDING', $2)
            "#,
            store.id,
            snapshot,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                ApiError::new(ErrorCode::ReviewAlreadyPending)
            }
            _ => ApiError::from(error),
        })?;

        let result = sqlx::query!(
            r#"
            UPDATE coupon.stores
            SET status = 'PENDING_REVIEW'
            WHERE id = $1 AND owner_user_id = $2 AND status = 'DRAFT'
            "#,
            store.id,
            owner_user_id,
        )
        .execute(&mut *tx)
        .await?;

        if !changed_one_row(&result) {
            return Err(ApiError::new(ErrorCode::InvalidStateTransition)
                .internal("store left DRAFT while the review was being queued"));
        }

        tx.commit().await?;

        self.find_by_owner(pool, owner_user_id)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::StoreNotFound))
    }
}

/// The preconditions for review submission, separated so they can be tested directly.
fn ensure_submittable(store: &StoreResponse) -> ApiResult<()> {
    if store.status != StoreStatus::Draft {
        return Err(ApiError::with_message(
            ErrorCode::InvalidStateTransition,
            match store.status {
                StoreStatus::PendingReview => "이미 검수 대기 중입니다.",
                StoreStatus::Active => "이미 영업 중인 상점입니다.",
                _ => "현재 상태에서는 검수를 제출할 수 없습니다.",
            },
        ));
    }

    if !store.business_profile_complete {
        return Err(ApiError::with_message(
            ErrorCode::StoreNotReadyForReview,
            "사업자 정보를 먼저 입력해 주세요.",
        ));
    }

    if store
        .address
        .as_object()
        .is_none_or(|address| address.is_empty())
    {
        return Err(ApiError::with_message(
            ErrorCode::StoreNotReadyForReview,
            "상점 주소를 입력해 주세요.",
        ));
    }

    Ok(())
}

/// What the reviewer sees, frozen at submission time.
///
/// Deliberately excludes every encrypted column: the registration number and
/// representative name stay in `store_business_profiles`, not duplicated in a JSONB
/// blob that outlives them.
fn submission_snapshot(store: &StoreResponse, note: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "store_id": store.id,
        "name": store.name,
        "slug": store.slug,
        "business_type": store.business_type,
        "description": store.description,
        "address": store.address,
        "timezone": store.timezone,
        "business_day_cutoff": store.business_day_cutoff,
        "business_hours": store.business_hours,
        "business_profile_complete": store.business_profile_complete,
        "store_version": store.version,
        "note": note,
        "submitted_at": Utc::now().to_rfc3339(),
    })
}

/// Accept `HH:MM` or `HH:MM:SS` and normalise to what `time` expects.
fn normalise_time_of_day(raw: &str) -> ApiResult<String> {
    let raw = raw.trim();
    let parts: Vec<&str> = raw.split(':').collect();

    let invalid = || {
        ApiError::with_message(
            ErrorCode::ValidationFailed,
            "영업일 기준 시각은 HH:MM 형식이어야 합니다.",
        )
    };

    if !(2..=3).contains(&parts.len()) {
        return Err(invalid());
    }

    let hour: u32 = parts[0].parse().map_err(|_| invalid())?;
    let minute: u32 = parts[1].parse().map_err(|_| invalid())?;
    let second: u32 = parts
        .get(2)
        .map_or(Ok(0), |s| s.parse().map_err(|_| invalid()))?;

    if hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }

    Ok(format!("{hour:02}:{minute:02}:{second:02}"))
}

/// Turn store-table constraint violations into the codes a client can act on.
fn map_store_write_error(error: sqlx::Error) -> ApiError {
    let sqlx::Error::Database(db) = &error else {
        return ApiError::from(error);
    };
    if db.code().as_deref() != Some("23505") {
        return ApiError::from(error);
    }

    match db.constraint() {
        // §12.6-1: one active store per owner.
        Some("uq_stores_active_owner") => ApiError::new(ErrorCode::StoreAlreadyExists),
        Some("uq_stores_slug") => ApiError::new(ErrorCode::StoreSlugTaken),
        _ => ApiError::new(ErrorCode::Conflict).internal(db.to_string()),
    }
}

/// Mirror of the `ck_stores_slug_format` CHECK constraint
/// (`^[a-z0-9][a-z0-9-]{1,98}[a-z0-9]$`), checked here so the client gets a field error
/// instead of a bare constraint violation.
fn ensure_valid_slug(slug: &str) -> ApiResult<()> {
    let invalid = || {
        ApiError::with_fields(
            ErrorCode::ValidationFailed,
            vec![crate::error::FieldError::new(
                "slug",
                "INVALID_FORMAT",
                "상점 주소는 소문자·숫자·하이픈만 사용해 3~100자여야 합니다.",
            )],
        )
    };

    if !(3..=100).contains(&slug.len()) {
        return Err(invalid());
    }
    if !slug
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(invalid());
    }
    // The first and last characters may not be a hyphen.
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(invalid());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(status: StoreStatus) -> StoreResponse {
        StoreResponse {
            id: Uuid::nil(),
            status,
            name: "브로트베르크".to_owned(),
            slug: "brotwerk".to_owned(),
            business_type: Some("BAKERY".to_owned()),
            description: None,
            contact_phone: None,
            address: serde_json::json!({"road": "성수이로 1"}),
            timezone: "Asia/Seoul".to_owned(),
            business_day_cutoff: "00:00:00".to_owned(),
            business_hours: serde_json::json!({}),
            business_profile_complete: true,
            latest_review: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
        }
    }

    #[test]
    fn statuses_round_trip_and_unknown_values_fail_closed() {
        for status in [
            StoreStatus::Draft,
            StoreStatus::PendingReview,
            StoreStatus::Active,
            StoreStatus::Suspended,
            StoreStatus::Closed,
        ] {
            assert_eq!(StoreStatus::from_db(status.as_db()), status);
        }
        assert_eq!(
            StoreStatus::from_db("SOMETHING_NEW"),
            StoreStatus::Suspended
        );
    }

    #[test]
    fn only_draft_and_active_stores_are_editable() {
        assert!(StoreStatus::Draft.is_editable());
        assert!(StoreStatus::Active.is_editable());
        assert!(!StoreStatus::PendingReview.is_editable());
        assert!(!StoreStatus::Suspended.is_editable());
        assert!(!StoreStatus::Closed.is_editable());
    }

    #[test]
    fn a_complete_draft_may_be_submitted() {
        ensure_submittable(&store(StoreStatus::Draft)).expect("complete draft submits");
    }

    #[test]
    fn only_drafts_may_be_submitted() {
        for status in [
            StoreStatus::PendingReview,
            StoreStatus::Active,
            StoreStatus::Suspended,
        ] {
            let error = ensure_submittable(&store(status)).expect_err("must reject");
            assert_eq!(error.code, ErrorCode::InvalidStateTransition);
            assert_eq!(error.status().as_u16(), 422);
        }
    }

    #[test]
    fn submission_requires_business_identity_and_an_address() {
        let mut incomplete = store(StoreStatus::Draft);
        incomplete.business_profile_complete = false;
        assert_eq!(
            ensure_submittable(&incomplete)
                .expect_err("no profile")
                .code,
            ErrorCode::StoreNotReadyForReview
        );

        let mut no_address = store(StoreStatus::Draft);
        no_address.address = serde_json::json!({});
        assert_eq!(
            ensure_submittable(&no_address)
                .expect_err("no address")
                .code,
            ErrorCode::StoreNotReadyForReview
        );
    }

    #[test]
    fn the_review_snapshot_carries_no_encrypted_identity() {
        let snapshot =
            submission_snapshot(&store(StoreStatus::Draft), Some("빠른 검수 부탁드립니다"));
        let serialised = snapshot.to_string();

        assert_eq!(snapshot["schema_version"], 1);
        assert_eq!(snapshot["slug"], "brotwerk");
        assert_eq!(snapshot["note"], "빠른 검수 부탁드립니다");
        for forbidden in [
            "registration_no",
            "representative",
            "ciphertext",
            "contact_phone",
        ] {
            assert!(
                !serialised.contains(forbidden),
                "snapshot must not carry {forbidden}"
            );
        }
    }

    #[test]
    fn business_day_cutoff_accepts_both_precisions() {
        assert_eq!(normalise_time_of_day("06:00").expect("parses"), "06:00:00");
        assert_eq!(
            normalise_time_of_day(" 6:5:1 ").expect("parses"),
            "06:05:01"
        );
        assert_eq!(
            normalise_time_of_day("23:59:59").expect("parses"),
            "23:59:59"
        );
    }

    #[test]
    fn an_impossible_cutoff_is_rejected() {
        for raw in ["24:00", "06", "06:60", "abc", "06:00:00:00", ""] {
            assert_eq!(
                normalise_time_of_day(raw).expect_err("must reject").code,
                ErrorCode::ValidationFailed,
                "{raw} must not be accepted"
            );
        }
    }

    #[test]
    fn slugs_follow_the_database_check_constraint() {
        for valid in ["brotwerk", "cafe-1", "a1b", &"x".repeat(100)] {
            ensure_valid_slug(valid).unwrap_or_else(|_| panic!("{valid} should be valid"));
        }
        for invalid in [
            "ab",
            "-cafe",
            "cafe-",
            "Cafe",
            "카페",
            "ca fe",
            &"x".repeat(101),
            "",
        ] {
            let error =
                ensure_valid_slug(invalid).expect_err(&format!("{invalid} should be invalid"));
            assert_eq!(error.code, ErrorCode::ValidationFailed);
            assert_eq!(
                error.field_errors.first().map(|f| f.field.as_str()),
                Some("slug")
            );
        }
    }
}
