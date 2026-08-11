import { ChangeDetectionStrategy, Component, input } from "@angular/core";

@Component({
  selector: "coupon-page-header",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div>
      @if (eyebrow()) {
        <p class="eyebrow">{{ eyebrow() }}</p>
      }
      <h1>{{ title() }}</h1>
      @if (description()) {
        <p class="description">{{ description() }}</p>
      }
    </div>
    <div class="actions"><ng-content /></div>
  `,
  styles: `
    :host {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 1rem;
      margin-bottom: var(--coupon-space-lg);
    }
    h1,
    p {
      margin: 0;
    }
    h1 {
      font-size: var(--coupon-font-size-xl);
      line-height: 1.2;
    }
    .eyebrow {
      margin-bottom: 0.35rem;
      color: var(--coupon-color-primary);
      font-size: var(--coupon-font-size-sm);
      font-weight: 800;
      letter-spacing: 0.06em;
      text-transform: uppercase;
    }
    .description {
      margin-top: 0.5rem;
      max-width: 48rem;
      color: var(--coupon-color-text-muted);
    }
    .actions {
      display: flex;
      gap: 0.5rem;
    }
    @media (max-width: 480px) {
      :host {
        flex-direction: column;
      }
    }
  `,
})
export class CouponPageHeaderComponent {
  readonly title = input.required<string>();
  readonly description = input("");
  readonly eyebrow = input("");
}
