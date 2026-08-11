import {
  ChangeDetectionStrategy,
  Component,
  inject,
  signal,
} from "@angular/core";
import { FormsModule } from "@angular/forms";
import { Router } from "@angular/router";
import { AuthSessionService } from "@coupon/client-core";
import {
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";
import { AccountApi } from "./account.api";
import {
  acknowledgeWithdrawalImpact,
  completeWithdrawal,
  completeWithdrawalReauthentication,
  initialWithdrawalFlow,
} from "./withdrawal-flow";

@Component({
  selector: "coupon-account-withdraw",
  imports: [
    FormsModule,
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <coupon-page-header
        title="계정 탈퇴"
        description="탈퇴가 혜택·상점·민원에 미치는 영향을 먼저 확인합니다."
        eyebrow="고위험 작업"
      />

      <ol class="steps" aria-label="탈퇴 진행 단계">
        <li [attr.aria-current]="flow().step === 'IMPACT' ? 'step' : null">
          영향 확인
        </li>
        <li
          [attr.aria-current]="
            flow().step === 'REAUTHENTICATION' ? 'step' : null
          "
        >
          재인증
        </li>
        <li [attr.aria-current]="flow().step === 'COMPLETE' ? 'step' : null">
          완료·보존 안내
        </li>
      </ol>

      @if (flow().step === "IMPACT") {
        <coupon-card>
          <h2>탈퇴 영향 요약</h2>
          <ul>
            <li>
              사용 가능 쿠폰은 즉시 <strong>REVOKED(탈퇴)</strong> 처리됩니다.
            </li>
            <li>미해결 민원이 있으면 처리 영향을 추가로 확인해야 합니다.</li>
            <li>활성 상점 소유자는 폐점 절차를 먼저 완료해야 합니다.</li>
            <li>동일 인증수단으로 재가입해도 과거 혜택은 복원되지 않습니다.</li>
          </ul>
          <label
            ><input type="checkbox" [(ngModel)]="acknowledged" /> 영향과 되돌릴
            수 없는 내용을 확인했습니다.</label
          >
          <coupon-button
            variant="danger"
            [disabled]="!acknowledged"
            (click)="continueToReauthentication()"
            >재인증으로 계속</coupon-button
          >
        </coupon-card>
      } @else if (flow().step === "REAUTHENTICATION") {
        <coupon-card>
          <h2>본인 재인증</h2>
          <p>
            비밀번호로 Firebase 재인증을 완료하면 새 ID Token의
            <code>auth_time</code>을 서버가 확인합니다. 재인증 토큰을 요청
            body에 보내지 않습니다.
          </p>
          <label
            >현재 비밀번호<input
              type="password"
              autocomplete="current-password"
              [(ngModel)]="password"
          /></label>
          <coupon-button
            variant="danger"
            [disabled]="busy() || password.length === 0"
            (click)="reauthenticateAndWithdraw()"
            >재인증하고 탈퇴</coupon-button
          >
        </coupon-card>
      } @else if (flow().step === "SUBMITTING") {
        <coupon-card role="status"
          ><h2>탈퇴를 처리하는 중입니다.</h2>
          <p>창을 닫지 말아 주세요.</p></coupon-card
        >
      } @else {
        <coupon-card role="status">
          <h2>탈퇴가 완료됐습니다.</h2>
          <p>
            선택 정보·알림 토큰은 파기됩니다. 법정 보존·분쟁 보존 대상 원장은
            가명화하여 보존되며 거래 무결성을 유지합니다.
          </p>
          <coupon-button (click)="finish()">확인</coupon-button>
        </coupon-card>
      }
      @if (error()) {
        <p class="error" role="alert">{{ error() }}</p>
      }
    </main>
  `,
  styles: `
    main {
      display: grid;
      width: min(100% - 2rem, 48rem);
      gap: 1rem;
      margin: 0 auto;
      padding: 2rem 0;
    }
    .steps {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 0.4rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .steps li {
      min-height: 44px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      color: var(--coupon-color-text-muted);
      text-align: center;
    }
    .steps li[aria-current="step"] {
      border-color: var(--coupon-color-primary);
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    h2 {
      margin-top: 0;
    }
    label {
      display: grid;
      gap: 0.4rem;
      min-height: 44px;
      margin: 1rem 0;
      font-weight: 700;
    }
    input[type="password"] {
      min-height: 44px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .error {
      color: var(--coupon-color-danger);
      font-weight: 700;
    }
  `,
})
export class AccountWithdrawComponent {
  private readonly auth = inject(AuthSessionService);
  private readonly api = inject(AccountApi);
  private readonly router = inject(Router);

  readonly flow = signal(initialWithdrawalFlow);
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  acknowledged = false;
  password = "";

  continueToReauthentication(): void {
    if (!this.acknowledged) return;
    this.flow.set(acknowledgeWithdrawalImpact());
  }

  async reauthenticateAndWithdraw(): Promise<void> {
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.auth.reauthenticateWithPassword(this.password);
      this.flow.set(completeWithdrawalReauthentication(this.flow()));
      this.api.withdraw().subscribe({
        next: async () => {
          this.flow.set(completeWithdrawal(this.flow()));
          await this.auth.signOut();
          this.busy.set(false);
        },
        error: () => {
          this.flow.set(acknowledgeWithdrawalImpact());
          this.error.set(
            "탈퇴 요청을 완료하지 못했습니다. 영향 요약과 계정 상태를 확인해 주세요.",
          );
          this.busy.set(false);
        },
      });
    } catch {
      this.error.set("재인증에 실패했습니다. 현재 비밀번호를 확인해 주세요.");
      this.busy.set(false);
    }
  }

  finish(): void {
    void this.router.navigateByUrl("/login");
  }
}
