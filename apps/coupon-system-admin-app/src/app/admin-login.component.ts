import {
  ChangeDetectionStrategy,
  Component,
  inject,
  signal,
} from "@angular/core";
import { FormBuilder, ReactiveFormsModule, Validators } from "@angular/forms";
import { Router } from "@angular/router";
import { AuthSessionService } from "@coupon/client-core";
import {
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";

@Component({
  selector: "coupon-admin-login",
  imports: [
    ReactiveFormsModule,
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
        <form [formGroup]="form" (ngSubmit)="submit()">
          <label
            >관리자 이메일<input
              type="email"
              formControlName="email"
              autocomplete="username"
          /></label>
          <label
            >비밀번호<input
              type="password"
              formControlName="password"
              autocomplete="current-password"
          /></label>
          @if (errorMessage()) {
            <p class="auth-error" role="alert">{{ errorMessage() }}</p>
          }
          <coupon-button
            type="submit"
            [fullWidth]="true"
            [disabled]="form.invalid || submitting()"
            >{{
              submitting() ? "확인 중…" : "안전한 로그인 계속"
            }}</coupon-button
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
    .auth-error {
      margin: 0;
      padding: 0.75rem;
      border-left: 4px solid var(--coupon-color-danger);
      background: var(--coupon-color-surface-muted);
      color: var(--coupon-color-danger);
      font-weight: 700;
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
export class AdminLoginComponent {
  private readonly auth = inject(AuthSessionService);
  private readonly router = inject(Router);
  private readonly formBuilder = inject(FormBuilder);

  readonly submitting = signal(false);
  readonly errorMessage = signal<string | null>(null);
  readonly form = this.formBuilder.nonNullable.group({
    email: ["", [Validators.required, Validators.email]],
    password: ["", [Validators.required, Validators.minLength(6)]],
  });

  async submit(): Promise<void> {
    if (this.form.invalid || this.submitting()) {
      this.form.markAllAsTouched();
      return;
    }

    this.submitting.set(true);
    this.errorMessage.set(null);
    const { email, password } = this.form.getRawValue();

    try {
      await this.auth.signInWithEmail(email, password);
      await this.router.navigateByUrl("/");
    } catch {
      this.errorMessage.set("이메일 또는 비밀번호가 맞지 않습니다.");
    } finally {
      this.submitting.set(false);
    }
  }
}
