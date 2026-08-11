import { ChangeDetectionStrategy, Component } from "@angular/core";
import {
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";

@Component({
  selector: "coupon-admin-login",
  imports: [
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <coupon-page-header
        title="운영 관리자 로그인"
        description="소비자·점주와 분리된 관리자 audience를 사용합니다."
        eyebrow="Restricted"
      />
      <coupon-card>
        <div class="notice" role="note">
          <span aria-hidden="true">⚿</span>
          <div>
            <strong>다중 요소 인증 필수</strong>
            <p>비밀번호 확인 후 등록된 추가 인증 수단을 요청합니다.</p>
          </div>
        </div>
        <form (submit)="$event.preventDefault()">
          <label
            >관리자 이메일<input type="email" autocomplete="username"
          /></label>
          <label
            >비밀번호<input type="password" autocomplete="current-password"
          /></label>
          <coupon-button type="submit" [fullWidth]="true"
            >안전한 로그인 계속</coupon-button
          >
        </form>
      </coupon-card>
    </main>
  `,
  styles: `
    main {
      width: min(100% - 2rem, 36rem);
      margin: 0 auto;
      padding: 3rem 0;
    }
    form {
      display: grid;
      gap: 1rem;
      margin-top: 1.5rem;
    }
    label {
      display: grid;
      gap: 0.4rem;
      font-weight: 700;
    }
    input {
      min-height: 44px;
      padding: 0.65rem 0.75rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .notice {
      display: grid;
      grid-template-columns: 2rem 1fr;
      gap: 0.75rem;
      padding: 0.75rem;
      border: 1px solid var(--coupon-color-warning);
      border-radius: var(--coupon-radius-sm);
    }
    .notice > span {
      color: var(--coupon-color-warning);
      font-size: 1.5rem;
    }
    .notice p {
      margin: 0.25rem 0 0;
      color: var(--coupon-color-text-muted);
    }
  `,
})
export class AdminLoginComponent {}
