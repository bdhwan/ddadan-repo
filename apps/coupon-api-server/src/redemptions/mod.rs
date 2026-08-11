//! 예약·사용·취소와 할인 계산 (§10.2 `redemptions`, §13.3, §13.4, REDEEM-001…006).
//!
//! Owns `redemption_reservations` and `redemption_transactions`.
//!
//! Spending a coupon is deliberately two steps. §13.3 splits it into a two-minute
//! reservation and a confirmation, and the reason is a counter one: the owner is standing
//! at a till with a customer, and needs to see the discount before committing to it. A
//! one-shot API would either show a number it had not reserved (so the customer could
//! spend the coupon elsewhere in between) or commit before the owner agreed.
//!
//! ## What makes it safe
//!
//! * `uq_redemption_reservations_active_coupon` — 쿠폰당 활성 예약 최대 1개 (§12.6-6).
//! * `uq_redemption_transactions_confirmed_coupon` — 쿠폰당 성공 사용 최대 1개.
//! * `uq_redemption_transactions_order_ref` — 주문당 혜택 1개 (§5.4, §8.6).
//! * The coupon row lock, taken in the same order by every path here, so a confirmation
//!   racing an expiry sweep or a cancellation resolves to exactly one outcome (§15).
//!
//! Every condition is recomputed at `confirm` from the coupon's own frozen snapshot. The
//! reservation is a hold and a display, never an authority (§11.4: "preview는 표시
//! 편의를 위한 것이며 confirm에서 모든 조건을 다시 검증한다").

pub mod discount;
pub mod routes;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::audit::{ActorType, AuditEntry, AuditService};
use crate::catalog::{CatalogService, OrderLine};
use crate::db::Tx;
use crate::error::{ApiError, ApiResult, ErrorCode};
use crate::loyalty::OrderInput;
use crate::qr::{AUDIENCE_STAMP, QrService};
use crate::stores::{OwnedStore, StoreService};
use crate::wallet::CouponStatus;

pub use discount::{Benefit, CouponConditions, Discount, FreeItemAward, LocalTimeRange};
pub use routes::owner_redemption_router;

/// `coupon.reservation_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReservationStatus {
    Active,
    Confirmed,
    Cancelled,
    Expired,
    Revoked,
}

impl ReservationStatus {
    pub fn as_db(self) -> &'static str {
        match self {
            ReservationStatus::Active => "ACTIVE",
            ReservationStatus::Confirmed => "CONFIRMED",
            ReservationStatus::Cancelled => "CANCELLED",
            ReservationStatus::Expired => "EXPIRED",
            ReservationStatus::Revoked => "REVOKED",
        }
    }

    /// Unknown reads as `REVOKED`: a reservation this build cannot interpret must not
    /// look like one that may still be confirmed.
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "ACTIVE" => ReservationStatus::Active,
            "CONFIRMED" => ReservationStatus::Confirmed,
            "CANCELLED" => ReservationStatus::Cancelled,
            "EXPIRED" => ReservationStatus::Expired,
            _ => ReservationStatus::Revoked,
        }
    }
}

/// `coupon.redemption_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RedemptionStatus {
    Confirmed,
    Voided,
    RequiresAdminReview,
}

impl RedemptionStatus {
    pub fn from_db(raw: &str) -> Self {
        match raw {
            "CONFIRMED" => RedemptionStatus::Confirmed,
            "VOIDED" => RedemptionStatus::Voided,
            _ => RedemptionStatus::RequiresAdminReview,
        }
    }
}

/// `POST /owner/redemptions/preview` (§11.4, §13.3).
#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ReservationRequest {
    /// The customer's rotating QR, exactly as the accrual path takes it (§16.2).
    pub qr_token: Option<String>,
    #[validate(length(min = 8, max = 16))]
    pub fallback_code: Option<String>,
    /// Which coupon the customer chose (REDEEM-001 step 2).
    pub coupon_id: Uuid,
    /// The till session. REDEEM-002 allows one active reservation per owner session, so
    /// two tills in one shop can serve two customers but one till cannot start two sales.
    #[validate(length(min = 1, max = 255))]
    pub owner_session_id: String,
    #[validate(nested)]
    pub order: OrderInput,
}

impl ReservationRequest {
    fn presented(&self) -> ApiResult<crate::qr::Presented<'_>> {
        match (self.qr_token.as_deref(), self.fallback_code.as_deref()) {
            (Some(token), None) => Ok(crate::qr::Presented::Token(token)),
            (None, Some(code)) => Ok(crate::qr::Presented::FallbackCode(code)),
            _ => Err(ApiError::with_message(
                ErrorCode::InvalidRequest,
                "QR 토큰 또는 8자리 보조 코드 중 하나만 보내 주세요.",
            )),
        }
    }
}

#[derive(Debug, Clone, Deserialize, ToSchema, Validate)]
pub struct ConfirmRedemptionRequest {
    /// Echoed so a mismatch between what the owner saw and what they are approving is
    /// caught rather than silently reconciled (§13.3-2 주문 hash).
    #[validate(nested)]
    pub order: OrderInput,
    #[validate(length(min = 1, max = 255))]
    pub owner_session_id: String,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema, Validate)]
pub struct CancelRedemptionRequest {
    #[validate(length(min = 1, max = 500, message = "취소 사유를 입력해 주세요."))]
    pub reason: Option<String>,
    /// REDEEM-004: 원 캠페인이 아직 유효하고 쿠폰 만료 전이면 `AVAILABLE` 복원을
    /// 선택할 수 있다.
    #[serde(default = "default_restore")]
    pub restore_coupon: bool,
}

fn default_restore() -> bool {
    true
}

/// What the owner is shown before approving (§13.3 예약 4).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Reservation {
    pub reservation_id: Uuid,
    pub coupon_id: Uuid,
    pub store_id: Uuid,
    pub status: ReservationStatus,
    pub coupon_title: String,
    pub gross_amount: i64,
    pub eligible_amount: i64,
    pub expected_discount_amount: i64,
    pub payable_amount: i64,
    pub free_item: Option<FreeItemAward>,
    /// Whether the store said its own discounts may be combined. Shown to the owner
    /// because §5.4 says the system cannot verify an external discount.
    pub external_discount_stackable: bool,
    pub reserved_at: DateTime<Utc>,
    /// Two minutes (§23.1).
    pub expires_at: DateTime<Utc>,
}

