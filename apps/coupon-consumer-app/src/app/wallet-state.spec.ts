import { describe, expect, it } from "vitest";
import {
  EMPTY_WALLET_SNAPSHOT,
  initialWalletState,
  reduceWalletState,
  type WalletSnapshot,
} from "./wallet-state";

const cached: WalletSnapshot = {
  ...EMPTY_WALLET_SNAPSHOT,
  coupons: [{ id: "coupon-1" } as WalletSnapshot["coupons"][number]],
  synced_at: "2026-08-10T06:00:00Z",
  version: 3,
  updated_at: "2026-08-10T06:00:00Z",
};

describe("wallet view states", () => {
  it("starts loading and resolves an empty response as empty", () => {
    expect(initialWalletState().status).toBe("loading");
    expect(
      reduceWalletState(initialWalletState(), {
        type: "SUCCESS",
        snapshot: EMPTY_WALLET_SNAPSHOT,
      }).status,
    ).toBe("empty");
  });

  it("marks cached data stale after an online failure", () => {
    const state = reduceWalletState(initialWalletState(), {
      type: "FAILURE",
      message: "failed",
      cached,
      online: true,
    });
    expect(state).toMatchObject({
      status: "stale",
      read_only: true,
      version: 3,
    });
  });

  it("shows a blocking error when no cache exists", () => {
    expect(
      reduceWalletState(initialWalletState(), {
        type: "FAILURE",
        message: "불러오기 실패",
        cached: null,
        online: true,
      }),
    ).toMatchObject({
      status: "error",
      message: "불러오기 실패",
      read_only: true,
    });
  });

  it("distinguishes offline with and without a cached snapshot", () => {
    expect(
      reduceWalletState(initialWalletState(), { type: "OFFLINE", cached })
        .status,
    ).toBe("offline");
    const withoutCache = reduceWalletState(initialWalletState(), {
      type: "OFFLINE",
      cached: null,
    });
    expect(withoutCache).toMatchObject({
      status: "offline",
      coupons: [],
      read_only: true,
    });
  });
});
