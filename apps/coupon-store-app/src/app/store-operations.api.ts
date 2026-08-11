import { HttpClient, HttpHeaders, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  CatalogItemListResponseDto,
  CreateStampTransactionRequestDto,
  OwnerDashboardResponseDto,
  ResolvedCustomerDto,
  ScanResolveRequestDto,
  StampPreviewRequestDto,
  StampPreviewResponseDto,
  StampTransactionResponseDto,
  RedemptionPreviewRequestDto,
  RedemptionPreviewResponseDto,
  ConfirmRedemptionRequestDto,
  RedemptionResponseDto,
  CancelRedemptionRequestDto,
} from "@coupon/contracts";
import { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class StoreOperationsApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/owner";

  resolve(payload: ScanResolveRequestDto): Observable<ResolvedCustomerDto> {
    return this.http.post<ResolvedCustomerDto>(
      `${this.base}/scan/resolve`,
      payload,
    );
  }
  preview(
    payload: StampPreviewRequestDto,
  ): Observable<StampPreviewResponseDto> {
    return this.http.post<StampPreviewResponseDto>(
      `${this.base}/stamp-transactions/preview`,
      payload,
    );
  }
  submit(
    payload: CreateStampTransactionRequestDto,
    idempotencyKey: string,
  ): Observable<StampTransactionResponseDto> {
    return this.http.post<StampTransactionResponseDto>(
      `${this.base}/stamp-transactions`,
      payload,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }
  previewRedemption(
    payload: RedemptionPreviewRequestDto,
    idempotencyKey: string,
  ): Observable<RedemptionPreviewResponseDto> {
    return this.http.post<RedemptionPreviewResponseDto>(
      `${this.base}/redemptions/preview`,
      payload,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }
  confirmRedemption(
    redemptionId: string,
    payload: ConfirmRedemptionRequestDto,
    idempotencyKey: string,
  ): Observable<RedemptionResponseDto> {
    return this.http.post<RedemptionResponseDto>(
      `${this.base}/redemptions/${redemptionId}/confirm`,
      payload,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }
  cancelRedemption(
    redemptionId: string,
    payload: CancelRedemptionRequestDto,
    idempotencyKey: string,
  ): Observable<RedemptionResponseDto> {
    return this.http.post<RedemptionResponseDto>(
      `${this.base}/redemptions/${redemptionId}/cancel`,
      payload,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }
  catalog(): Observable<CatalogItemListResponseDto> {
    return this.http.get<CatalogItemListResponseDto>(
      `${this.base}/catalog/items`,
    );
  }
  dashboard(
    version?: number,
    updatedAt?: string,
  ): Observable<OwnerDashboardResponseDto> {
    let params = new HttpParams();
    if (version !== undefined) params = params.set("version", version);
    if (updatedAt) params = params.set("updated_at", updatedAt);
    return this.http.get<OwnerDashboardResponseDto>(`${this.base}/analytics`, {
      params: params.set("scope", "today"),
    });
  }
}
