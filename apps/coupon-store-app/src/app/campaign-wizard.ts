import type {
  CampaignBenefitDto,
  CampaignIssuanceMethod,
  SaveCampaignRequestDto,
} from "@coupon/contracts";
import { previewDiscount, type OrderItem } from "@coupon/domain";

export const CAMPAIGN_WIZARD_STEPS = [
  "benefit",
  "conditions",
  "audience",
  "quantity",
  "schedule",
  "notification",
  "review",
] as const;

export type CampaignWizardStep = (typeof CAMPAIGN_WIZARD_STEPS)[number];
export type CampaignBenefitType = CampaignBenefitDto["type"];

export const IMMUTABLE_AFTER_ISSUANCE = [
  "혜택 유형과 할인 값",
  "최소 주문액·품목 조건 스냅샷",
  "회원별 및 영업일별 수량",
  "발급 후 계산된 사용 시작·만료 시각",
  "회수 시 수량 복원 정책",
] as const;

export interface CampaignDraft {
  name: string;
  benefit_type: CampaignBenefitType;
  fixed_discount_amount: number;
  percentage: number;
  maximum_discount_amount: number;
  free_item_ids: string[];
  minimum_order_amount: number;
  eligible_item_ids: string[];
  excluded_item_ids: string[];
  audience_type: SaveCampaignRequestDto["audience_type"];
  issuance_method: CampaignIssuanceMethod;
  total_quantity: number | null;
  per_user_quantity: number;
  per_business_day_quantity: number | null;
  issuance_starts_at: string;
  issuance_ends_at: string;
  usable_from: string;
  usable_until: string;
  notify_in_app: boolean;
  notify_push: boolean;
  restore_quantity_on_revoke: boolean;
  estimated_audience: number;
}

export function createCampaignDraft(now = new Date()): CampaignDraft {
  const issueStart = new Date(now.getTime() + 60 * 60_000);
  const issueEnd = new Date(now.getTime() + 7 * 86_400_000);
  const usableEnd = new Date(now.getTime() + 30 * 86_400_000);
  return {
    name: "",
    benefit_type: "FIXED",
    fixed_discount_amount: 1_000,
    percentage: 10,
    maximum_discount_amount: 5_000,
    free_item_ids: [],
    minimum_order_amount: 0,
    eligible_item_ids: [],
    excluded_item_ids: [],
    audience_type: "ALL_FAVORITES",
    issuance_method: "FIRST_COME",
    total_quantity: 100,
    per_user_quantity: 1,
    per_business_day_quantity: null,
    issuance_starts_at: toLocalInput(issueStart),
    issuance_ends_at: toLocalInput(issueEnd),
    usable_from: toLocalInput(issueStart),
    usable_until: toLocalInput(usableEnd),
    notify_in_app: true,
    notify_push: false,
    restore_quantity_on_revoke: false,
    estimated_audience: 0,
  };
}

export function validateCampaignStep(
  draft: CampaignDraft,
  step: CampaignWizardStep,
): string[] {
  switch (step) {
    case "benefit":
      if (!draft.name.trim()) return ["캠페인 이름을 입력해 주세요."];
      if (
        draft.benefit_type === "FIXED" &&
        !isPositiveWon(draft.fixed_discount_amount)
      ) {
        return ["정액 할인액은 1원 이상의 정수여야 합니다."];
      }
      if (draft.benefit_type === "PERCENTAGE") {
        const errors: string[] = [];
        if (
          !Number.isInteger(draft.percentage) ||
          draft.percentage < 1 ||
          draft.percentage > 100
        ) {
          errors.push("할인율은 1~100% 정수여야 합니다.");
        }
        if (!isPositiveWon(draft.maximum_discount_amount)) {
          errors.push("최대 할인액은 1원 이상의 정수여야 합니다.");
        }
        return errors;
      }
      return draft.benefit_type === "FREE_ITEM" &&
        draft.free_item_ids.length === 0
        ? ["무료 품목을 하나 이상 선택해 주세요."]
        : [];
    case "conditions": {
      const errors: string[] = [];
      if (!isNonNegativeWon(draft.minimum_order_amount)) {
        errors.push("최소 주문액은 0원 이상의 정수여야 합니다.");
      }
      const excluded = new Set(draft.excluded_item_ids);
      if (draft.eligible_item_ids.some((id) => excluded.has(id))) {
        errors.push("같은 품목을 대상과 제외에 동시에 넣을 수 없습니다.");
      }
      return errors;
    }
    case "audience":
      return draft.estimated_audience < 0
        ? ["예상 대상 수는 0명 이상이어야 합니다."]
        : [];
    case "quantity": {
      const errors: string[] = [];
      if (
        draft.total_quantity !== null &&
        (!isPositiveInt(draft.total_quantity) ||
          draft.total_quantity > 1_000_000)
      ) {
        errors.push(
          "총 발급 수량은 1~1,000,000이거나 운영 상한 내 무제한이어야 합니다.",
        );
      }
      if (!isPositiveInt(draft.per_user_quantity)) {
        errors.push("회원별 수량은 1 이상이어야 합니다.");
      }
      if (
        draft.per_business_day_quantity !== null &&
        !isPositiveInt(draft.per_business_day_quantity)
      ) {
        errors.push("영업일별 수량은 1 이상이어야 합니다.");
      }
      return errors;
    }
    case "schedule": {
      const timestamps = [
        draft.issuance_starts_at,
        draft.issuance_ends_at,
        draft.usable_from,
        draft.usable_until,
      ].map(Date.parse);
      if (timestamps.some(Number.isNaN)) return ["모든 일정을 입력해 주세요."];
      const [issueStart, issueEnd, useStart, useEnd] = timestamps;
      const errors: string[] = [];
      if (issueStart >= issueEnd) {
        errors.push("발급 종료는 발급 시작보다 늦어야 합니다.");
      }
      if (useStart >= useEnd) {
        errors.push("사용 종료는 사용 시작보다 늦어야 합니다.");
      }
      if (useEnd <= issueStart) {
        errors.push("사용 종료는 발급 시작보다 늦어야 합니다.");
      }
      return errors;
    }
    case "notification":
      return [];
    case "review":
      return CAMPAIGN_WIZARD_STEPS.slice(0, -1).flatMap((candidate) =>
        validateCampaignStep(draft, candidate),
      );
  }
}

