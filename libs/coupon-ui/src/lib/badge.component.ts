import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

type BadgeStatus = 'success' | 'warning' | 'danger' | 'neutral';

@Component({
  selector: 'coupon-badge',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[class]': 'status()',
    '[attr.aria-label]': 'accessibleLabel()',
  },
  template: `<span aria-hidden="true">{{ icon() }}</span><span><ng-content /></span>`,
  styles: `
    :host { display: inline-flex; align-items: center; gap: .35rem; min-height: 28px; padding: .2rem .65rem; border: 1px solid currentColor; border-radius: 999px; font-weight: 700; font-size: var(--coupon-font-size-sm); }
    :host.success { color: var(--coupon-color-success); }
    :host.warning { color: var(--coupon-color-warning); }
    :host.danger { color: var(--coupon-color-danger); }
    :host.neutral { color: var(--coupon-color-text-muted); }
  `,
})
export class CouponBadgeComponent {
  readonly status = input<BadgeStatus>('neutral');
  readonly label = input.required<string>();
  readonly icon = computed(() => ({ success: '✓', warning: '⚠', danger: '✕', neutral: '•' })[this.status()]);
  readonly accessibleLabel = computed(() => `${this.label()} 상태`);
}
