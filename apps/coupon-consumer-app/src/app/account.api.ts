import { HttpClient, HttpHeaders } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  ConsentsDataDto,
  UpdateConsentsRequestDto,
} from "@coupon/contracts";
import { mapApiData } from "@coupon/client-core";
import type { Observable } from "rxjs";

@Injectable({ providedIn: "root" })
export class AccountApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/me";

  consents(): Observable<ConsentsDataDto> {
    return this.http
      .get<ApiSuccessDto<ConsentsDataDto>>(`${this.base}/consents`)
      .pipe(mapApiData());
  }

  updateConsents(
    payload: UpdateConsentsRequestDto,
  ): Observable<ConsentsDataDto> {
    return this.http
      .post<ApiSuccessDto<ConsentsDataDto>>(`${this.base}/consents`, payload, {
        headers: idempotencyHeaders(),
      })
      .pipe(mapApiData());
  }

  revokeSessions(): Observable<void> {
    return this.http
      .post<ApiSuccessDto<void>>(
        `${this.base}/sessions/revoke`,
        {},
        {
          headers: idempotencyHeaders(),
        },
      )
      .pipe(mapApiData());
  }

  withdraw(): Observable<{ status: string; preserved_records: string[] }> {
    return this.http
      .post<ApiSuccessDto<{ status: string; preserved_records: string[] }>>(
        `${this.base}/withdrawal`,
        { confirmation: "WITHDRAW" },
        {
          headers: idempotencyHeaders(),
        },
      )
      .pipe(mapApiData());
  }
}

function idempotencyHeaders(): HttpHeaders {
  const idempotencyKey =
    typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (character) => {
          const random = Math.floor(Math.random() * 16);
          return (character === "x" ? random : (random & 3) | 8).toString(16);
        });
  return new HttpHeaders({ "Idempotency-Key": idempotencyKey });
}
