import type { CouponBenefitType, WalletCouponDto } from "@coupon/contracts";

export type WalletSort = "expiry" | "recent" | "store";

export interface WalletFilters {
  store: string;
  benefit_type: CouponBenefitType | "";
  expires_within_7_days: boolean;
  sort: WalletSort;
}

export function filterAndSortWalletCoupons(
  coupons: readonly WalletCouponDto[],
  filters: WalletFilters,
  now = new Date(),
): WalletCouponDto[] {
  const reference = now.getTime();
  return coupons
    .filter((coupon) => !filters.store || coupon.store_name === filters.store)
    .filter(
      (coupon) =>
        !filters.benefit_type || coupon.benefit_type === filters.benefit_type,
    )
    .filter((coupon) => {
      if (!filters.expires_within_7_days) return true;
      const remaining = Date.parse(coupon.expires_at) - reference;
      return remaining >= 0 && remaining <= 7 * 86_400_000;
    })
    .sort((left, right) => compareWalletCoupons(left, right, filters.sort));
}

export function compareWalletCoupons(
  left: WalletCouponDto,
  right: WalletCouponDto,
  sort: WalletSort,
): number {
  if (sort === "recent") {
    return Date.parse(right.issued_at) - Date.parse(left.issued_at);
  }
  if (sort === "store") {
    return left.store_name.localeCompare(right.store_name, "ko");
  }
  return Date.parse(left.expires_at) - Date.parse(right.expires_at);
}

export function walletTerminalDescription(
  coupon: WalletCouponDto,
): string | null {
  if (coupon.status === "EXPIRED") {
    return coupon.terminal_reason ?? "사용 기간이 종료되어 만료되었습니다.";
  }
  if (coupon.status === "REVOKED") {
    return coupon.terminal_reason ?? "캠페인 운영 정책에 따라 회수되었습니다.";
  }
  return coupon.terminal_reason;
}
