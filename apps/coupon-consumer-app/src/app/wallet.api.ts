import { HttpClient, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  WalletCouponListResponseDto,
  WalletStampListResponseDto,
} from "@coupon/contracts";
import type { VersionCursor } from "@coupon/client-core";
import { Observable, forkJoin, map } from "rxjs";

interface WalletCouponTransport {
  id: string;
  store_id: string;
  store_name: string;
  source_type: string;
  status: WalletCouponListResponseDto["items"][number]["status"];
  effective_status: WalletCouponListResponseDto["items"][number]["status"];
  title: string;
  description: string;
  benefit_type: WalletCouponListResponseDto["items"][number]["benefit_type"];
  usable_from: string;
  expires_at: string;
  created_at: string;
  issued_at: string | null;
  used_at: string | null;
  expired_at: string | null;
  revoked_at: string | null;
  revocation_reason: string | null;
  version: number;
}

interface WalletCouponPageTransport {
  items: WalletCouponTransport[];
  next_cursor: string | null;
  has_more: boolean;
}

interface WalletStampTransport {
  store_id: string;
  store_name: string;
  available: number;
  target: number;
  earliest_expiry: string | null;
  reward_title: string | null;
}

interface WalletStampsTransport {
  boards: WalletStampTransport[];
  total_available: number;
  as_of: string;
}

@Injectable({ providedIn: "root" })
export class WalletApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/me/wallet";

  load(cursor: VersionCursor | null): Observable<{
    available: WalletCouponListResponseDto;
    history: WalletCouponListResponseDto;
    stamps: WalletStampListResponseDto;
  }> {
    const availableParams = new HttpParams().set("status", "AVAILABLE");
    const historyParams = new HttpParams().set("status", "HISTORY");
    return forkJoin({
      available: this.http.get<ApiSuccessDto<WalletCouponPageTransport>>(
        `${this.base}/coupons`,
        { params: availableParams },
      ),
      history: this.http.get<ApiSuccessDto<WalletCouponPageTransport>>(
        `${this.base}/coupons`,
        { params: historyParams },
      ),
      stamps: this.http.get<ApiSuccessDto<WalletStampsTransport>>(
        `${this.base}/stamps`,
      ),
    }).pipe(
      map((response) => ({
        available: adaptCoupons(response.available),
        history: adaptCoupons(response.history),
        stamps: adaptStamps(response.stamps),
      })),
    );
  }
}

function adaptCoupons(
  response: ApiSuccessDto<WalletCouponPageTransport>,
): WalletCouponListResponseDto {
  const items = response.data.items.map((coupon) => ({
    id: coupon.id,
    inquiry_reference: coupon.id.slice(0, 8).toUpperCase(),
    store_id: coupon.store_id,
    store_name: coupon.store_name,
    campaign_name: coupon.title,
    benefit_type: coupon.benefit_type,
    benefit_label: coupon.description || coupon.title,
    status: coupon.effective_status,
    minimum_order_amount: { amount: 0, currency: "KRW" as const },
    item_restriction_summary: null,
    conditions: [],
    issued_reason: coupon.source_type,
    issued_at: coupon.issued_at ?? coupon.created_at,
    usable_from: coupon.usable_from,
    expires_at: coupon.expires_at,
    used_at: coupon.used_at,
    expired_at: coupon.expired_at,
    revoked_at: coupon.revoked_at,
    terminal_reason: coupon.revocation_reason,
    version: coupon.version,
    updated_at: coupon.created_at,
  }));
  return {
    items,
    next_cursor: response.data.next_cursor,
    request_id: response.request_id,
    version: Math.max(0, ...items.map((item) => item.version)),
    updated_at: items[0]?.updated_at ?? new Date(0).toISOString(),
  };
}

function adaptStamps(
  response: ApiSuccessDto<WalletStampsTransport>,
): WalletStampListResponseDto {
  return {
    items: response.data.boards.map((board) => ({
      store_id: board.store_id,
      store_name: board.store_name,
      available_stamps: board.available,
      goal_stamps: board.target,
      earliest_stamp_expires_at: board.earliest_expiry,
      reward_description: board.reward_title ?? "리워드 준비 중",
      policy_status: "ACTIVE",
      version: 1,
      updated_at: response.data.as_of,
    })),
    next_cursor: null,
    request_id: response.request_id,
    version: 1,
    updated_at: response.data.as_of,
  };
}
