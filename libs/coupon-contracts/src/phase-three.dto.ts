import type {
  Currency,
  MoneyDto,
  Rfc3339Timestamp,
  Uuid,
} from "./phase-one.dto";
import type {
  CouponBenefitType,
  CouponWalletStatus,
  StampOrderDto,
  VersionedDto,
} from "./phase-two.dto";

export type CampaignStatus =
  | "DRAFT"
  | "SCHEDULED"
  | "ISSUING"
  | "ACTIVE"
  | "PAUSED"
  | "ENDED"
  | "CANCELLED";

export type CampaignIssuanceMethod = "FIRST_COME" | "TARGETED" | "DIRECT";

export type CampaignBenefitDto =
  | {
      type: "FIXED";
      discount_amount: number;
      currency: Currency;
    }
  | {
      type: "PERCENTAGE";
      percentage: number;
      maximum_discount_amount: number;
      currency: Currency;
    }
  | {
      type: "FREE_ITEM";
      eligible_catalog_item_ids: Uuid[];
    };

export interface OwnerCampaignDto extends VersionedDto {
  id: Uuid;
  name: string;
  status: CampaignStatus;
  issuance_method: CampaignIssuanceMethod;
  benefit: CampaignBenefitDto;
  benefit_label: string;
  issuance_starts_at: Rfc3339Timestamp;
  issuance_ends_at: Rfc3339Timestamp;
  usable_from: Rfc3339Timestamp;
  usable_until: Rfc3339Timestamp;
  total_quantity: number | null;
  per_user_quantity: number;
  per_business_day_quantity: number | null;
  issued_count: number;
  used_count: number;
  snapshot_target_count: number | null;
  processed_count: number;
  failed_count: number;
  immutable_fields: string[];
  request_id: string;
}

export interface OwnerCampaignListResponseDto extends VersionedDto {
  items: OwnerCampaignDto[];
  next_cursor: string | null;
  request_id: string;
}

export interface SaveCampaignRequestDto {
  name: string;
  benefit: CampaignBenefitDto;
  minimum_order_amount: MoneyDto;
  eligible_catalog_item_ids: Uuid[];
  excluded_catalog_item_ids: Uuid[];
  audience_type: "ALL_FAVORITES" | "SEGMENT" | "SPECIFIC_CUSTOMERS";
  issuance_method: CampaignIssuanceMethod;
  total_quantity: number | null;
  per_user_quantity: number;
  per_business_day_quantity: number | null;
  issuance_starts_at: Rfc3339Timestamp;
  issuance_ends_at: Rfc3339Timestamp;
  usable_from: Rfc3339Timestamp;
  usable_until: Rfc3339Timestamp;
  notify_in_app: boolean;
  notify_push: boolean;
  restore_quantity_on_revoke: boolean;
  version?: number;
}

export interface CampaignImpactActionRequestDto {
  confirmation_phrase: string;
  reason: string;
  revoke_issued_coupons: boolean;
}

export interface PublicCampaignDto {
  id: Uuid;
  name: string;
  benefit_type: CouponBenefitType;
  benefit_label: string;
  minimum_order_amount: MoneyDto;
  item_restriction_summary: string | null;
  issuance_ends_at: Rfc3339Timestamp;
  usable_until: Rfc3339Timestamp;
  remaining_quantity: number | null;
  claimed_coupon_id: Uuid | null;
}

export interface PublicStoreDetailDto extends VersionedDto {
  id: Uuid;
  slug: string;
  name: string;
  introduction: string;
  address_summary: string;
  business_hours_summary: string;
  currently_open: boolean;
  status: "ACTIVE" | "SUSPENDED" | "CLOSED";
  is_favorite: boolean;
  loyalty_policy_summary: string | null;
  campaigns: PublicCampaignDto[];
  request_id: string;
}

export interface CampaignClaimResponseDto {
  coupon_id: Uuid;
  outcome: "ISSUED" | "ALREADY_CLAIMED";
  status: CouponWalletStatus;
  request_id: string;
  transaction_id: Uuid;
}

export interface RedemptionPreviewRequestDto {
  scan_session_id: Uuid;
  coupon_id: Uuid;
  order: StampOrderDto;
}

export interface RedemptionPreviewResponseDto {
  redemption_id: Uuid;
  coupon_id: Uuid;
  coupon_inquiry_reference: string;
  customer_reference_masked: string;
  display_name_masked: string;
  benefit_label: string;
  expected_discount_amount: number;
  payable_amount: number;
  currency: Currency;
  conditions: string[];
  reserved_at: Rfc3339Timestamp;
  reservation_expires_at: Rfc3339Timestamp;
  request_id: string;
}

export interface ConfirmRedemptionRequestDto {
  order: StampOrderDto;
}

export interface RedemptionResponseDto {
  transaction_id: Uuid;
  redemption_id: Uuid;
  coupon_id: Uuid;
  discount_amount: number;
  payable_amount: number;
  currency: Currency;
  status: "USED" | "CANCELLED";
  processed_at: Rfc3339Timestamp;
  cancellable_until: Rfc3339Timestamp | null;
  request_id: string;
}

export interface CancelRedemptionRequestDto {
  reason: string;
  restore_if_eligible: boolean;
}

export interface AdminCampaignDto extends VersionedDto {
  id: Uuid;
  name: string;
  store_name: string;
  status: CampaignStatus;
  snapshot_target_count: number | null;
  processed_count: number;
  issued_count: number;
  used_count: number;
  estimated_revoke_count: number;
  reversible_after_stop: boolean;
}

export interface AdminCampaignListResponseDto extends VersionedDto {
  items: AdminCampaignDto[];
  next_cursor: string | null;
  request_id: string;
}

export interface AdminEmergencyCampaignRequestDto {
  action: "EMERGENCY_STOP" | "REVOKE";
  reason: string;
  understood_reversibility: boolean;
  case_id?: Uuid | null;
}

export interface AdminJobDto extends VersionedDto {
  id: Uuid;
  job_key: string;
  job_type: string;
  status: "QUEUED" | "RUNNING" | "RETRYING" | "SUCCEEDED" | "FAILED";
  attempts: number;
  max_attempts: number;
  checkpoint: string | null;
  last_error: string | null;
  retryable: boolean;
}

export interface AdminJobListResponseDto extends VersionedDto {
  items: AdminJobDto[];
  next_cursor: string | null;
  request_id: string;
}

export interface RetryAdminJobRequestDto {
  reason: string;
}
