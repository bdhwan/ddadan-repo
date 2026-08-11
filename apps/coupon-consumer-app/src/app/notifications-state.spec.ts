import type { NotificationDto } from "@coupon/contracts";
import { describe, expect, it } from "vitest";
import { mergeNotifications, optimisticRead } from "./notifications-state";

const unread: NotificationDto = {
  id: "notification-1",
  event_id: "event-1",
  category: "TRANSACTION",
  type: "STAMP_EARNED",
  title: "도장이 적립됐어요",
  body: "도장 1개가 적립됐습니다.",
  read_at: null,
  created_at: "2026-08-11T06:00:00Z",
  version: 1,
};

describe("notification optimistic read state", () => {
  it("marks immediately and lets a failed request resynchronize server truth", () => {
    const optimistic = optimisticRead(
      [unread],
      unread.id,
      "2026-08-11T06:01:00Z",
    );
    expect(optimistic[0]?.read_at).toBe("2026-08-11T06:01:00Z");

    const resynchronized = mergeNotifications(optimistic, [unread]);
    expect(resynchronized[0]?.read_at).toBeNull();
    expect(resynchronized[0]?.version).toBe(1);
  });
});
