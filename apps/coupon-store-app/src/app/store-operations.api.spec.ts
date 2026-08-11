import "@angular/compiler";
import { HttpClient } from "@angular/common/http";
import { Injector, runInInjectionContext } from "@angular/core";
import type { RedemptionResponseDto } from "@coupon/contracts";
import { of } from "rxjs";
import { describe, expect, it, vi } from "vitest";
import { StoreOperationsApi } from "./store-operations.api";

describe("StoreOperationsApi redemption HTTP contract", () => {
  it("reuses the explicit idempotency key while confirming an uncertain result", () => {
    const response: RedemptionResponseDto = {
      transaction_id: "tx-1",
      redemption_id: "redemption-1",
      coupon_id: "coupon-1",
      discount_amount: 1_000,
      payable_amount: 9_000,
      currency: "KRW",
      status: "USED",
      processed_at: "2026-08-10T06:01:00Z",
      cancellable_until: "2026-08-10T06:11:00Z",
      request_id: "req-1",
    };
    const post = vi.fn().mockReturnValue(of(response));
    const injector = Injector.create({
      providers: [{ provide: HttpClient, useValue: { post } }],
    });
    const api = runInInjectionContext(injector, () => new StoreOperationsApi());
    const payload = {
      order: {
        external_order_ref: "POS-1",
        gross_amount: 10_000,
        currency: "KRW" as const,
        items: [],
      },
    };

    api.confirmRedemption("redemption-1", payload, "stable-key").subscribe();
    api.confirmRedemption("redemption-1", payload, "stable-key").subscribe();

    expect(post).toHaveBeenCalledTimes(2);
    for (const [url, body, options] of post.mock.calls) {
      expect(url).toBe("/api/coupon/v1/owner/redemptions/redemption-1/confirm");
      expect(body).toEqual(payload);
      expect(options.headers.get("Idempotency-Key")).toBe("stable-key");
    }
  });
});
