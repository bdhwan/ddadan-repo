import { HttpClient, HttpHeaders } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  CampaignClaimResponseDto,
  PublicStoreDetailDto,
} from "@coupon/contracts";
import { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class StoreDetailApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1";

  detail(slug: string): Observable<PublicStoreDetailDto> {
    return this.http.get<PublicStoreDetailDto>(
      `${this.base}/public/stores/${encodeURIComponent(slug)}`,
    );
  }

  favorite(
    storeId: string,
    favorite: boolean,
    idempotencyKey: string,
  ): Observable<void> {
    const options = {
      headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }),
    };
    return favorite
      ? this.http.put<void>(
          `${this.base}/me/favorite-stores/${storeId}`,
          {},
          options,
        )
      : this.http.delete<void>(
          `${this.base}/me/favorite-stores/${storeId}`,
          options,
        );
  }

  claim(
    campaignId: string,
    idempotencyKey: string,
  ): Observable<CampaignClaimResponseDto> {
    return this.http.post<CampaignClaimResponseDto>(
      `${this.base}/campaigns/${campaignId}/claims`,
      {},
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }
}
