export interface HalfOpenPeriod {
  start: Date | string;
  end: Date | string;
}

/** Returns true only for the half-open interval [start, end). */
export function isWithinPeriod(
  value: Date | string,
  period: HalfOpenPeriod,
): boolean {
  const instant = timestamp(value);
  const start = timestamp(period.start);
  const end = timestamp(period.end);
  if (start >= end) {
    throw new RangeError("period end must be later than start");
  }
  return instant >= start && instant < end;
}

function timestamp(value: Date | string): number {
  const result = value instanceof Date ? value.getTime() : Date.parse(value);
  if (Number.isNaN(result)) {
    throw new RangeError(
      "period values must be valid dates or RFC3339 timestamps",
    );
  }
  return result;
}
