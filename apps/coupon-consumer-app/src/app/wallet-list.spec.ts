import type { WalletCouponDto } from "@coupon/contracts";
import { describe, expect, it } from "vitest";
import {
  filterAndSortWalletCoupons,
  walletTerminalDescription,
} from "./wallet-list";

const base: WalletCouponDto = {
  id: "coupon-a",
  inquiry_reference: "INQ-A",
  store_id: "store-a",
  store_name: "나무 커피",
  campaign_name: "할인",
  benefit_type: "FIXED",
  benefit_label: "1,000원 할인",
  status: "AVAILABLE",
  minimum_order_amount: { amount: 10_000, currency: "KRW" },
  item_restriction_summary: null,
  conditions: [],
  issued_reason: "선착순",
  issued_at: "2026-08-01T00:00:00Z",
  usable_from: "2026-08-01T00:00:00Z",
  expires_at: "2026-08-15T00:00:00Z",
  used_at: null,
  expired_at: null,
  revoked_at: null,
  terminal_reason: null,
  version: 1,
  updated_at: "2026-08-01T00:00:00Z",
};

describe("wallet list filters and expiry display", () => {
  it("sorts by soonest expiry by default", () => {
    const later = { ...base, id: "later", expires_at: "2026-08-20T00:00:00Z" };
    const sooner = {
      ...base,
      id: "sooner",
      expires_at: "2026-08-12T00:00:00Z",
    };
    expect(
      filterAndSortWalletCoupons(
        [later, sooner],
        {
          store: "",
          benefit_type: "",
          expires_within_7_days: false,
          sort: "expiry",
        },
        new Date("2026-08-10T00:00:00Z"),
      ).map((coupon) => coupon.id),
    ).toEqual(["sooner", "later"]);
  });

  it("combines store, benefit, and 7-day filters", () => {
    const matching = { ...base, expires_at: "2026-08-16T00:00:00Z" };
    const other = {
      ...base,
      id: "other",
      store_name: "다른 상점",
      benefit_type: "PERCENTAGE" as const,
    };
    expect(
      filterAndSortWalletCoupons(
        [other, matching],
        {
          store: "나무 커피",
          benefit_type: "FIXED",
          expires_within_7_days: true,
          sort: "expiry",
        },
        new Date("2026-08-10T00:00:00Z"),
      ),
    ).toEqual([matching]);
  });

  it("never hides expiry and revocation reasons", () => {
    expect(walletTerminalDescription({ ...base, status: "EXPIRED" })).toMatch(
      /만료/,
    );
    expect(
      walletTerminalDescription({
        ...base,
        status: "REVOKED",
        terminal_reason: "안전 이슈로 운영 회수",
      }),
    ).toBe("안전 이슈로 운영 회수");
  });
});