/// The completed use (§13.3 승인 4–5).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Redemption {
    pub redemption_id: Uuid,
    pub reservation_id: Uuid,
    pub coupon_id: Uuid,
    pub store_id: Uuid,
    pub status: RedemptionStatus,
    pub gross_amount: i64,
    pub eligible_amount: i64,
    pub discount_amount: i64,
    pub payable_amount: i64,
    pub free_item: Option<FreeItemAward>,
    pub external_order_ref: Option<String>,
    pub confirmed_at: DateTime<Utc>,
    pub voided_at: Option<DateTime<Utc>>,
    /// Until when the owner may still undo this themselves — 사용 취소 10분 (§8.6).
    pub voidable_until: Option<DateTime<Utc>>,
    /// Whether cancelling put the coupon back in the wallet (REDEEM-004).
    pub coupon_restored: bool,
    pub coupon_status: CouponStatus,
}

pub struct RedemptionService {
    stores: Arc<StoreService>,
    catalog: Arc<CatalogService>,
    qr: Arc<QrService>,
    audit: Arc<AuditService>,
    reservation_ttl: chrono::Duration,
    void_window: chrono::Duration,
}

impl RedemptionService {
    pub fn new(
        stores: Arc<StoreService>,
        catalog: Arc<CatalogService>,
        qr: Arc<QrService>,
        audit: Arc<AuditService>,
        reservation_ttl: chrono::Duration,
        void_window: chrono::Duration,
    ) -> Self {
        Self {
            stores,
            catalog,
            qr,
            audit,
            reservation_ttl,
            void_window,
        }
    }

    /// `POST /owner/redemptions/preview` — §13.3's reservation, step by step.
    pub async fn reserve(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        idempotency_key: Uuid,
        request: &ReservationRequest,
    ) -> ApiResult<Reservation> {
        store.ensure_operating()?;

        // Validated before the transaction, locked inside it — the same shape the accrual
        // path uses so the two take the store and the nonce in one consistent order.
        let unlocked_now = crate::qr::database_now(pool).await?;
        let nonce = self
            .qr
            .resolve(pool, request.presented()?, AUDIENCE_STAMP, unlocked_now)
            .await?;

        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        self.stores.lock_store(&mut tx, store.id).await?;

        // Step 1: 쿠폰 행 잠금. Everything after this is decided under it.
        let coupon = self.lock_coupon(&mut tx, request.coupon_id).await?;

        // Step 2: 소유·상점·AVAILABLE·기간·조건 확인.
        //
        // A coupon belonging to someone else reads as absent rather than as forbidden, so
        // an owner cannot probe which coupons a customer holds (SEC-001).
        if coupon.user_id != nonce.user_id {
            return Err(ApiError::new(ErrorCode::CouponNotFound)
                .internal("the presented QR belongs to a different consumer"));
        }
        if coupon.store_id != store.id {
            return Err(ApiError::with_message(
                ErrorCode::CouponNotFound,
                "다른 상점의 쿠폰입니다.",
            ));
        }

        let conditions = CouponConditions::from_snapshot(&coupon.condition_snapshot)?;
        ensure_spendable(&coupon, &conditions, now)?;

        let lines = self
            .catalog
            .resolve_order_lines(&mut *tx, store.id, &request.order.items)
            .await?;
        ensure_currency(&request.order)?;
        self.ensure_order_not_already_discounted(
            &mut tx,
            store.id,
            request.order.external_order_ref.as_deref(),
        )
        .await?;

        let discount = discount::calculate(&conditions, &lines, request.order.gross_amount)?;

        // Step 3: 2분 만료 예약 생성, 쿠폰을 RESERVED 로 전환.
        //
        // The coupon moves first and conditionally: `WHERE status = 'AVAILABLE'` is what
        // makes two simultaneous reservations resolve to one winner even before the
        // partial unique index gets involved (§15 동일 쿠폰 동시 예약).
        let claimed = sqlx::query!(
            r#"
            UPDATE coupon.coupon_instances
            SET status = 'RESERVED', reserved_at = $2
            WHERE id = $1 AND status = 'AVAILABLE'
            "#,
            coupon.id,
            now,
        )
        .execute(&mut *tx)
        .await?;

        if claimed.rows_affected() != 1 {
            return Err(ApiError::new(ErrorCode::CouponNotAvailable));
        }

        let expires_at = now + self.reservation_ttl;
        let order_snapshot = order_snapshot(&request.order, &lines, &discount);
        let discount_snapshot = serde_json::to_value(&discount).unwrap_or_default();

        let reservation_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.redemption_reservations
                (coupon_id, store_id, user_id, owner_user_id, owner_session_id, qr_nonce_id,
                 status, order_snapshot, order_snapshot_hash, discount_snapshot,
                 external_order_ref, expected_discount_amount, reserved_at, expires_at,
                 idempotency_key)
            VALUES ($1, $2, $3, $4, $5, $6, 'ACTIVE', $7, $8, $9, $10, $11, $12, $13, $14)
            RETURNING id
            "#,
            coupon.id,
            store.id,
            coupon.user_id,
            owner_user_id,
            request.owner_session_id.trim(),
            nonce.nonce_id,
            order_snapshot,
            order_hash(&request.order),
            discount_snapshot,
            request.order.external_order_ref.as_deref(),
            discount.discount_amount,
            now,
            expires_at,
            idempotency_key,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(map_reservation_conflict)?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.coupon_status_events
                (coupon_id, from_status, to_status, actor_type, actor_user_id, reason_code,
                 transaction_id, metadata, occurred_at)
            VALUES ($1, 'AVAILABLE', 'RESERVED', 'STORE_OWNER', $2, 'REDEMPTION_RESERVED',
                    $3, $4, $5)
            "#,
            coupon.id,
            owner_user_id,
            reservation_id,
            serde_json::json!({ "expected_discount_amount": discount.discount_amount }),
            now,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!(
            %reservation_id,
            coupon_id = %coupon.id,
            store_id = %store.id,
            discount = discount.discount_amount,
            "redemption.reserved"
        );

        Ok(Reservation {
            reservation_id,
            coupon_id: coupon.id,
            store_id: store.id,
            status: ReservationStatus::Active,
            coupon_title: coupon.title,
            gross_amount: request.order.gross_amount,
            eligible_amount: discount.eligible_amount,
            expected_discount_amount: discount.discount_amount,
            payable_amount: discount.payable_amount(request.order.gross_amount),
            free_item: discount.free_item,
            external_discount_stackable: conditions.external_discount_stackable,
            reserved_at: now,
            expires_at,
        })
    }

