import { HttpClient, HttpHeaders, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  AdminCampaignListResponseDto,
  AdminEmergencyCampaignRequestDto,
  AdminJobDto,
  AdminJobListResponseDto,
  RetryAdminJobRequestDto,
} from "@coupon/contracts";
import { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class AdminOperationsApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/admin";

  campaigns(cursor?: string): Observable<AdminCampaignListResponseDto> {
    const params = cursor ? new HttpParams().set("cursor", cursor) : undefined;
    return this.http.get<AdminCampaignListResponseDto>(
      `${this.base}/campaigns`,
      { params },
    );
  }

  emergencyCampaignAction(
    campaignId: string,
    payload: AdminEmergencyCampaignRequestDto,
    idempotencyKey: string,
  ): Observable<{ request_id: string; transaction_id: string }> {
    const endpoint =
      payload.action === "EMERGENCY_STOP" ? "emergency-stop" : "revoke-job";
    return this.http.post<{ request_id: string; transaction_id: string }>(
      `${this.base}/campaigns/${campaignId}/${endpoint}`,
      payload,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }

  jobs(cursor?: string): Observable<AdminJobListResponseDto> {
    const params = cursor ? new HttpParams().set("cursor", cursor) : undefined;
    return this.http.get<AdminJobListResponseDto>(`${this.base}/jobs`, {
      params,
    });
  }

  retryJob(
    jobId: string,
    payload: RetryAdminJobRequestDto,
    idempotencyKey: string,
  ): Observable<AdminJobDto> {
    return this.http.post<AdminJobDto>(
      `${this.base}/jobs/${jobId}/retry`,
      payload,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }
}
