import { HttpClient } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  LoyaltyPolicyDto,
  LoyaltyPolicyListResponseDto,
  PublishLoyaltyPolicyRequestDto,
  SaveLoyaltyPolicyRequestDto,
} from "@coupon/contracts";
import { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class LoyaltyApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/owner/loyalty-policies";
  list(): Observable<LoyaltyPolicyListResponseDto> {
    return this.http.get<LoyaltyPolicyListResponseDto>(this.base);
  }
  create(payload: SaveLoyaltyPolicyRequestDto): Observable<LoyaltyPolicyDto> {
    return this.http.post<LoyaltyPolicyDto>(this.base, payload);
  }
  update(
    id: string,
    payload: SaveLoyaltyPolicyRequestDto,
  ): Observable<LoyaltyPolicyDto> {
    return this.http.patch<LoyaltyPolicyDto>(`${this.base}/${id}`, payload);
  }
  publish(
    id: string,
    payload: PublishLoyaltyPolicyRequestDto,
  ): Observable<LoyaltyPolicyDto> {
    return this.http.post<LoyaltyPolicyDto>(
      `${this.base}/${id}/publish`,
      payload,
    );
  }
}
