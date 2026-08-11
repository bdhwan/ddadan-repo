import {
  ChangeDetectionStrategy,
  Component,
  inject,
  signal,
} from "@angular/core";
import { FormsModule } from "@angular/forms";
import { ActivatedRoute, RouterLink } from "@angular/router";
import { AuthSessionService } from "@coupon/client-core";
import {
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";
import { AdminPhaseFourApi } from "./admin-phase-four.api";

@Component({
  selector: "coupon-admin-high-risk-action",
  imports: [
    FormsModule,
    RouterLink,
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      [title]="title"
      description="작업 영향과 되돌림 가능성을 확인한 후 Firebase 재인증을 완료합니다."
      eyebrow="Reauthentication"
    />
    <coupon-card>
      <a class="back" routerLink="../">← 이전 목록으로</a>
      <h2>작업 영향</h2>
      <dl>
        <div>
          <dt>대상</dt>
          <dd>{{ target }}</dd>
        </div>
        <div>
          <dt>세션</dt>
          <dd>즉시 폐기·변경 차단</dd>
        </div>
        <div>
          <dt>관련 사건</dt>
          <dd>감사 로그와 민원에 연결</dd>
        </div>
      </dl>
      <section class="reversibility" [class.irreversible]="!reversible">
        <h3>되돌림 가능성</h3>
        <p>
          {{
            reversible
              ? "별도 승인 후 상태를 복구할 수 있습니다."
              : "자동으로 되돌릴 수 없습니다. 잘못된 처리는 별도 민원·보정 승인이 필요합니다."
          }}
        </p>
      </section>
      @if (!complete()) {
        <form (ngSubmit)="submit()">
          <label
            >관리자 사유<textarea
              name="reason"
              [(ngModel)]="reason"
              rows="4"
              minlength="10"
              required
            ></textarea>
          </label>
          <label
            >현재 비밀번호<input
              name="password"
              type="password"
              [(ngModel)]="password"
              autocomplete="current-password"
              required
          /></label>
          <label class="check"
            ><input name="ack" type="checkbox" [(ngModel)]="acknowledged" />
            영향과 되돌림 가능성을 확인했습니다.</label
          >
          @if (error()) {
            <p class="error" role="alert">{{ error() }}</p>
          }
          <coupon-button
            type="submit"
            variant="danger"
            [fullWidth]="true"
            [disabled]="
              busy() ||
              reason.trim().length < 10 ||
              password.length === 0 ||
              !acknowledged
            "
            >재인증하고 실행</coupon-button
          >
        </form>
      } @else {
        <section class="success" role="status">
          <h2>작업을 접수했습니다.</h2>
          <p>결과는 관련 사건·감사 로그에 남습니다.</p>
        </section>
      }
    </coupon-card>
  `,
  styles: `
    :host {
      display: grid;
      max-width: 52rem;
      gap: 1rem;
    }
    .back {
      display: inline-flex;
      min-height: 44px;
      align-items: center;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    dl {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
    }
    dl div {
      padding: 0.65rem;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dd {
      margin: 0.3rem 0 0;
      font-weight: 800;
    }
    .reversibility {
      padding: 1rem;
      border: 1px solid var(--coupon-color-warning);
      border-radius: var(--coupon-radius-sm);
    }
    .reversibility.irreversible {
      border-color: var(--coupon-color-danger);
    }
    form,
    label {
      display: grid;
      gap: 0.4rem;
    }
    form {
      gap: 1rem;
      margin-top: 1rem;
    }
    label {
      font-weight: 800;
    }
    input,
    textarea {
      min-height: 44px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .check {
      display: flex;
      align-items: center;
      min-height: 44px;
    }
    .check input {
      width: 1.75rem;
    }
    .error {
      color: var(--coupon-color-danger);
    }
  `,
})
export class AdminHighRiskActionComponent {
  private readonly route = inject(ActivatedRoute);
  private readonly auth = inject(AuthSessionService);
  private readonly api = inject(AdminPhaseFourApi);

  readonly title =
    this.route.snapshot.queryParamMap.get("title") ?? "고위험 작업";
  readonly target = this.route.snapshot.queryParamMap.get("target") ?? "대상";
  readonly endpoint = this.route.snapshot.queryParamMap.get("endpoint") ?? "";
  readonly reversible =
    this.route.snapshot.queryParamMap.get("reversible") === "true";
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly complete = signal(false);
  reason = "";
  password = "";
  acknowledged = false;

  async submit(): Promise<void> {
    if (this.busy() || !this.acknowledged || this.reason.trim().length < 10)
      return;
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.auth.reauthenticateWithPassword(this.password);
    } catch {
      this.error.set("관리자 재인증을 완료하지 못했습니다.");
      this.password = "";
      this.busy.set(false);
      return;
    }
    this.api.highRiskAction(this.endpoint, this.reason.trim()).subscribe({
      next: () => {
        this.complete.set(true);
        this.password = "";
        this.busy.set(false);
      },
      error: () => {
        this.error.set("작업 상태가 변경됐거나 재인증이 만료됐습니다.");
        this.password = "";
        this.busy.set(false);
      },
    });
  }
}