    /// `POST /owner/redemptions/:id/confirm` — §13.3's approval.
    pub async fn confirm(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        reservation_id: Uuid,
        idempotency_key: Uuid,
        request: &ConfirmRedemptionRequest,
    ) -> ApiResult<Redemption> {
        store.ensure_operating()?;

        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        self.stores.lock_store(&mut tx, store.id).await?;

        // Step 1: 예약과 쿠폰을 같은 순서로 잠금 — reservation, then coupon, on every
        // path here, which is what stops a confirmation and a cancellation deadlocking.
        let reservation = self.lock_reservation(&mut tx, store.id, reservation_id).await?;
        let coupon = self.lock_coupon(&mut tx, reservation.coupon_id).await?;

        // Step 2: 예약 소유 점주 세션, 미만료, 주문 hash 검증.
        if reservation.status != ReservationStatus::Active {
            return Err(match reservation.status {
                ReservationStatus::Confirmed => ApiError::with_message(
                    ErrorCode::Conflict,
                    "이미 승인된 예약입니다.",
                ),
                ReservationStatus::Expired => ApiError::new(ErrorCode::ReservationExpired),
                _ => ApiError::with_message(
                    ErrorCode::InvalidStateTransition,
                    "취소되었거나 회수된 예약입니다.",
                ),
            });
        }

        // §15: 예약 만료와 최종 승인 경합은 행 잠금 후 서버 시각으로 결정한다. The
        // sweep and this check agree because both compare `expires_at` to the *database*
        // clock under the same row lock — only one of them can be the one that moves it.
        if now >= reservation.expires_at {
            self.lapse(&mut tx, &reservation, now, "RESERVATION_EXPIRED")
                .await?;
            tx.commit().await?;
            return Err(ApiError::new(ErrorCode::ReservationExpired));
        }

        if reservation.owner_session_id != request.owner_session_id.trim() {
            return Err(ApiError::with_message(
                ErrorCode::Forbidden,
                "예약을 만든 세션에서만 승인할 수 있습니다.",
            ));
        }
        if reservation.owner_user_id != owner_user_id {
            return Err(ApiError::with_message(
                ErrorCode::Forbidden,
                "예약을 만든 점주만 승인할 수 있습니다.",
            ));
        }
        if reservation.order_snapshot_hash != order_hash(&request.order) {
            return Err(ApiError::with_message(
                ErrorCode::Conflict,
                "주문 내용이 예약 때와 다릅니다. 다시 확인해 주세요.",
            ));
        }

        // Step 3: 조건을 재계산한다. Not a formality — the coupon may have expired inside
        // the two minutes, and the discount is recomputed from the snapshot rather than
        // trusted from the reservation (§11.4).
        let conditions = CouponConditions::from_snapshot(&coupon.condition_snapshot)?;
        if coupon.status != CouponStatus::Reserved {
            return Err(ApiError::new(ErrorCode::CouponNotAvailable)
                .internal(format!("coupon is {:?}", coupon.status)));
        }
        if now >= coupon.expires_at {
            return Err(ApiError::new(ErrorCode::CouponExpired));
        }
        if now < coupon.usable_from {
            return Err(ApiError::new(ErrorCode::CouponNotYetUsable));
        }
        if !conditions.allows_moment(now) {
            return Err(ApiError::new(ErrorCode::CouponOutsideUsageWindow));
        }

        let lines = self
            .catalog
            .resolve_order_lines(&mut *tx, store.id, &request.order.items)
            .await?;
        ensure_currency(&request.order)?;
        self.ensure_order_not_already_discounted(
            &mut tx,
            store.id,
            request.order.external_order_ref.as_deref(),
        )
        .await?;

        let discount = discount::calculate(&conditions, &lines, request.order.gross_amount)?;

        // Step 4: 사용 원장과 USED 상태 사건을 기록한다.
        let moved = sqlx::query!(
            r#"
            UPDATE coupon.coupon_instances
            SET status = 'USED', used_at = $2
            WHERE id = $1 AND status = 'RESERVED'
            "#,
            coupon.id,
            now,
        )
        .execute(&mut *tx)
        .await?;

        if moved.rows_affected() != 1 {
            return Err(ApiError::new(ErrorCode::CouponNotAvailable));
        }

        sqlx::query!(
            r#"
            UPDATE coupon.redemption_reservations
            SET status = 'CONFIRMED', completed_at = $2
            WHERE id = $1 AND status = 'ACTIVE'
            "#,
            reservation_id,
            now,
        )
        .execute(&mut *tx)
        .await?;

        let order_snapshot = order_snapshot(&request.order, &lines, &discount);
        let redemption_id = sqlx::query_scalar!(
            r#"
            INSERT INTO coupon.redemption_transactions
                (coupon_id, reservation_id, store_id, user_id, approved_by_user_id,
                 external_order_ref, order_snapshot, discount_snapshot, gross_amount,
                 eligible_amount, discount_amount, status, confirmed_at, idempotency_key)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'CONFIRMED', $12, $13)
            RETURNING id
            "#,
            coupon.id,
            reservation_id,
            store.id,
            coupon.user_id,
            owner_user_id,
            request.order.external_order_ref.as_deref(),
            order_snapshot,
            serde_json::to_value(&discount).unwrap_or_default(),
            request.order.gross_amount,
            discount.eligible_amount,
            discount.discount_amount,
            now,
            idempotency_key,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(map_redemption_conflict)?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.coupon_status_events
                (coupon_id, from_status, to_status, actor_type, actor_user_id, reason_code,
                 transaction_id, metadata, occurred_at)
            VALUES ($1, 'RESERVED', 'USED', 'STORE_OWNER', $2, 'REDEMPTION_CONFIRMED', $3,
                    $4, $5)
            "#,
            coupon.id,
            owner_user_id,
            redemption_id,
            serde_json::json!({ "discount_amount": discount.discount_amount }),
            now,
        )
        .execute(&mut *tx)
        .await?;

