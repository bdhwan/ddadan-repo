import "@angular/compiler";
import { HttpClient } from "@angular/common/http";
import { Injector, runInInjectionContext } from "@angular/core";
import type { CampaignClaimResponseDto } from "@coupon/contracts";
import { of } from "rxjs";
import { describe, expect, it, vi } from "vitest";
import { StoreDetailApi } from "./store-detail.api";

describe("StoreDetailApi HTTP contract", () => {
  it("sends claim with the caller-owned idempotency key", () => {
    const expected: CampaignClaimResponseDto = {
      coupon_id: "coupon-1",
      outcome: "ISSUED",
      status: "AVAILABLE",
      request_id: "req-1",
      transaction_id: "tx-1",
    };
    const post = vi.fn().mockReturnValue(of(expected));
    const injector = Injector.create({
      providers: [{ provide: HttpClient, useValue: { post } }],
    });
    const api = runInInjectionContext(injector, () => new StoreDetailApi());
    let actual: CampaignClaimResponseDto | undefined;

    api.claim("campaign-1", "claim-key-1").subscribe((result) => {
      actual = result;
    });

    expect(post).toHaveBeenCalledOnce();
    const [url, body, options] = post.mock.calls[0];
    expect(url).toBe("/api/coupon/v1/campaigns/campaign-1/claims");
    expect(body).toEqual({});
    expect(options.headers.get("Idempotency-Key")).toBe("claim-key-1");
    expect(actual).toEqual(expected);
  });
});
