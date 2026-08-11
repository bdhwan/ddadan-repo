import { Injectable } from '@angular/core';

export interface RequestTelemetry {
  request_id: string;
  method: string;
  url: string;
  status: number;
}

@Injectable({ providedIn: 'root' })
export class CouponTelemetryService {
  recordRequest(event: RequestTelemetry): void {
    // The Phase 1 sink is intentionally free of payload/PII. A production sink
    // can replace this service while preserving request correlation semantics.
    console.info('[coupon-request]', event);
  }
}
