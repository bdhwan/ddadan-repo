import type {
  Currency,
  MoneyDto,
  Rfc3339Timestamp,
  Uuid,
} from "./phase-one.dto";

export interface VersionedDto {
  version: number;
  updated_at: Rfc3339Timestamp;
}

export type CouponWalletStatus =
  | "PENDING"
  | "AVAILABLE"
  | "RESERVED"
  | "USED"
  | "EXPIRED"
  | "REVOKED"
  | "VOIDED";

export type CouponBenefitType =
  | "FIXED"
  | "PERCENTAGE"
  | "FREE_ITEM"
  | "STAMP_REWARD";

export interface WalletCouponDto extends VersionedDto {
  id: Uuid;
  inquiry_reference: string;
  store_id: Uuid;
  store_name: string;
  campaign_name: string;
  benefit_type: CouponBenefitType;
  benefit_label: string;
  status: CouponWalletStatus;
  minimum_order_amount: MoneyDto;
  item_restriction_summary: string | null;
  conditions: string[];
  issued_reason: string;
  issued_at: Rfc3339Timestamp;
  usable_from: Rfc3339Timestamp;
  expires_at: Rfc3339Timestamp;
  used_at: Rfc3339Timestamp | null;
  expired_at: Rfc3339Timestamp | null;
  revoked_at: Rfc3339Timestamp | null;
  terminal_reason: string | null;
}

export interface WalletCouponListResponseDto extends VersionedDto {
  items: WalletCouponDto[];
  next_cursor: string | null;
  request_id: string;
}

export interface WalletStampBoardDto extends VersionedDto {
  store_id: Uuid;
  store_name: string;
  available_stamps: number;
  goal_stamps: number;
  earliest_stamp_expires_at: Rfc3339Timestamp | null;
  reward_description: string;
  policy_status: "ACTIVE" | "ENDED";
}

export interface WalletStampListResponseDto extends VersionedDto {
  items: WalletStampBoardDto[];
  next_cursor: string | null;
  request_id: string;
}

export interface QrTokenResponseDto {
  qr_token: string;
  qr_payload: string;
  auxiliary_code: string;
  issued_at: Rfc3339Timestamp;
  expires_at: Rfc3339Timestamp;
  refresh_after_seconds: number;
  request_id: string;
}

export interface ConsumerNotificationDto extends VersionedDto {
  id: Uuid;
  category: "TRANSACTION" | "BENEFIT" | "SECURITY" | "OPERATIONS";
  title: string;
  body: string;
  read_at: Rfc3339Timestamp | null;
  created_at: Rfc3339Timestamp;
}

export interface ConsumerNotificationListResponseDto extends VersionedDto {
  items: ConsumerNotificationDto[];
  next_cursor: string | null;
  request_id: string;
}

export interface ScanResolveRequestDto {
  qr_token?: string;
  auxiliary_code?: string;
}

export interface ResolvedCustomerDto {
  scan_session_id: Uuid;
  customer_reference_masked: string;
  display_name_masked: string;
  available_stamp_count: number;
  qr_expires_at: Rfc3339Timestamp;
  policy_version: number;
  request_id: string;
}

export interface OrderItemDto {
  catalog_item_id: Uuid | null;
  name_snapshot: string;
  quantity: number;
  unit_price: number;
}

export interface StampOrderDto {
  external_order_ref: string | null;
  gross_amount: number;
  currency: Currency;
  items: OrderItemDto[];
}

export interface StampPreviewRequestDto {
  scan_session_id: Uuid;
  order: StampOrderDto;
}

export interface StampPreviewResponseDto {
  preview_id: Uuid;
  customer_reference_masked: string;
  display_name_masked: string;
  expected_stamp_count: number;
  balance_after: number;
  stamp_expires_at: Rfc3339Timestamp;
  reward_description: string;
  limits: string[];
  duplicate_warning: string | null;
  request_id: string;
}

export interface CreateStampTransactionRequestDto {
  qr_token?: string;
  auxiliary_code?: string;
  preview_id: Uuid;
  order: StampOrderDto;
}