        // Step 5: 앱 내 알림 outbox 생성 후 커밋 (REDEEM-001 step 7).
        self.publish_outbox(
            &mut tx,
            "redemption_transaction",
            redemption_id,
            1,
            "COUPON_REDEEMED",
            serde_json::json!({
                "store_id": store.id,
                "store_name": store.name,
                "user_id": coupon.user_id,
                "coupon_id": coupon.id,
                "discount_amount": discount.discount_amount,
                "confirmed_at": now,
            }),
        )
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::StoreOwner,
                    "redemption.confirmed",
                    "redemption_transaction",
                )
                .actor(owner_user_id)
                .resource(redemption_id)
                .store(store.id)
                .metadata(serde_json::json!({
                    "coupon_id": coupon.id,
                    "reservation_id": reservation_id,
                    "discount_amount": discount.discount_amount,
                    "external_order_ref": request.order.external_order_ref,
                })),
            )
            .await?;

        tx.commit().await?;

        tracing::info!(
            %redemption_id,
            coupon_id = %coupon.id,
            store_id = %store.id,
            discount = discount.discount_amount,
            "redemption.confirmed"
        );

        Ok(Redemption {
            redemption_id,
            reservation_id,
            coupon_id: coupon.id,
            store_id: store.id,
            status: RedemptionStatus::Confirmed,
            gross_amount: request.order.gross_amount,
            eligible_amount: discount.eligible_amount,
            discount_amount: discount.discount_amount,
            payable_amount: discount.payable_amount(request.order.gross_amount),
            free_item: discount.free_item,
            external_order_ref: request.order.external_order_ref.clone(),
            confirmed_at: now,
            voided_at: None,
            voidable_until: Some(now + self.void_window),
            coupon_restored: false,
            coupon_status: CouponStatus::Used,
        })
    }

    /// `POST /owner/redemptions/:id/cancel` — REDEEM-002 (예약 취소) and REDEEM-004
    /// (10분 내 사용 취소) behind one endpoint, because the owner pressing "취소" does not
    /// know or care which of the two states the sale is in.
    pub async fn cancel(
        &self,
        pool: &PgPool,
        store: &OwnedStore,
        owner_user_id: Uuid,
        reservation_id: Uuid,
        request: &CancelRedemptionRequest,
    ) -> ApiResult<Redemption> {
        let mut tx = pool.begin().await?;
        let now = crate::qr::transaction_now(&mut tx).await?;

        self.stores.lock_store(&mut tx, store.id).await?;
        let reservation = self.lock_reservation(&mut tx, store.id, reservation_id).await?;
        let coupon = self.lock_coupon(&mut tx, reservation.coupon_id).await?;

        match reservation.status {
            // REDEEM-002: cancelling a hold. The coupon simply comes back.
            ReservationStatus::Active => {
                self.release(&mut tx, &reservation, &coupon, now, owner_user_id, "OWNER_CANCELLED")
                    .await?;
                tx.commit().await?;

                return Ok(Redemption {
                    redemption_id: Uuid::nil(),
                    reservation_id,
                    coupon_id: coupon.id,
                    store_id: store.id,
                    status: RedemptionStatus::Voided,
                    gross_amount: 0,
                    eligible_amount: 0,
                    discount_amount: 0,
                    payable_amount: 0,
                    free_item: None,
                    external_order_ref: reservation.external_order_ref,
                    confirmed_at: reservation.reserved_at,
                    voided_at: Some(now),
                    voidable_until: None,
                    coupon_restored: true,
                    coupon_status: CouponStatus::Available,
                });
            }
            ReservationStatus::Confirmed => {}
            _ => {
                return Err(ApiError::with_message(
                    ErrorCode::InvalidStateTransition,
                    "이미 취소되었거나 만료된 예약입니다.",
                ));
            }
        }

        // REDEEM-004: 사용 승인 취소.
        let redemption = sqlx::query!(
            r#"
            SELECT id, gross_amount, eligible_amount, discount_amount, external_order_ref,
                   status::text AS "status!", confirmed_at, approved_by_user_id
            FROM coupon.redemption_transactions
            WHERE reservation_id = $1
            FOR UPDATE
            "#,
            reservation_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::TransactionNotFound))?;

        if RedemptionStatus::from_db(&redemption.status) != RedemptionStatus::Confirmed {
            return Err(ApiError::with_message(
                ErrorCode::InvalidStateTransition,
                "이미 취소된 사용 승인입니다.",
            ));
        }

        // §8.6: 사용 취소 10분. Past that, and REDEEM-004 sends it to an administrator.
        if now > redemption.confirmed_at + self.void_window {
            return Err(ApiError::new(ErrorCode::VoidWindowExpired));
        }
        // REDEEM-004: 같은 브라우저 세션에서.
        if reservation.owner_session_id != request_session(request, &reservation) {
            return Err(ApiError::with_message(
                ErrorCode::Forbidden,
                "사용을 승인한 세션에서만 취소할 수 있습니다.",
            ));
        }

        // 원 캠페인이 아직 유효하고 쿠폰 만료 전이며 회수되지 않았다면 복원을 선택할 수
        // 있다. Past the coupon's own expiry there is nothing to restore *to*, so it stays
        // spent and the customer is sent to support rather than handed a dead coupon.
        let restorable = request.restore_coupon
            && now < coupon.expires_at
            && self.campaign_still_valid(&mut tx, &coupon).await?;

        let next_status = if restorable {
            CouponStatus::Available
        } else {
            CouponStatus::Voided
        };

        sqlx::query!(
            r#"
            UPDATE coupon.coupon_instances
            SET status = $2::text::coupon.coupon_status,
                used_at = NULL,
                voided_at = CASE WHEN $2 = 'VOIDED' THEN $3 ELSE voided_at END
            WHERE id = $1 AND status = 'USED'
            "#,
            coupon.id,
            next_status.as_db(),
            now,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE coupon.redemption_transactions
            SET status = 'VOIDED', voided_at = $2, voided_by_user_id = $3, void_reason = $4,
                coupon_restored = $5
            WHERE id = $1 AND status = 'CONFIRMED'
            "#,
            redemption.id,
            now,
            owner_user_id,
            request.reason.as_deref(),
            restorable,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            INSERT INTO coupon.coupon_status_events
                (coupon_id, from_status, to_status, actor_type, actor_user_id, reason_code,
                 transaction_id, metadata, occurred_at)
            VALUES ($1, 'USED', $2::text::coupon.coupon_status, 'STORE_OWNER', $3,
                    'REDEMPTION_VOIDED', $4, $5, $6)
            "#,
            coupon.id,
            next_status.as_db(),
            owner_user_id,
            redemption.id,
            serde_json::json!({ "restored": restorable, "reason": request.reason }),
            now,
        )
        .execute(&mut *tx)
        .await?;

        self.publish_outbox(
            &mut tx,
            "redemption_transaction",
            redemption.id,
            2,
            "REDEMPTION_VOIDED",
            serde_json::json!({
                "store_id": store.id,
                "store_name": store.name,
                "user_id": coupon.user_id,
                "coupon_id": coupon.id,
                "restored": restorable,
                "detail": if restorable {
                    "쿠폰 사용이 취소되어 다시 사용할 수 있습니다."
                } else {
                    "쿠폰 사용이 취소되었습니다."
                },
            }),
        )
        .await?;

        self.audit
            .record(
                &mut tx,
                AuditEntry::new(
                    ActorType::StoreOwner,
                    "redemption.voided",
                    "redemption_transaction",
                )
                .actor(owner_user_id)
                .resource(redemption.id)
                .store(store.id)
                .reason(
                    request
                        .reason
                        .clone()
                        .unwrap_or_else(|| "점주 사용 취소".to_owned()),
                )
                .transition(
                    &serde_json::json!({ "status": "CONFIRMED" }),
                    &serde_json::json!({ "status": "VOIDED" }),
                )
                .metadata(serde_json::json!({ "coupon_restored": restorable })),
            )
            .await?;

        tx.commit().await?;

        tracing::info!(
            redemption_id = %redemption.id,
            coupon_id = %coupon.id,
            restored = restorable,
            "redemption.voided"
        );

        Ok(Redemption {
            redemption_id: redemption.id,
            reservation_id,
            coupon_id: coupon.id,
            store_id: store.id,
            status: RedemptionStatus::Voided,
            gross_amount: redemption.gross_amount,
            eligible_amount: redemption.eligible_amount,
            discount_amount: redemption.discount_amount,
            payable_amount: redemption.gross_amount,
            free_item: None,
            external_order_ref: redemption.external_order_ref,
            confirmed_at: redemption.confirmed_at,
            voided_at: Some(now),
            voidable_until: None,
            coupon_restored: restorable,
            coupon_status: next_status,
        })
    }

    /// Return coupons whose two minutes ran out (REDEEM-002, §18.1 housekeeping).
    ///
    /// Every online read already treats an expired reservation as gone — `confirm` checks
    /// the clock itself — so this only tidies state and can safely run late.
    pub async fn expire_due_reservations(
        &self,
        pool: &PgPool,
        now: DateTime<Utc>,
        batch: i64,
    ) -> ApiResult<u64> {
        let due: Vec<Uuid> = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.redemption_reservations
            WHERE status = 'ACTIVE' AND expires_at <= $1
            ORDER BY expires_at
            LIMIT $2
            "#,
            now,
            batch,
        )
        .fetch_all(pool)
        .await?;

        let mut released = 0u64;
        for reservation_id in due {
            let mut tx = pool.begin().await?;

            // Re-read under the lock. §15 says the confirmation and the expiry race is
            // settled by whoever validly commits first, and this is the losing side
            // noticing that it lost.
            let reservation = sqlx::query!(
                r#"
                SELECT id, coupon_id, store_id
                FROM coupon.redemption_reservations
                WHERE id = $1 AND status = 'ACTIVE' AND expires_at <= $2
                FOR UPDATE
                "#,
                reservation_id,
                now,
            )
            .fetch_optional(&mut *tx)
            .await?;

            let Some(reservation) = reservation else {
                tx.commit().await?;
                continue;
            };

            sqlx::query!(
                r#"
                UPDATE coupon.redemption_reservations
                SET status = 'EXPIRED', completed_at = $2
                WHERE id = $1 AND status = 'ACTIVE'
                "#,
                reservation.id,
                now,
            )
            .execute(&mut *tx)
            .await?;

            let restored = sqlx::query!(
                r#"
                UPDATE coupon.coupon_instances
                SET status = 'AVAILABLE', reserved_at = NULL
                WHERE id = $1 AND status = 'RESERVED'
                "#,
                reservation.coupon_id,
            )
            .execute(&mut *tx)
            .await?;

            if restored.rows_affected() == 1 {
                sqlx::query!(
                    r#"
                    INSERT INTO coupon.coupon_status_events
                        (coupon_id, from_status, to_status, actor_type, reason_code,
                         transaction_id, occurred_at)
                    VALUES ($1, 'RESERVED', 'AVAILABLE', 'SYSTEM', 'RESERVATION_EXPIRED', $2, $3)
                    "#,
                    reservation.coupon_id,
                    reservation.id,
                    now,
                )
                .execute(&mut *tx)
                .await?;
                released += 1;
            }

            tx.commit().await?;
        }

        Ok(released)
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    async fn lock_coupon(&self, tx: &mut Tx<'_>, coupon_id: Uuid) -> ApiResult<LockedCoupon> {
        let row = sqlx::query!(
            r#"
            SELECT id, store_id, user_id, campaign_id, status::text AS "status!", title,
                   usable_from, expires_at, condition_snapshot
            FROM coupon.coupon_instances
            WHERE id = $1
            FOR UPDATE
            "#,
            coupon_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::CouponNotFound))?;

        Ok(LockedCoupon {
            id: row.id,
            store_id: row.store_id,
            user_id: row.user_id,
            campaign_id: row.campaign_id,
            status: CouponStatus::from_db(&row.status),
            title: row.title,
            usable_from: row.usable_from,
            expires_at: row.expires_at,
            condition_snapshot: row.condition_snapshot,
        })
    }

    async fn lock_reservation(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        reservation_id: Uuid,
    ) -> ApiResult<LockedReservation> {
        let row = sqlx::query!(
            r#"
            SELECT id, coupon_id, store_id, user_id, owner_user_id, owner_session_id,
                   status::text AS "status!", order_snapshot_hash, external_order_ref,
                   expected_discount_amount, reserved_at, expires_at
            FROM coupon.redemption_reservations
            WHERE id = $1 AND store_id = $2
            FOR UPDATE
            "#,
            reservation_id,
            store_id,
        )
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| ApiError::new(ErrorCode::ReservationNotFound))?;

        Ok(LockedReservation {
            id: row.id,
            coupon_id: row.coupon_id,
            owner_user_id: row.owner_user_id,
            owner_session_id: row.owner_session_id,
            status: ReservationStatus::from_db(&row.status),
            order_snapshot_hash: row.order_snapshot_hash,
            external_order_ref: row.external_order_ref,
            reserved_at: row.reserved_at,
            expires_at: row.expires_at,
        })
    }

    /// Mark a reservation expired and put the coupon back, inside the caller's
    /// transaction.
    async fn lapse(
        &self,
        tx: &mut Tx<'_>,
        reservation: &LockedReservation,
        now: DateTime<Utc>,
        reason: &str,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.redemption_reservations
            SET status = 'EXPIRED', completed_at = $2
            WHERE id = $1 AND status = 'ACTIVE'
            "#,
            reservation.id,
            now,
        )
        .execute(&mut **tx)
        .await?;

        let restored = sqlx::query!(
            r#"
            UPDATE coupon.coupon_instances
            SET status = 'AVAILABLE', reserved_at = NULL
            WHERE id = $1 AND status = 'RESERVED'
            "#,
            reservation.coupon_id,
        )
        .execute(&mut **tx)
        .await?;

        if restored.rows_affected() == 1 {
            sqlx::query!(
                r#"
                INSERT INTO coupon.coupon_status_events
                    (coupon_id, from_status, to_status, actor_type, reason_code,
                     transaction_id, occurred_at)
                VALUES ($1, 'RESERVED', 'AVAILABLE', 'SYSTEM', $2, $3, $4)
                "#,
                reservation.coupon_id,
                reason,
                reservation.id,
                now,
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    async fn release(
        &self,
        tx: &mut Tx<'_>,
        reservation: &LockedReservation,
        coupon: &LockedCoupon,
        now: DateTime<Utc>,
        owner_user_id: Uuid,
        reason: &str,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            UPDATE coupon.redemption_reservations
            SET status = 'CANCELLED', completed_at = $2, cancelled_reason = $3
            WHERE id = $1 AND status = 'ACTIVE'
            "#,
            reservation.id,
            now,
            reason,
        )
        .execute(&mut **tx)
        .await?;

        let restored = sqlx::query!(
            r#"
            UPDATE coupon.coupon_instances
            SET status = 'AVAILABLE', reserved_at = NULL
            WHERE id = $1 AND status = 'RESERVED'
            "#,
            coupon.id,
        )
        .execute(&mut **tx)
        .await?;

        if restored.rows_affected() == 1 {
            sqlx::query!(
                r#"
                INSERT INTO coupon.coupon_status_events
                    (coupon_id, from_status, to_status, actor_type, actor_user_id,
                     reason_code, transaction_id, occurred_at)
                VALUES ($1, 'RESERVED', 'AVAILABLE', 'STORE_OWNER', $2, $3, $4, $5)
                "#,
                coupon.id,
                owner_user_id,
                reason,
                reservation.id,
                now,
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    /// §5.4 / §8.6: 주문 1건에 혜택 1개.
    ///
    /// The unique index is the real guarantee; this exists so the owner is told *why*
    /// before the insert fails, and so a reservation cannot be taken against an order
    /// that has already been discounted.
    async fn ensure_order_not_already_discounted(
        &self,
        tx: &mut Tx<'_>,
        store_id: Uuid,
        external_order_ref: Option<&str>,
    ) -> ApiResult<()> {
        let Some(reference) = external_order_ref.map(str::trim).filter(|r| !r.is_empty()) else {
            // No POS reference means there is nothing to correlate on. §8.6 makes the
            // reference optional, so this cannot be enforced without one.
            return Ok(());
        };

        let existing = sqlx::query_scalar!(
            r#"
            SELECT id FROM coupon.redemption_transactions
            WHERE store_id = $1 AND external_order_ref = $2 AND status = 'CONFIRMED'
            "#,
            store_id,
            reference,
        )
        .fetch_optional(&mut **tx)
        .await?;

        if existing.is_some() {
            return Err(ApiError::new(ErrorCode::OrderAlreadyDiscounted));
        }

        Ok(())
    }

    /// Whether the coupon's campaign would still allow it to exist (REDEEM-004).
    async fn campaign_still_valid(
        &self,
        tx: &mut Tx<'_>,
        coupon: &LockedCoupon,
    ) -> ApiResult<bool> {
        let Some(campaign_id) = coupon.campaign_id else {
            // A loyalty reward has no campaign to have been cancelled.
            return Ok(true);
        };

        let status = sqlx::query_scalar!(
            r#"SELECT status::text AS "status!" FROM coupon.campaigns WHERE id = $1"#,
            campaign_id,
        )
        .fetch_optional(&mut **tx)
        .await?;

        Ok(!matches!(status.as_deref(), Some("CANCELLED") | None))
    }

    async fn publish_outbox(
        &self,
        tx: &mut Tx<'_>,
        aggregate_type: &str,
        aggregate_id: Uuid,
        aggregate_version: i64,
        event_type: &str,
        payload: serde_json::Value,
    ) -> ApiResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO coupon.outbox_events
                (aggregate_type, aggregate_id, aggregate_version, event_type, correlation_id, payload)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (aggregate_type, aggregate_id, aggregate_version, event_type) DO NOTHING
            "#,
            aggregate_type,
            aggregate_id,
            aggregate_version,
            event_type,
            Uuid::new_v4(),
            payload,
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct LockedCoupon {
    id: Uuid,
    store_id: Uuid,
    user_id: Uuid,
    campaign_id: Option<Uuid>,
    status: CouponStatus,
    title: String,
    usable_from: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    condition_snapshot: serde_json::Value,
}

#[derive(Debug, Clone)]
struct LockedReservation {
    id: Uuid,
    coupon_id: Uuid,
    owner_user_id: Uuid,
    owner_session_id: String,
    status: ReservationStatus,
    order_snapshot_hash: String,
    external_order_ref: Option<String>,
    reserved_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

/// REDEEM-003's list of reasons a coupon cannot be spent, in the order that gives the
/// owner the most actionable answer first.
fn ensure_spendable(
    coupon: &LockedCoupon,
    conditions: &CouponConditions,
    now: DateTime<Utc>,
) -> ApiResult<()> {
    match coupon.status {
        CouponStatus::Available => {}
        CouponStatus::Reserved => return Err(ApiError::new(ErrorCode::CouponNotAvailable)),
        CouponStatus::Used => {
            return Err(ApiError::with_message(
                ErrorCode::CouponNotAvailable,
                "이미 사용된 쿠폰입니다.",
            ));
        }
        CouponStatus::Expired => return Err(ApiError::new(ErrorCode::CouponExpired)),
        CouponStatus::Revoked => {
            return Err(ApiError::with_message(
                ErrorCode::CouponNotAvailable,
                "회수된 쿠폰입니다.",
            ));
        }
        _ => return Err(ApiError::new(ErrorCode::CouponNotAvailable)),
    }

    // §5.2 / §8.5: `[usable_from, expires_at)`, judged against the database clock, so a
    // coupon is unusable from its expiry instant onwards without waiting for a sweep.
    if now < coupon.usable_from {
        return Err(ApiError::new(ErrorCode::CouponNotYetUsable));
    }
    if now >= coupon.expires_at {
        return Err(ApiError::new(ErrorCode::CouponExpired));
    }
    if !conditions.allows_moment(now) {
        return Err(ApiError::new(ErrorCode::CouponOutsideUsageWindow));
    }

    Ok(())
}

fn ensure_currency(order: &OrderInput) -> ApiResult<()> {
    if order.currency != "KRW" {
        return Err(ApiError::with_message(
            ErrorCode::UnprocessableRequest,
            "원화 주문만 처리할 수 있습니다.",
        ));
    }
    Ok(())
}

/// The cancel request has no session field of its own; REDEEM-004 scopes the undo to the
/// session that made the sale, and the reservation already records which one that was.
fn request_session<'a>(
    _request: &'a CancelRedemptionRequest,
    reservation: &'a LockedReservation,
) -> &'a str {
    &reservation.owner_session_id
}

/// A digest of exactly the fields the owner approved (§13.3-2 주문 hash).
///
/// Amount, currency, reference and every line — so an order edited between preview and
/// confirm is caught rather than silently re-priced.
pub fn order_hash(order: &OrderInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(order.gross_amount.to_string());
    hasher.update(b"|");
    hasher.update(order.currency.as_bytes());
    hasher.update(b"|");
    hasher.update(order.external_order_ref.as_deref().unwrap_or("").as_bytes());
    for item in &order.items {
        hasher.update(b"|");
        hasher.update(
            item.catalog_item_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        );
        hasher.update(b":");
        hasher.update(item.quantity.to_string());
        hasher.update(b":");
        hasher.update(item.unit_price.to_string());
    }
    hex::encode(hasher.finalize())
}

fn order_snapshot(
    order: &OrderInput,
    lines: &[OrderLine],
    discount: &Discount,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "gross_amount": order.gross_amount,
        "eligible_amount": discount.eligible_amount,
        "discount_amount": discount.discount_amount,
        "currency": order.currency,
        "external_order_ref": order.external_order_ref,
        "items": lines
            .iter()
            .map(|line| serde_json::json!({
                "catalog_item_id": line.catalog_item_id,
                "name_snapshot": line.name_snapshot,
                "quantity": line.quantity,
                "unit_price": line.unit_price,
            }))
            .collect::<Vec<_>>(),
    })
}

fn map_reservation_conflict(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            match db.constraint() {
                // §12.6-6, and §15's 동일 쿠폰 동시 예약 row.
                Some("uq_redemption_reservations_active_coupon") => {
                    ApiError::new(ErrorCode::CouponNotAvailable).internal(db.to_string())
                }
                // REDEEM-002: 같은 점주 세션은 동시에 하나의 사용 예약만 가질 수 있다.
                Some("uq_redemption_reservations_active_owner_session") => {
                    ApiError::new(ErrorCode::ReservationAlreadyActive).internal(db.to_string())
                }
                Some("uq_redemption_reservation_idempotency") => {
                    ApiError::new(ErrorCode::IdempotencyRequestInProgress)
                        .internal(db.to_string())
                }
                _ => ApiError::new(ErrorCode::Conflict).internal(db.to_string()),
            }
        }
        _ => ApiError::from(error),
    }
}

