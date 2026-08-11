import { ChangeDetectionStrategy, Component, input } from "@angular/core";

@Component({
  selector: "coupon-button",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "[class.full-width]": "fullWidth()" },
  template: `
    <button
      [attr.type]="type()"
      [attr.aria-label]="ariaLabel() || null"
      [class.secondary]="variant() === 'secondary'"
      [class.danger]="variant() === 'danger'"
      [disabled]="disabled()"
    >
      <ng-content />
    </button>
  `,
  styles: `
    :host {
      display: inline-block;
    }
    :host.full-width,
    :host.full-width button {
      width: 100%;
    }
    button {
      min-height: 44px;
      padding: 0.7rem 1rem;
      border: 2px solid var(--coupon-color-primary);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-primary);
      color: var(--coupon-color-on-primary);
      font: 700 var(--coupon-font-size-md) / 1.2 var(--coupon-font-sans);
      cursor: pointer;
    }
    button:hover:not(:disabled) {
      filter: brightness(0.92);
    }
    button.secondary {
      background: transparent;
      color: var(--coupon-color-text);
      border-color: var(--coupon-color-border);
    }
    button.danger {
      background: var(--coupon-color-danger);
      border-color: var(--coupon-color-danger);
      color: var(--coupon-color-surface);
    }
    button:disabled {
      opacity: 0.55;
      cursor: not-allowed;
    }
  `,
})
export class CouponButtonComponent {
  readonly variant = input<"primary" | "secondary" | "danger">("primary");
  readonly type = input<"button" | "submit">("button");
  readonly disabled = input(false);
  readonly ariaLabel = input("");
  readonly fullWidth = input(false);
}
