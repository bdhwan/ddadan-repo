import { HttpClient, HttpHeaders, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  CampaignImpactActionRequestDto,
  OwnerCampaignDto,
  OwnerCampaignListResponseDto,
  SaveCampaignRequestDto,
} from "@coupon/contracts";
import { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class CampaignsApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/owner/campaigns";

  list(
    version?: number,
    updatedAt?: string,
  ): Observable<OwnerCampaignListResponseDto> {
    let params = new HttpParams();
    if (version !== undefined) params = params.set("version", version);
    if (updatedAt) params = params.set("updated_at", updatedAt);
    return this.http.get<OwnerCampaignListResponseDto>(this.base, { params });
  }

  create(
    payload: SaveCampaignRequestDto,
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    return this.http.post<OwnerCampaignDto>(this.base, payload, {
      headers: idempotencyHeaders(idempotencyKey),
    });
  }

  publish(
    id: string,
    payload: { confirmation_phrase: string; reauthentication_token: string },
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    return this.http.post<OwnerCampaignDto>(
      `${this.base}/${id}/publish`,
      payload,
      { headers: idempotencyHeaders(idempotencyKey) },
    );
  }

  action(
    id: string,
    action: "pause" | "resume" | "cancel",
    payload: CampaignImpactActionRequestDto,
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    return this.http.post<OwnerCampaignDto>(
      `${this.base}/${id}/${action}`,
      payload,
      { headers: idempotencyHeaders(idempotencyKey) },
    );
  }

  updateQuantity(
    campaign: Pick<OwnerCampaignDto, "id" | "version">,
    totalQuantity: number | null,
    perUserQuantity: number,
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    return this.http.patch<OwnerCampaignDto>(
      `${this.base}/${campaign.id}`,
      {
        total_quantity: totalQuantity,
        per_user_quantity: perUserQuantity,
        version: campaign.version,
      },
      { headers: idempotencyHeaders(idempotencyKey) },
    );
  }
}

function idempotencyHeaders(key: string): HttpHeaders {
  return new HttpHeaders({ "Idempotency-Key": key });
}
