import { HttpClient, HttpHeaders, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  AdminCampaignListResponseDto,
  AdminEmergencyCampaignRequestDto,
  AdminJobDto,
  AdminJobListResponseDto,
  RetryAdminJobRequestDto,
} from "@coupon/contracts";
import { map, Observable } from "rxjs";

interface JobSummaryTransport {
  id: string;
  unique_key: string;
  job_type: string;
  status:
    | "PENDING_OUTBOX"
    | "QUEUED"
    | "RUNNING"
    | "RETRY_WAIT"
    | "PAUSE_REQUESTED"
    | "PAUSED"
    | "SUCCEEDED"
    | "DEAD_LETTER"
    | "CANCELLED";
  generation: number;
  attempt_count: number;
  max_attempts: number;
  processed_count: number;
  succeeded_count: number;
  failed_count: number;
  last_error_message: string | null;
  scheduled_at: string;
  started_at: string | null;
  heartbeat_at: string | null;
  finished_at: string | null;
  created_at: string;
}

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
  ): Observable<ApiSuccessDto<unknown>> {
    const endpoint =
      payload.action === "EMERGENCY_STOP" ? "emergency-stop" : "revoke-job";
    const body =
      payload.action === "EMERGENCY_STOP"
        ? { reason: payload.reason }
        : { reason: payload.reason, case_id: payload.case_id ?? null };
    return this.http.post<ApiSuccessDto<unknown>>(
      `${this.base}/campaigns/${campaignId}/${endpoint}`,
      body,
      { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) },
    );
  }

  jobs(cursor?: string): Observable<AdminJobListResponseDto> {
    const params = cursor ? new HttpParams().set("cursor", cursor) : undefined;
    return this.http
      .get<ApiSuccessDto<JobSummaryTransport[]>>(`${this.base}/jobs`, {
        params,
      })
      .pipe(
        map((response) => {
          const items = response.data.map(adaptJob);
          return {
            items,
            next_cursor: null,
            request_id: response.request_id,
            version: Math.max(0, ...items.map((item) => item.version)),
            updated_at: items[0]?.updated_at ?? "1970-01-01T00:00:00.000Z",
          };
        }),
      );
  }

  retryJob(
    jobId: string,
    payload: RetryAdminJobRequestDto,
    idempotencyKey: string,
  ): Observable<void> {
    return this.http
      .post<
        ApiSuccessDto<unknown>
      >(`${this.base}/jobs/${jobId}/retry`, payload, { headers: new HttpHeaders({ "Idempotency-Key": idempotencyKey }) })
      .pipe(map(() => undefined));
  }
}

function adaptJob(job: JobSummaryTransport): AdminJobDto {
  const status: AdminJobDto["status"] =
    job.status === "DEAD_LETTER" || job.status === "CANCELLED"
      ? "FAILED"
      : job.status === "RETRY_WAIT" ||
          job.status === "PAUSE_REQUESTED" ||
          job.status === "PAUSED"
        ? "RETRYING"
        : job.status === "PENDING_OUTBOX"
          ? "QUEUED"
          : job.status;

  return {
    id: job.id,
    job_key: job.unique_key,
    job_type: job.job_type,
    status,
    attempts: job.attempt_count,
    max_attempts: job.max_attempts,
    checkpoint: `${job.processed_count}건 처리 · 성공 ${job.succeeded_count} · 실패 ${job.failed_count}`,
    last_error: job.last_error_message,
    retryable: job.status === "DEAD_LETTER",
    version: job.generation,
    updated_at:
      job.finished_at ??
      job.heartbeat_at ??
      job.started_at ??
      job.scheduled_at ??
      job.created_at,
  };
}