export function campaignBenefit(draft: CampaignDraft): CampaignBenefitDto {
  switch (draft.benefit_type) {
    case "FIXED":
      return {
        type: "FIXED",
        discount_amount: draft.fixed_discount_amount,
        currency: "KRW",
      };
    case "PERCENTAGE":
      return {
        type: "PERCENTAGE",
        percentage: draft.percentage,
        maximum_discount_amount: draft.maximum_discount_amount,
        currency: "KRW",
      };
    case "FREE_ITEM":
      return {
        type: "FREE_ITEM",
        eligible_catalog_item_ids: draft.free_item_ids,
      };
  }
}

export function previewCampaignDiscount(
  draft: CampaignDraft,
  targetAmount: number,
  items: readonly OrderItem[] = [],
): number {
  const benefit = campaignBenefit(draft);
  const domainBenefit =
    benefit.type === "FIXED"
      ? benefit
      : benefit.type === "PERCENTAGE"
        ? benefit
        : {
            type: "FREE_ITEM" as const,
            eligible_item_ids: benefit.eligible_catalog_item_ids,
          };
  return previewDiscount(targetAmount, domainBenefit, items).discount_amount;
}

export function maximumCampaignExposure(draft: CampaignDraft): number | null {
  if (draft.total_quantity === null || draft.benefit_type === "FREE_ITEM") {
    return null;
  }
  const maximumPerCoupon =
    draft.benefit_type === "FIXED"
      ? draft.fixed_discount_amount
      : draft.maximum_discount_amount;
  const result = maximumPerCoupon * draft.total_quantity;
  return Number.isSafeInteger(result) ? result : null;
}

export function estimatedNotificationCount(draft: CampaignDraft): number {
  const channels = Number(draft.notify_in_app) + Number(draft.notify_push);
  return draft.estimated_audience * channels;
}

export function toSaveCampaignRequest(
  draft: CampaignDraft,
): SaveCampaignRequestDto {
  return {
    name: draft.name.trim(),
    benefit: campaignBenefit(draft),
    minimum_order_amount: {
      amount: draft.minimum_order_amount,
      currency: "KRW",
    },
    eligible_catalog_item_ids: draft.eligible_item_ids,
    excluded_catalog_item_ids: draft.excluded_item_ids,
    audience_type: draft.audience_type,
    issuance_method: draft.issuance_method,
    total_quantity: draft.total_quantity,
    per_user_quantity: draft.per_user_quantity,
    per_business_day_quantity: draft.per_business_day_quantity,
    issuance_starts_at: new Date(draft.issuance_starts_at).toISOString(),
    issuance_ends_at: new Date(draft.issuance_ends_at).toISOString(),
    usable_from: new Date(draft.usable_from).toISOString(),
    usable_until: new Date(draft.usable_until).toISOString(),
    notify_in_app: draft.notify_in_app,
    notify_push: draft.notify_push,
    restore_quantity_on_revoke: draft.restore_quantity_on_revoke,
  };
}

function toLocalInput(date: Date): string {
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function isPositiveWon(value: number): boolean {
  return isNonNegativeWon(value) && value > 0;
}

function isNonNegativeWon(value: number): boolean {
  return Number.isSafeInteger(value) && value >= 0;
}

function isPositiveInt(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0;
}
