import type { RedemptionPreviewResponseDto } from "@coupon/contracts";

export interface RedemptionReservationView {
  remaining_seconds: number;
  expired: boolean;
  message: string;
}

export function redemptionReservationView(
  preview: RedemptionPreviewResponseDto,
  now: Date | string,
): RedemptionReservationView {
  const remainingSeconds = Math.max(
    0,
    Math.ceil(
      (Date.parse(preview.reservation_expires_at) - timestamp(now)) / 1_000,
    ),
  );
  return {
    remaining_seconds: remainingSeconds,
    expired: remainingSeconds === 0,
    message:
      remainingSeconds === 0
        ? "2분 예약이 만료되었습니다. 최신 조건으로 다시 예약하세요."
        : `승인 예약 ${formatCountdown(remainingSeconds)} 남음`,
  };
}

export function formatCountdown(seconds: number): string {
  const safeSeconds = Math.max(0, Math.floor(seconds));
  const minutes = Math.floor(safeSeconds / 60);
  return `${minutes}:${String(safeSeconds % 60).padStart(2, "0")}`;
}

export function redemptionConditionMessage(
  code: string,
  fieldErrors: ReadonlyArray<{ field: string; message: string }>,
  fallback: string,
): string {
  const details = fieldErrors.map((error) => error.message).filter(Boolean);
  if (details.length) return details.join(" ");
  return (
    {
      MINIMUM_ORDER_NOT_MET: "주문 금액이 쿠폰의 최소 주문액에 부족합니다.",
      COUPON_ITEM_MISMATCH:
        "주문 품목이 쿠폰의 대상 품목 조건과 일치하지 않습니다.",
      COUPON_OUTSIDE_USABLE_PERIOD:
        "현재 시각이 쿠폰 사용 기간 [시작, 종료) 밖입니다. 사용 종료 시각은 포함되지 않습니다.",
      REDEMPTION_RESERVATION_EXPIRED:
        "2분 예약이 만료되었습니다. 다시 예약해 주세요.",
    }[code] ?? fallback
  );
}

function timestamp(value: Date | string): number {
  const result = value instanceof Date ? value.getTime() : Date.parse(value);
  if (Number.isNaN(result)) throw new RangeError("now must be a valid date");
  return result;
}
