import type { ConsentStateDto } from "@coupon/contracts";
import { describe, expect, it } from "vitest";
import {
  optimisticConsent,
  permissionCopy,
} from "./notification-settings-state";

const consent: ConsentStateDto = {
  scope: "TRANSACTIONAL_WEB_PUSH",
  store_id: null,
  granted: true,
  required: false,
  document_version: "push-v1",
  decided_at: "2026-08-11T06:00:00Z",
};

describe("notification settings state", () => {
  it("reflects consent withdrawal immediately", () => {
    const next = optimisticConsent(
      [consent],
      "TRANSACTIONAL_WEB_PUSH",
      null,
      false,
      "2026-08-11T06:01:00Z",
    );
    expect(next[0]?.granted).toBe(false);
    expect(next[0]?.decided_at).toBe("2026-08-11T06:01:00Z");
  });

  it("explains denied browser permission with a recovery action", () => {
    expect(permissionCopy("denied")).toContain("브라우저 사이트 설정");
    expect(permissionCopy("granted")).toContain("허용");
  });
});
