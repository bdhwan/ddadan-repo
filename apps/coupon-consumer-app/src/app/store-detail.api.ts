import { HttpClient, HttpHeaders } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type { CampaignClaimResponseDto } from "@coupon/contracts";
import { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class StoreDetailApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1";

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
