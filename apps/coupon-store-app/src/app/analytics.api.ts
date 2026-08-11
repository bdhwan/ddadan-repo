import { HttpClient, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type { ApiSuccessDto, OwnerAnalyticsDto } from "@coupon/contracts";
import { map } from "rxjs";
import type { Observable } from "rxjs";

interface DailyCountsTransport {
  stamp_earned_count: number;
  stamp_voided_count: number;
  stamp_net_count: number;
  stamp_transaction_count: number;
  active_customer_count: number | null;
  reward_issued_count: number;
  reward_used_count: number;
  campaign_coupon_issued_count: number;
  campaign_coupon_used_count: number;
  campaign_coupon_revoked_count: number;
  coupon_expired_count: number;
  redemption_voided_count: number;
  discount_amount_total: number;
}

interface OwnerAnalyticsTransport {
  store_id: string;
  from: string;
  to: string;
  finalised_days: number;
  pending_days: number;
  minimum_cohort_size: number;
  totals: DailyCountsTransport;
  days: Array<{
    business_day: string;
    state: "PENDING" | "PROVISIONAL" | "FINAL";
    metrics: DailyCountsTransport | null;
    computed_through: string | null;
    suppressed: boolean;
  }>;
}

@Injectable({ providedIn: "root" })
export class AnalyticsApi {
  private readonly http = inject(HttpClient);

  load(from: string, to: string): Observable<OwnerAnalyticsDto> {
    const params = new HttpParams().set("from", from).set("to", to);
    return this.http
      .get<
        ApiSuccessDto<OwnerAnalyticsTransport>
      >("/api/coupon/v1/owner/analytics", { params })
      .pipe(map((response) => adaptAnalytics(response.data)));
  }
}

function adaptAnalytics(data: OwnerAnalyticsTransport): OwnerAnalyticsDto {
  const computedAt = data.days
    .map((day) => day.computed_through)
    .filter((value): value is string => value !== null)
    .sort()
    .at(-1);
  const confirmedThrough = data.days
    .filter((day) => day.state === "FINAL")
    .map((day) => day.business_day)
    .sort()
    .at(-1);
  const status = data.pending_days > 0 ? "PENDING" : "READY";
  const observedGroupSize = data.totals.active_customer_count ?? 0;
  const detailSuppressed =
    data.days.some((day) => day.suppressed) ||
    data.totals.active_customer_count === null;

  return {
    period_from: data.from,
    period_to: data.to,
    provisional_as_of: computedAt ?? new Date().toISOString(),
    confirmed_through: confirmedThrough ?? null,
    minimum_group_size: data.minimum_cohort_size,
    observed_group_size: observedGroupSize,
    detail_suppressed: detailSuppressed,
    metrics: [
      metric("EARNED", "적립 도장", data.totals.stamp_earned_count, status),
      metric("REWARDS", "발급 리워드", data.totals.reward_issued_count, status),
      metric(
        "CAMPAIGNS",
        "캠페인 쿠폰",
        data.totals.campaign_coupon_issued_count,
        status,
      ),
      metric(
        "VOIDS",
        "취소",
        data.totals.stamp_voided_count + data.totals.redemption_voided_count,
        status,
      ),
      metric("ADJUSTMENTS", "순 도장", data.totals.stamp_net_count, status),
    ],
    breakdowns: data.days
      .filter((day) => !day.suppressed && day.metrics !== null)
      .map((day) => ({
        label: day.business_day,
        value: day.metrics?.stamp_net_count ?? 0,
      })),
  };
}

function metric(
  key: OwnerAnalyticsDto["metrics"][number]["key"],
  label: string,
  value: number,
  aggregationStatus: "READY" | "PENDING",
): OwnerAnalyticsDto["metrics"][number] {
  return {
    key,
    label,
    value: aggregationStatus === "PENDING" ? null : value,
    aggregation_status: aggregationStatus,
  };
}
