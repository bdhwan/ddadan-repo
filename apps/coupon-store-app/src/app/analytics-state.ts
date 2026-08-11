import type {
  OwnerAnalyticsDto,
  OwnerAnalyticsMetricDto,
} from "@coupon/contracts";

export function metricDisplay(metric: OwnerAnalyticsMetricDto): string {
  return metric.aggregation_status === "PENDING" || metric.value === null
    ? "집계 중"
    : metric.value.toLocaleString("ko-KR");
}

export function canShowAnalyticsDetail(
  analytics: Pick<
    OwnerAnalyticsDto,
    "detail_suppressed" | "minimum_group_size" | "observed_group_size"
  >,
): boolean {
  return (
    !analytics.detail_suppressed &&
    analytics.observed_group_size >= analytics.minimum_group_size
  );
}
