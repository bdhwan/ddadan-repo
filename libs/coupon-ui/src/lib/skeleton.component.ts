import { ChangeDetectionStrategy, Component, input } from '@angular/core';

@Component({
  selector: 'coupon-skeleton',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'status', '[attr.aria-label]': 'label()' },
  template: `
    <span class="sr-only">{{ label() }}</span>
    @for (line of linesArray(); track $index) { <span class="line" aria-hidden="true"></span> }
  `,
  styles: `
    :host { display: grid; gap: .75rem; padding: 1rem 0; }
    .line { height: 1rem; border-radius: 999px; background: linear-gradient(90deg, var(--coupon-color-surface-muted), var(--coupon-color-border), var(--coupon-color-surface-muted)); background-size: 200% 100%; animation: loading 1.4s infinite; }
    .line:nth-of-type(2n) { width: 74%; }
    .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0,0,0,0); }
    @keyframes loading { to { background-position: -200% 0; } }
  `,
})
export class CouponSkeletonComponent {
  readonly lines = input(4);
  readonly label = input('내용을 불러오는 중입니다.');
  readonly linesArray = () => Array.from({ length: Math.max(1, this.lines()) });
}