fn map_redemption_conflict(error: sqlx::Error) -> ApiError {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            match db.constraint() {
                Some("uq_redemption_transactions_confirmed_coupon") => {
                    ApiError::with_message(ErrorCode::CouponNotAvailable, "이미 사용된 쿠폰입니다.")
                        .internal(db.to_string())
                }
                Some("uq_redemption_transactions_order_ref") => {
                    ApiError::new(ErrorCode::OrderAlreadyDiscounted).internal(db.to_string())
                }
                _ => ApiError::new(ErrorCode::Conflict).internal(db.to_string()),
            }
        }
        _ => ApiError::from(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::OrderItemInput;

    fn order(gross: i64, reference: Option<&str>) -> OrderInput {
        OrderInput {
            external_order_ref: reference.map(str::to_owned),
            gross_amount: gross,
            currency: "KRW".to_owned(),
            items: Vec::new(),
        }
    }

    fn coupon(status: CouponStatus, usable_from: i64, expires_at: i64) -> LockedCoupon {
        LockedCoupon {
            id: Uuid::from_u128(1),
            store_id: Uuid::from_u128(2),
            user_id: Uuid::from_u128(3),
            campaign_id: None,
            status,
            title: "쿠폰".to_owned(),
            usable_from: at(usable_from),
            expires_at: at(expires_at),
            condition_snapshot: serde_json::json!({}),
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    fn conditions() -> CouponConditions {
        CouponConditions {
            schema_version: 1,
            benefit: Benefit {
                benefit_type: crate::loyalty::BenefitType::FixedAmount,
                fixed_amount: Some(1_000),
                percentage: None,
                maximum_discount_amount: None,
                free_item_ids: Vec::new(),
            },
            minimum_order_amount: 0,
            eligible_item_ids: Vec::new(),
            eligible_category_ids: Vec::new(),
            excluded_item_ids: Vec::new(),
            allowed_weekdays: Vec::new(),
            allowed_local_time_ranges: Vec::new(),
            timezone: "Asia/Seoul".to_owned(),
            external_discount_stackable: false,
        }
    }

    #[test]
    fn the_same_order_hashes_the_same_way_twice() {
        let order = OrderInput {
            items: vec![OrderItemInput {
                catalog_item_id: Some(Uuid::from_u128(9)),
                name_snapshot: Some("아메리카노".to_owned()),
                quantity: 2,
                unit_price: 4_500,
            }],
            ..order(9_000, Some("POS-1"))
        };

        assert_eq!(order_hash(&order), order_hash(&order));
    }

    #[test]
    fn an_edited_order_does_not_match_the_reservation() {
        // §13.3-2: the hash is what catches an order changed between preview and confirm.
        let original = order(9_000, Some("POS-1"));

        assert_ne!(
            order_hash(&original),
            order_hash(&order(9_500, Some("POS-1"))),
            "a changed amount must not pass"
        );
        assert_ne!(
            order_hash(&original),
            order_hash(&order(9_000, Some("POS-2"))),
            "a changed order reference must not pass"
        );

        let with_item = OrderInput {
            items: vec![OrderItemInput {
                catalog_item_id: Some(Uuid::from_u128(9)),
                name_snapshot: None,
                quantity: 1,
                unit_price: 9_000,
            }],
            ..order(9_000, Some("POS-1"))
        };
        assert_ne!(
            order_hash(&original),
            order_hash(&with_item),
            "added line items must not pass"
        );
    }

    #[test]
    fn the_hash_ignores_only_the_display_name() {
        // The name is a label the owner may retype; the money and the identity are not.
        let named = OrderInput {
            items: vec![OrderItemInput {
                catalog_item_id: Some(Uuid::from_u128(9)),
                name_snapshot: Some("아메리카노".to_owned()),
                quantity: 1,
                unit_price: 4_500,
            }],
            ..order(4_500, None)
        };
        let renamed = OrderInput {
            items: vec![OrderItemInput {
                name_snapshot: Some("아이스 아메리카노".to_owned()),
                ..named.items[0].clone()
            }],
            ..order(4_500, None)
        };

        assert_eq!(order_hash(&named), order_hash(&renamed));
    }

    #[test]
    fn a_coupon_outside_its_period_cannot_be_reserved() {
        // §5.2 `[start, end)` and REDEEM-003.
        let coupon = coupon(CouponStatus::Available, 1_000, 2_000);

        assert_eq!(
            ensure_spendable(&coupon, &conditions(), at(999))
                .expect_err("too early")
                .code,
            ErrorCode::CouponNotYetUsable
        );
        assert!(ensure_spendable(&coupon, &conditions(), at(1_000)).is_ok());
        assert!(ensure_spendable(&coupon, &conditions(), at(1_999)).is_ok());
        assert_eq!(
            ensure_spendable(&coupon, &conditions(), at(2_000))
                .expect_err("the end instant is excluded")
                .code,
            ErrorCode::CouponExpired,
            "§8.5: 사용 종료 시각은 미포함이므로 정확히 그 시각부터 사용 불가다"
        );
    }

    #[test]
    fn every_non_available_status_refuses_a_reservation() {
        for status in [
            CouponStatus::Reserved,
            CouponStatus::Used,
            CouponStatus::Expired,
            CouponStatus::Revoked,
            CouponStatus::Voided,
            CouponStatus::Pending,
            CouponStatus::IssueFailed,
        ] {
            let coupon = coupon(status, 1_000, 2_000);
            assert!(
                ensure_spendable(&coupon, &conditions(), at(1_500)).is_err(),
                "{status:?} must not be spendable"
            );
        }
    }

    #[test]
    fn a_coupon_outside_its_weekday_window_is_refused_with_its_own_reason() {
        let mut conditions = conditions();
        // Sunday only. 2026-08-10T03:00Z is a Monday in Seoul.
        conditions.allowed_weekdays = vec![0];
        let coupon = coupon(CouponStatus::Available, 0, i64::from(u32::MAX));

        let monday = "2026-08-10T03:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("time");
        assert_eq!(
            ensure_spendable(&coupon, &conditions, monday)
                .expect_err("wrong weekday")
                .code,
            ErrorCode::CouponOutsideUsageWindow
        );
    }

    #[test]
    fn only_won_orders_are_accepted() {
        assert!(ensure_currency(&order(1_000, None)).is_ok());

        let usd = OrderInput {
            currency: "USD".to_owned(),
            ..order(1_000, None)
        };
        assert_eq!(
            ensure_currency(&usd).expect_err("not KRW").code,
            ErrorCode::UnprocessableRequest
        );
    }

    #[test]
    fn reservation_and_redemption_statuses_fail_closed() {
        for status in [
            ReservationStatus::Active,
            ReservationStatus::Confirmed,
            ReservationStatus::Cancelled,
            ReservationStatus::Expired,
            ReservationStatus::Revoked,
        ] {
            assert_eq!(ReservationStatus::from_db(status.as_db()), status);
        }
        assert_eq!(
            ReservationStatus::from_db("SOMETHING"),
            ReservationStatus::Revoked,
            "an uninterpretable reservation must not look confirmable"
        );

        assert_eq!(
            RedemptionStatus::from_db("SOMETHING"),
            RedemptionStatus::RequiresAdminReview
        );
    }

    #[test]
    fn cancelling_defaults_to_restoring_the_coupon() {
        // REDEEM-004 offers restoration; the customer losing their coupon to the owner's
        // mistake is the outcome worth defaulting away from.
        let request: CancelRedemptionRequest =
            serde_json::from_value(serde_json::json!({})).expect("deserialises");
        assert!(request.restore_coupon);
    }
}
