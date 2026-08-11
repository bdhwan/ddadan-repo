import type { RedemptionPreviewResponseDto } from "@coupon/contracts";
import { describe, expect, it } from "vitest";
import {
  redemptionConditionMessage,
  redemptionReservationView,
} from "./redemption-state";

const reservation: RedemptionPreviewResponseDto = {
  redemption_id: "redemption-1",
  coupon_id: "coupon-1",
  coupon_inquiry_reference: "INQ-1",
  customer_reference_masked: "CUS-•1234",
  display_name_masked: "김•단",
  benefit_label: "20% 할인 (최대 3,000원)",
  expected_discount_amount: 2_400,
  payable_amount: 9_600,
  currency: "KRW",
  conditions: ["최소 주문 10,000원"],
  reserved_at: "2026-08-10T06:00:00Z",
  reservation_expires_at: "2026-08-10T06:02:00Z",
  request_id: "req-1",
};

describe("redemption reservation countdown", () => {
  it("counts down the two-minute reservation and marks the end instant expired", () => {
    expect(
      redemptionReservationView(reservation, "2026-08-10T06:00:00Z"),
    ).toMatchObject({ remaining_seconds: 120, expired: false });
    expect(
      redemptionReservationView(reservation, "2026-08-10T06:01:59Z"),
    ).toMatchObject({ remaining_seconds: 1, expired: false });
    expect(
      redemptionReservationView(reservation, "2026-08-10T06:02:00Z"),
    ).toMatchObject({ remaining_seconds: 0, expired: true });
  });

  it("resets to a fresh countdown after re-reservation", () => {
    const expired = redemptionReservationView(
      reservation,
      "2026-08-10T06:02:01Z",
    );
    const renewed = redemptionReservationView(
      {
        ...reservation,
        redemption_id: "redemption-2",
        reserved_at: "2026-08-10T06:02:01Z",
        reservation_expires_at: "2026-08-10T06:04:01Z",
      },
      "2026-08-10T06:02:01Z",
    );
    expect(expired.expired).toBe(true);
    expect(renewed).toMatchObject({ remaining_seconds: 120, expired: false });
  });

  it("explains business-condition failures instead of only showing a code", () => {
    expect(
      redemptionConditionMessage("MINIMUM_ORDER_NOT_MET", [], "fallback"),
    ).toMatch(/최소 주문액/);
    expect(
      redemptionConditionMessage("COUPON_ITEM_MISMATCH", [], "fallback"),
    ).toMatch(/품목/);
    expect(
      redemptionConditionMessage(
        "COUPON_OUTSIDE_USABLE_PERIOD",
        [],
        "fallback",
      ),
    ).toMatch(/종료 시각/);
  });
});