export interface StampTransactionResponseDto {
  transaction_id: Uuid;
  stamp_count: number;
  balance_after: number;
  reward_issued: boolean;
  reward_description: string | null;
  processed_at: Rfc3339Timestamp;
  request_id: string;
}

export type LoyaltyPolicyStatus =
  | "DRAFT"
  | "SCHEDULED"
  | "ACTIVE"
  | "PAUSED"
  | "ENDED";

export interface LoyaltyPolicyDto extends VersionedDto {
  id: Uuid;
  policy_version: number;
  status: LoyaltyPolicyStatus;
  goal_stamps: number;
  stamps_per_order: number;
  minimum_order_amount: MoneyDto;
  per_business_day_limit: number | null;
  stamp_validity_days: number;
  reward_validity_days: number;
  duplicate_warning_minutes: number;
  reward_description: string;
  eligible_catalog_item_ids: Uuid[];
  starts_at: Rfc3339Timestamp | null;
  ends_at: Rfc3339Timestamp | null;
}

export interface LoyaltyPolicyListResponseDto {
  items: LoyaltyPolicyDto[];
  request_id: string;
}

export interface SaveLoyaltyPolicyRequestDto {
  goal_stamps: number;
  stamps_per_order: number;
  minimum_order_amount: MoneyDto;
  per_business_day_limit: number | null;
  stamp_validity_days: number;
  reward_validity_days: number;
  duplicate_warning_minutes: number;
  reward_description: string;
  eligible_catalog_item_ids: Uuid[];
  version?: number;
}

export interface PublishLoyaltyPolicyRequestDto {
  publish_at: Rfc3339Timestamp | null;
  version: number;
}

export interface CatalogItemDto extends VersionedDto {
  id: Uuid;
  name: string;
  sku: string | null;
  category: string;
  active: boolean;
  reference_price: MoneyDto | null;
}

export interface CatalogItemListResponseDto extends VersionedDto {
  items: CatalogItemDto[];
  request_id: string;
}

export interface SaveCatalogItemRequestDto {
  name: string;
  sku: string | null;
  category: string;
  active: boolean;
  reference_price: MoneyDto | null;
  version?: number;
}

export interface OwnerDashboardMetricDto {
  value: number | null;
  aggregation_status: "READY" | "PENDING";
}

export interface OwnerDashboardResponseDto extends VersionedDto {
  earned: OwnerDashboardMetricDto;
  redeemed: OwnerDashboardMetricDto;
  voided: OwnerDashboardMetricDto;
  active_campaign_count: number;
  queue_health: "HEALTHY" | "DELAYED" | "ERROR";
  delivery_health: "HEALTHY" | "DELAYED" | "ERROR";
  request_id: string;
}

export interface OwnerCampaignProgressDto extends VersionedDto {
  id: Uuid;
  name: string;
  status: "DRAFT" | "SCHEDULED" | "ISSUING" | "PAUSED" | "ENDED" | "CANCELLED";
  snapshot_target_count: number | null;
  processed_count: number;
  issued_count: number;
  failed_count: number;
}

export interface OwnerCampaignProgressListResponseDto extends VersionedDto {
  items: OwnerCampaignProgressDto[];
  request_id: string;
}

export type AdminLedgerKind = "EARN" | "REDEEM" | "VOID" | "ADJUSTMENT";

export interface AdminLedgerEntryDto {
  id: Uuid;
  kind: AdminLedgerKind;
  amount: number;
  occurred_at: Rfc3339Timestamp;
  reason: string;
  actor_reference_masked: string;
}

export interface AdminTransactionTimelineEventDto {
  id: Uuid;
  status: string;
  title: string;
  description: string;
  occurred_at: Rfc3339Timestamp;
  request_id: string | null;
}

export interface AdminTransactionDetailDto {
  transaction_id: Uuid;
  transaction_type: AdminLedgerKind;
  status: string;
  store_name: string;
  store_reference_masked: string;
  customer_reference_masked: string;
  external_order_ref_masked: string | null;
  gross_amount: MoneyDto | null;
  ledgers: AdminLedgerEntryDto[];
  timeline: AdminTransactionTimelineEventDto[];
  created_at: Rfc3339Timestamp;
  updated_at: Rfc3339Timestamp;
  version: number;
  request_id: string;
}
