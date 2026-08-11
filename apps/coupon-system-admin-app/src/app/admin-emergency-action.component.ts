import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormsModule } from "@angular/forms";
import { ActivatedRoute, RouterLink } from "@angular/router";
import {
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";
import { AdminOperationsApi } from "./admin-operations.api";

@Component({
  selector: "coupon-admin-emergency-action",
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
      title="고위험 캠페인 작업 재인증"
      description="일반 확인 모달이 아닙니다. 작업 영향과 되돌림 가능성을 읽고 재인증하세요."
      eyebrow="Reauthentication"
    />
    <coupon-card>
      <a routerLink="/campaigns" class="back">← 캠페인 목록으로</a>
      <h2>{{ actionLabel() }} 영향 요약</h2>
      <dl>
        <div>
          <dt>캠페인</dt>
          <dd>{{ name }}</dd>
        </div>
        <div>
          <dt>상점</dt>
          <dd>{{ store }}</dd>
        </div>
        <div>
          <dt>이미 발급</dt>
          <dd>{{ issued }}건</dd>
        </div>
        <div>
          <dt>이미 사용</dt>
          <dd>{{ used }}건 (원장 보존)</dd>
        </div>
        @if (action === "revoke") {
          <div>
            <dt>예상 회수</dt>
            <dd>{{ revokeCount }}건</dd>
          </div>
        }
      </dl>
      <section class="reversibility" [class.irreversible]="!reversible">
        <h3>되돌림 가능성</h3>
        <p>
          {{
            reversible
              ? "긴급 중단 후 상태를 검증하여 안전하게 재개할 수 있습니다. 이미 발급된 쿠폰은 별도 회수 전까지 유지됩니다."
              : "회수 작업이 진행되면 자동으로 되돌릴 수 없습니다. 잘못된 회수는 별도 보정·민원 승인이 필요합니다."
          }}
        </p>
      </section>
      @if (!completed()) {
        <form (ngSubmit)="submit()">
          <label
            >운영 사유<textarea
              name="reason"
              [(ngModel)]="reason"
              rows="4"
              required
              minlength="10"
            ></textarea
            ><small
              >감사 로그와 당사자 안내에 사용할 구체적 사유를 10자 이상
              입력하세요.</small
            ></label
          ><label
            >관리자 재인증 토큰<input
              name="reauth"
              type="password"
              [(ngModel)]="reauthenticationToken"
              autocomplete="current-password"
              required /></label
          ><label class="check"
            ><input
              name="understood"
              type="checkbox"
              [(ngModel)]="understood"
            />작업 영향과 되돌림 가능성을 이해했습니다.</label
          >
          @if (error()) {
            <p class="error" role="alert">{{ error() }}</p>
          }
          <coupon-button
            type="submit"
            [fullWidth]="true"
            [disabled]="
              reason.trim().length < 10 ||
              !reauthenticationToken ||
              !understood ||
              submitting()
            "
            >{{
              submitting() ? "재인증·요청 중…" : actionLabel() + " 요청"
            }}</coupon-button
          >
        </form>
      } @else {
        <section class="success" role="status">
          <span aria-hidden="true">✓</span>
          <h2>요청을 접수했습니다</h2>
          <p>
            거래 ID <code>{{ transactionId() }}</code>
          </p>
          <p>완료 시점과 결과는 작업 큐에서 확인하세요.</p>
          <a routerLink="/jobs">작업 큐로 이동</a>
        </section>
      }
    </coupon-card>
  `,
  styles: `
    :host {
      display: block;
      max-width: 52rem;
    }
    .back {
      display: inline-flex;
      align-items: center;
      min-height: 44px;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    dl {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      margin: 1rem 0;
    }
    dl div {
      padding: 0.65rem;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dt {
      color: var(--coupon-color-text-muted);
    }
    dd {
      margin: 0.2rem 0 0;
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
    form {
      display: grid;
      gap: 1rem;
      margin-top: 1.25rem;
    }
    label {
      display: grid;
      gap: 0.35rem;
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
    small {
      color: var(--coupon-color-text-muted);
    }
    .check {
      grid-template-columns: 24px 1fr;
      align-items: center;
      min-height: 44px;
    }
    .check input {
      width: 22px;
      height: 22px;
    }
    .error {
      padding: 0.75rem;
      border-left: 4px solid var(--coupon-color-danger);
      color: var(--coupon-color-danger);
    }
    .success {
      text-align: center;
    }
    .success > span {
      display: grid;
      place-items: center;
      width: 4rem;
      height: 4rem;
      margin: auto;
      border: 3px solid var(--coupon-color-success);
      border-radius: 50%;
      color: var(--coupon-color-success);
      font-size: 2rem;
    }
    code {
      overflow-wrap: anywhere;
    }
  `,
})
export class AdminEmergencyActionComponent {
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(AdminOperationsApi);
  private readonly destroyRef = inject(DestroyRef);
  readonly campaignId = this.route.snapshot.paramMap.get("id") ?? "";
  readonly action =
    this.route.snapshot.queryParamMap.get("action") === "revoke"
      ? "revoke"
      : "stop";
  readonly name = this.route.snapshot.queryParamMap.get("name") ?? "캠페인";
  readonly store = this.route.snapshot.queryParamMap.get("store") ?? "상점";
  readonly issued = Number(
    this.route.snapshot.queryParamMap.get("issued") ?? 0,
  );
  readonly used = Number(this.route.snapshot.queryParamMap.get("used") ?? 0);
  readonly revokeCount = Number(
    this.route.snapshot.queryParamMap.get("revoke_count") ?? 0,
  );
  readonly reversible =
    this.action === "stop" &&
    this.route.snapshot.queryParamMap.get("reversible") === "true";
  readonly submitting = signal(false);
  readonly error = signal<string | null>(null);
  readonly completed = signal(false);
  readonly transactionId = signal("");
  reason = "";
  reauthenticationToken = "";
  understood = false;
  actionLabel(): string {
    return this.action === "revoke" ? "대량 회수" : "긴급 중단";
  }
  submit(): void {
    if (
      this.reason.trim().length < 10 ||
      !this.reauthenticationToken ||
      !this.understood ||
      this.submitting()
    )
      return;
    this.submitting.set(true);
    this.error.set(null);
    this.api
      .emergencyCampaignAction(
        this.campaignId,
        {
          action: this.action === "revoke" ? "REVOKE" : "EMERGENCY_STOP",
          reason: this.reason.trim(),
          reauthentication_token: this.reauthenticationToken,
          understood_reversibility: true,
        },
        createUuid(),
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (response) => {
          this.transactionId.set(response.transaction_id);
          this.completed.set(true);
          this.reauthenticationToken = "";
          this.submitting.set(false);
        },
        error: () => {
          this.error.set(
            "재인증이 만료되었거나 작업 상태가 변경되었습니다. 현황을 확인하고 다시 시도하세요.",
          );
          this.reauthenticationToken = "";
          this.submitting.set(false);
        },
      });
  }
}

function createUuid(): string {
  return typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (character) => {
        const random = Math.floor(Math.random() * 16);
        return (character === "x" ? random : (random & 3) | 8).toString(16);
      });
}
