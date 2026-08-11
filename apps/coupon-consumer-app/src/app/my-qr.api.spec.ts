import "@angular/compiler";
import { HttpClient } from "@angular/common/http";
import { Injector, runInInjectionContext } from "@angular/core";
import { of } from "rxjs";
import { describe, expect, it, vi } from "vitest";
import { MyQrApi } from "./my-qr.api";

describe("MyQrApi multi-device contract", () => {
  it("creates an independent token per tab without a client-side revoke call", () => {
    const post = vi
      .fn()
      .mockReturnValueOnce(
        of({
          data: tokenTransport("token-tab-a", "12345678"),
          request_id: "request-a",
        }),
      )
      .mockReturnValueOnce(
        of({
          data: tokenTransport("token-tab-b", "87654321"),
          request_id: "request-b",
        }),
      );
    const injector = Injector.create({
      providers: [{ provide: HttpClient, useValue: { post } }],
    });
    const api = runInInjectionContext(injector, () => new MyQrApi());
    const tokens: string[] = [];

    api.create().subscribe((result) => tokens.push(result.qr_token));
    api.create().subscribe((result) => tokens.push(result.qr_token));

    expect(tokens).toEqual(["token-tab-a", "token-tab-b"]);
    expect(post).toHaveBeenCalledTimes(2);
    for (const [url, body] of post.mock.calls) {
      expect(url).toBe("/api/coupon/v1/me/qr-tokens");
      expect(body).toEqual({});
    }
  });
});

function tokenTransport(token: string, fallbackCode: string) {
  return {
    token,
    fallback_code: fallbackCode,
    issued_at: "2026-08-10T06:00:00Z",
    expires_at: "2026-08-10T06:01:00Z",
    expires_in_seconds: 60,
    refresh_after_seconds: 30,
    key_id: "key-1",
  };
}
