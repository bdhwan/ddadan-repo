import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'coupon-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<ng-content />`,
  styles: `
    :host {
      display: block;
      padding: var(--coupon-space-lg);
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
      box-shadow: var(--coupon-shadow);
    }
  `,
})
export class CouponCardComponent {}
