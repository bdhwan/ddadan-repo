import {
  ChangeDetectionStrategy,
  Component,
  input,
  output,
} from "@angular/core";
import { CouponButtonComponent } from "./button.component";

@Component({
  selector: "coupon-error-state",
  imports: [CouponButtonComponent],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: "alert" },
  template: `
    <span class="icon" aria-hidden="true">⚠</span>
    <h2>{{ title() }}</h2>
    <p>{{ description() }}</p>
    @if (requestId()) {
      <small>문의용 식별번호 {{ requestId() }}</small>
    }
    @if (retryable()) {
      <coupon-button (click)="retry.emit()">다시 시도</coupon-button>
    }
  `,
  styles: `
    :host {
      display: grid;
      justify-items: center;
      gap: 0.5rem;
      padding: 2rem 1rem;
      text-align: center;
      border: 1px solid var(--coupon-color-danger);
      border-radius: var(--coupon-radius-md);
    }
    .icon {
      font-size: 2rem;
      color: var(--coupon-color-danger);
    }
    h2,
    p {
      margin: 0;
    }
    p,
    small {
      color: var(--coupon-color-text-muted);
    }
  `,
})
export class CouponErrorStateComponent {
  readonly title = input("정보를 불러오지 못했어요");
  readonly description = input("연결 상태를 확인하고 다시 시도해 주세요.");
  readonly requestId = input<string | null>(null);
  readonly retryable = input(true);
  readonly retry = output<void>();
}
