import { HttpClient } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import type { QrTokenResponseDto } from '@coupon/contracts';
import { Observable } from 'rxjs';

@Injectable({ providedIn: 'root' })
export class MyQrApi {
  private readonly http = inject(HttpClient);

  create(): Observable<QrTokenResponseDto> {
    return this.http.post<QrTokenResponseDto>('/api/coupon/v1/me/qr-tokens', {
      audience: 'STORE_TRANSACTION',
    });
  }
}
