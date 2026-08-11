import type { CampaignClaimResponseDto } from "@coupon/contracts";

export type ClaimStatus =
  | "idle"
  | "claiming"
  | "claimed"
  | "duplicate"
  | "sold_out"
  | "error";

export interface CampaignClaimState {
  status: ClaimStatus;
  idempotency_key: string | null;
  coupon_id: string | null;
  optimistic_claimed: boolean;
  message: string | null;
}

export const INITIAL_CLAIM_STATE: CampaignClaimState = {
  status: "idle",
  idempotency_key: null,
  coupon_id: null,
  optimistic_claimed: false,
  message: null,
};

export function beginCampaignClaim(
  state: CampaignClaimState,
  createKey: () => string,
): CampaignClaimState {
  if (state.status === "claiming" || state.coupon_id) return state;
  return {
    ...state,
    status: "claiming",
    idempotency_key: state.idempotency_key ?? createKey(),
    optimistic_claimed: true,
    message: "지갑에 반영하는 중입니다.",
  };
}

export function resolveCampaignClaim(
  state: CampaignClaimState,
  response: CampaignClaimResponseDto,
): CampaignClaimState {
  return {
    ...state,
    status: response.outcome === "ISSUED" ? "claimed" : "duplicate",
    coupon_id: response.coupon_id,
    optimistic_claimed: true,
    message:
      response.outcome === "ISSUED"
        ? "쿠폰을 받았습니다. 지갑에서 확인하세요."
        : "이미 받은 캠페인입니다. 기존 쿠폰으로 안내합니다.",
  };
}

export function rejectCampaignClaim(
  state: CampaignClaimState,
  code: string,
  message: string,
): CampaignClaimState {
  if (code === "CAMPAIGN_SOLD_OUT") {
    return {
      ...state,
      status: "sold_out",
      optimistic_claimed: false,
      message: "준비된 쿠폰이 모두 소진되었습니다 (CAMPAIGN_SOLD_OUT).",
    };
  }
  return {
    ...state,
    status: "error",
    optimistic_claimed: false,
    message,
  };
}
