import type { CampaignClaimResponseDto } from "@coupon/contracts";
import { describe, expect, it } from "vitest";
import {
  INITIAL_CLAIM_STATE,
  beginCampaignClaim,
  rejectCampaignClaim,
  resolveCampaignClaim,
} from "./store-detail-state";

const response: CampaignClaimResponseDto = {
  coupon_id: "coupon-1",
  outcome: "ISSUED",
  status: "AVAILABLE",
  request_id: "req-1",
  transaction_id: "tx-1",
};

describe("선착순 캠페인 받기", () => {
  it("성공하면 낙관적 표시를 유지하고 지갑 쿠폰을 연결한다", () => {
    const claiming = beginCampaignClaim(INITIAL_CLAIM_STATE, () => "key-1");
    const claimed = resolveCampaignClaim(claiming, response);
    expect(claimed).toMatchObject({
      status: "claimed",
      coupon_id: "coupon-1",
      optimistic_claimed: true,
      idempotency_key: "key-1",
    });
  });

  it("중복 요청은 서버가 준 기존 쿠폰으로 안내한다", () => {
    const claiming = beginCampaignClaim(INITIAL_CLAIM_STATE, () => "key-2");
    const duplicate = resolveCampaignClaim(claiming, {
      ...response,
      outcome: "ALREADY_CLAIMED",
    });
    expect(duplicate.status).toBe("duplicate");
    expect(duplicate.message).toMatch(/기존 쿠폰/);
  });

  it("소진되면 낙관적 표시를 되돌리고 코드를 보여 준다", () => {
    const claiming = beginCampaignClaim(INITIAL_CLAIM_STATE, () => "key-3");
    const soldOut = rejectCampaignClaim(
      claiming,
      "CAMPAIGN_SOLD_OUT",
      "sold out",
    );
    expect(soldOut).toMatchObject({
      status: "sold_out",
      optimistic_claimed: false,
      idempotency_key: "key-3",
    });
    expect(soldOut.message).toContain("CAMPAIGN_SOLD_OUT");
  });

  it("연타해도 동일 멱등키를 유지한다", () => {
    const claiming = beginCampaignClaim(INITIAL_CLAIM_STATE, () => "key-4");
    expect(beginCampaignClaim(claiming, () => "new-key")).toBe(claiming);
  });
});
