export interface OrderItem {
  item_id: string;
  unit_price: number;
  quantity: number;
}

export type DiscountBenefit =
  | { type: 'FIXED'; discount_amount: number }
  | { type: 'PERCENTAGE'; percentage: number; maximum_discount_amount: number }
  | { type: 'FREE_ITEM'; eligible_item_ids: readonly string[] };

export interface DiscountPreview {
  discount_amount: number;
  payable_amount: number;
  currency: 'KRW';
  free_item_id?: string;
}

export function previewDiscount(
  targetAmount: number,
  benefit: DiscountBenefit,
  items: readonly OrderItem[] = [],
): DiscountPreview {
  assertWon(targetAmount, 'targetAmount');
  let discountAmount: number;
  let freeItemId: string | undefined;

  switch (benefit.type) {
    case 'FIXED':
      assertWon(benefit.discount_amount, 'discount_amount');
      discountAmount = Math.min(targetAmount, benefit.discount_amount);
      break;
    case 'PERCENTAGE':
      if (!Number.isInteger(benefit.percentage) || benefit.percentage < 1 || benefit.percentage > 100) {
        throw new RangeError('percentage must be an integer from 1 to 100');
      }
      assertWon(benefit.maximum_discount_amount, 'maximum_discount_amount');
      discountAmount = Math.min(
        Math.floor((targetAmount * benefit.percentage) / 100),
        benefit.maximum_discount_amount,
      );
      break;
    case 'FREE_ITEM': {
      const eligible = new Set(benefit.eligible_item_ids);
      const cheapest = items
        .filter((item) => {
          assertWon(item.unit_price, 'unit_price');
          if (!Number.isSafeInteger(item.quantity) || item.quantity < 0) {
            throw new RangeError('quantity must be a non-negative safe integer');
          }
          return item.quantity > 0 && eligible.has(item.item_id);
        })
        .sort((left, right) => left.unit_price - right.unit_price)[0];
      discountAmount = Math.min(targetAmount, cheapest?.unit_price ?? 0);
      freeItemId = cheapest?.item_id;
      break;
    }
    default:
      return assertNever(benefit);
  }

  return {
    discount_amount: discountAmount,
    payable_amount: targetAmount - discountAmount,
    currency: 'KRW',
    ...(freeItemId ? { free_item_id: freeItemId } : {}),
  };
}

function assertWon(value: number, name: string): void {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new RangeError(`${name} must be a non-negative safe integer in KRW`);
  }
}

function assertNever(value: never): never {
  throw new Error(`Unsupported benefit: ${JSON.stringify(value)}`);
}
