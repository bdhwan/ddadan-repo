import { describe, expect, it } from "vitest";
import {
  IMMUTABLE_AFTER_ISSUANCE,
  createCampaignDraft,
  maximumCampaignExposure,
  previewCampaignDiscount,
  validateCampaignStep,
} from "./campaign-wizard";

describe("campaign creation wizard", () => {
  it("validates each discount type before leaving the benefit step", () => {
    const draft = createCampaignDraft(new Date("2026-08-10T00:00:00Z"));
    draft.name = "여름 할인";
    draft.benefit_type = "FIXED";
    draft.fixed_discount_amount = 0;
    expect(validateCampaignStep(draft, "benefit")).toContain(
      "정액 할인액은 1원 이상의 정수여야 합니다.",
    );

    draft.benefit_type = "PERCENTAGE";
    draft.percentage = 101;
    draft.maximum_discount_amount = 0;
    expect(validateCampaignStep(draft, "benefit")).toHaveLength(2);

    draft.benefit_type = "FREE_ITEM";
    draft.free_item_ids = [];
    expect(validateCampaignStep(draft, "benefit")[0]).toMatch(/하나 이상/);
    draft.free_item_ids = ["item-1"];
    expect(validateCampaignStep(draft, "benefit")).toEqual([]);
  });

  it("uses the shared discount calculator and exposes the maximum liability", () => {
    const draft = createCampaignDraft(new Date("2026-08-10T00:00:00Z"));
    draft.name = "15%";
    draft.benefit_type = "PERCENTAGE";
    draft.percentage = 15;
    draft.maximum_discount_amount = 2_000;
    draft.total_quantity = 100;
    expect(previewCampaignDiscount(draft, 9_999)).toBe(1_499);
    expect(maximumCampaignExposure(draft)).toBe(200_000);
  });

  it("makes issued coupon snapshots explicit and immutable", () => {
    expect(IMMUTABLE_AFTER_ISSUANCE).toContain("혜택 유형과 할인 값");
    expect(IMMUTABLE_AFTER_ISSUANCE).toContain(
      "발급 후 계산된 사용 시작·만료 시각",
    );
  });
});
