import type { NotificationDto } from "@coupon/contracts";

export function optimisticRead(
  items: readonly NotificationDto[],
  notificationId: string,
  readAt: string,
): NotificationDto[] {
  return items.map((item) =>
    item.id === notificationId && item.read_at === null
      ? { ...item, read_at: readAt }
      : item,
  );
}

export function mergeNotifications(
  current: readonly NotificationDto[],
  incoming: readonly NotificationDto[],
): NotificationDto[] {
  return [
    ...new Map(
      [...current, ...incoming].map((item) => [item.id, item]),
    ).values(),
  ].sort(
    (left, right) => Date.parse(right.created_at) - Date.parse(left.created_at),
  );
}
