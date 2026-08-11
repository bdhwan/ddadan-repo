import { HttpClient, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  WalletCouponListResponseDto,
  WalletStampListResponseDto,
} from "@coupon/contracts";
import type { VersionCursor } from "@coupon/client-core";
import { Observable, forkJoin } from "rxjs";

@Injectable({ providedIn: "root" })
export class WalletApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/me/wallet";

  load(cursor: VersionCursor | null): Observable<{
    available: WalletCouponListResponseDto;
    history: WalletCouponListResponseDto;
    stamps: WalletStampListResponseDto;
  }> {
    let common = new HttpParams();
    if (cursor?.version !== undefined)
      common = common.set("version", cursor.version);
    if (cursor?.updated_at)
      common = common.set("updated_at", cursor.updated_at);
    return forkJoin({
      available: this.http.get<WalletCouponListResponseDto>(
        `${this.base}/coupons`,
        {
          params: common.set("status", "AVAILABLE,RESERVED"),
        },
      ),
      history: this.http.get<WalletCouponListResponseDto>(
        `${this.base}/coupons`,
        {
          params: common.set("status", "USED,EXPIRED,REVOKED,VOIDED"),
        },
      ),
      stamps: this.http.get<WalletStampListResponseDto>(`${this.base}/stamps`, {
        params: common,
      }),
    });
  }
}
