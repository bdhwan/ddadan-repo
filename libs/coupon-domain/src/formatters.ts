const wonFormatter = new Intl.NumberFormat("ko-KR", {
  maximumFractionDigits: 0,
});

const koreaDateTimeFormatter = new Intl.DateTimeFormat("ko-KR", {
  timeZone: "Asia/Seoul",
  year: "numeric",
  month: "numeric",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
  hour12: true,
});

const koreaDatePartsFormatter = new Intl.DateTimeFormat("en-CA", {
  timeZone: "Asia/Seoul",
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

export function formatWon(amount: number): string {
  assertNonNegativeInteger(amount, "amount");
  return `${wonFormatter.format(amount)}원`;
}

export function formatKoreaDateTime(value: Date | string): string {
  const date = toValidDate(value);
  return koreaDateTimeFormatter.format(date);
}

export function formatStampBoard(current: number, goal = 10): string {
  assertNonNegativeInteger(current, "current");
  if (!Number.isInteger(goal) || goal <= 0) {
    throw new RangeError("goal must be a positive integer");
  }
  return `${Math.min(current, goal)}/${goal}`;
}

export function formatExpiryDday(
  expiresAt: Date | string,
  now: Date | string = new Date(),
): string {
  const expiry = toValidDate(expiresAt);
  const reference = toValidDate(now);
  if (expiry.getTime() <= reference.getTime()) {
    return "만료";
  }

  const expiryDay = koreaCalendarDay(expiry);
  const referenceDay = koreaCalendarDay(reference);
  const days = Math.round((expiryDay - referenceDay) / 86_400_000);
  return days === 0 ? "D-Day" : `D-${days}`;
}

function koreaCalendarDay(value: Date): number {
  const parts = koreaDatePartsFormatter.formatToParts(value);
  const year = Number(parts.find((part) => part.type === "year")?.value);
  const month = Number(parts.find((part) => part.type === "month")?.value);
  const day = Number(parts.find((part) => part.type === "day")?.value);
  return Date.UTC(year, month - 1, day);
}

function toValidDate(value: Date | string): Date {
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) {
    throw new RangeError("value must be a valid date or RFC3339 timestamp");
  }
  return date;
}

function assertNonNegativeInteger(value: number, name: string): void {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${name} must be a non-negative safe integer`);
  }
}
