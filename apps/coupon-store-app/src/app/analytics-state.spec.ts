import type {
  OwnerAnalyticsDto,
  OwnerAnalyticsMetricDto,
} from "@coupon/contracts";
import { describe, expect, it } from "vitest";
import { canShowAnalyticsDetail, metricDisplay } from "./analytics-state";

describe("owner analytics disclosure", () => {
  it("shows pending aggregation instead of a misleading zero", () => {
    const pending: OwnerAnalyticsMetricDto = {
      key: "EARNED",
      label: "적립",
      value: null,
      aggregation_status: "PENDING",
    };
    const readyZero = {
      ...pending,
      value: 0,
      aggregation_status: "READY" as const,
    };
    expect(metricDisplay(pending)).toBe("집계 중");
    expect(metricDisplay(readyZero)).toBe("0");
  });

  it("hides detailed segments below the privacy minimum", () => {
    const analytics: Pick<
      OwnerAnalyticsDto,
      "detail_suppressed" | "minimum_group_size" | "observed_group_size"
    > = {
      detail_suppressed: true,
      minimum_group_size: 10,
      observed_group_size: 7,
    };
    expect(canShowAnalyticsDetail(analytics)).toBe(false);
    expect(
      canShowAnalyticsDetail({
        ...analytics,
        detail_suppressed: false,
        observed_group_size: 10,
      }),
    ).toBe(true);
  });
});
