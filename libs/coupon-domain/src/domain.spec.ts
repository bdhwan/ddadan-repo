import { describe, expect, it } from 'vitest';
import { previewDiscount } from './discount';
import { formatExpiryDday, formatKoreaDateTime, formatStampBoard, formatWon } from './formatters';
import { isWithinPeriod } from './period';

describe('coupon domain formatters', () => {
  it('formats integer KRW, Seoul time, D-day and stamp boards', () => {
    expect(formatWon(12_000)).toBe('12,000원');
    expect(formatKoreaDateTime('2026-08-10T06:00:00Z')).toBe('2026. 8. 10. 오후 3:00');
    expect(formatExpiryDday('2026-08-11T14:59:59Z', '2026-08-10T06:00:00Z')).toBe('D-1');
    expect(formatStampBoard(7)).toBe('7/10');
  });
});

describe('discount previews', () => {
  it('caps fixed discounts at the order amount', () => {
    expect(previewDiscount(8_000, { type: 'FIXED', discount_amount: 10_000 }).discount_amount).toBe(8_000);
  });

  it('floors fractional won before applying the maximum percentage discount', () => {
    expect(
      previewDiscount(10_001, {
        type: 'PERCENTAGE',
        percentage: 15,
        maximum_discount_amount: 5_000,
      }).discount_amount,
    ).toBe(1_500);
    expect(
      previewDiscount(100_000, {
        type: 'PERCENTAGE',
        percentage: 15,
        maximum_discount_amount: 5_000,
      }).discount_amount,
    ).toBe(5_000);
  });

  it('makes one lowest-priced eligible item free', () => {
    const preview = previewDiscount(
      13_000,
      { type: 'FREE_ITEM', eligible_item_ids: ['americano', 'latte'] },
      [
        { item_id: 'latte', unit_price: 5_000, quantity: 1 },
        { item_id: 'americano', unit_price: 4_000, quantity: 2 },
      ],
    );
    expect(preview).toMatchObject({ discount_amount: 4_000, free_item_id: 'americano' });
  });
});

describe('half-open periods', () => {
  const period = {
    start: '2026-08-10T00:00:00Z',
    end: '2026-08-11T00:00:00Z',
  };

  it('includes the exact start', () => {
    expect(isWithinPeriod(period.start, period)).toBe(true);
  });

  it('excludes the exact end', () => {
    expect(isWithinPeriod(period.end, period)).toBe(false);
  });

  it('includes the last millisecond before end', () => {
    expect(isWithinPeriod('2026-08-10T23:59:59.999Z', period)).toBe(true);
  });
});
