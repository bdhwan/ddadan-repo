import { describe, expect, it } from "vitest";
import {
  acknowledgeWithdrawalImpact,
  completeWithdrawal,
  completeWithdrawalReauthentication,
  initialWithdrawalFlow,
} from "./withdrawal-flow";

describe("withdrawal reauthentication flow", () => {
  it("cannot complete before impact acknowledgement and reauthentication", () => {
    expect(completeWithdrawal(initialWithdrawalFlow).step).toBe("IMPACT");
    const acknowledged = acknowledgeWithdrawalImpact();
    expect(acknowledged.step).toBe("REAUTHENTICATION");
    expect(completeWithdrawal(acknowledged).step).toBe("REAUTHENTICATION");

    const recent = completeWithdrawalReauthentication(acknowledged);
    expect(recent.step).toBe("SUBMITTING");
    expect(completeWithdrawal(recent).step).toBe("COMPLETE");
  });
});
