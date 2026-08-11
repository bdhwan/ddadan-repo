import { ChangeDetectionStrategy, Component, input } from "@angular/core";

@Component({
  selector: "coupon-empty-state",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: "status" },
  template: `
    <span class="icon" aria-hidden="true">◇</span>
    <h2>{{ title() }}</h2>
    <p>{{ description() }}</p>
    <ng-content />
  `,
  styles: `
    :host {
      display: grid;
      justify-items: center;
      gap: 0.5rem;
      padding: 2rem 1rem;
      text-align: center;
      border: 1px dashed var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
    }
    .icon {
      font-size: 2rem;
      color: var(--coupon-color-primary);
    }
    h2,
    p {
      margin: 0;
    }
    h2 {
      font-size: var(--coupon-font-size-lg);
    }
    p {
      max-width: 34rem;
      color: var(--coupon-color-text-muted);
    }
  `,
})
export class CouponEmptyStateComponent {
  readonly title = input.required<string>();
  readonly description = input.required<string>();
}
