import { HttpClient } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type { QrTokenResponseDto } from "@coupon/contracts";
import type { ApiSuccessDto } from "@coupon/contracts";
import { map, Observable } from "rxjs";

interface QrTokenTransport {
  token: string;
  fallback_code: string;
  issued_at: string;
  expires_at: string;
  expires_in_seconds: number;
  refresh_after_seconds: number;
  key_id: string;
}

@Injectable({ providedIn: "root" })
export class MyQrApi {
  private readonly http = inject(HttpClient);

  create(): Observable<QrTokenResponseDto> {
    return this.http
      .post<ApiSuccessDto<QrTokenTransport>>("/api/coupon/v1/me/qr-tokens", {})
      .pipe(
        map((response) => ({
          qr_token: response.data.token,
          qr_payload: response.data.token,
          auxiliary_code: response.data.fallback_code,
          issued_at: response.data.issued_at,
          expires_at: response.data.expires_at,
          refresh_after_seconds: response.data.refresh_after_seconds,
          request_id: response.request_id,
        })),
      );
  }
}
