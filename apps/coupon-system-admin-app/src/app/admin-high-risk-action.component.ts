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
import {
  AdminPhaseFourApi,
  type AdminUserActionRequest,
} from "./admin-phase-four.api";

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
      <a class="back" routerLink="/members">← 회원·상점으로</a>
      <h2>작업 영향</h2>
      <dl>
        <div>
          <dt>회원 UUID</dt>
          <dd>{{ userId }}</dd>
        </div>
        <div>
          <dt>작업</dt>
          <dd>{{ actionLabel() }}</dd>
        </div>
        <div>
          <dt>감사</dt>
          <dd>사유와 사건 ID 기록</dd>
        </div>
      </dl>
      <section class="reversibility irreversible">
        <h3>되돌림 가능성</h3>
        <p>
          자동으로 되돌릴 수 없습니다. 잘못된 처리는 별도 민원·보정 승인이
          필요합니다.
        </p>
      </section>
      @if (!complete()) {
        <form (ngSubmit)="submit()">
          <label>
            사건 UUID {{ action === "revoke-sessions" ? "(선택)" : "" }}
            <input
              name="caseId"
              [(ngModel)]="caseId"
              autocomplete="off"
              [required]="action === 'suspend'"
            />
          </label>
          @if (action === "suspend") {
            <label>
              제재 유형
              <select name="sanctionType" [(ngModel)]="sanctionType">
                <option value="TEMPORARY">임시 제재</option>
                <option value="PERMANENT">영구 제재</option>
              </select>
            </label>
            @if (sanctionType === "TEMPORARY") {
              <label>
                제재 종료 시각
                <input
                  name="expiresAt"
                  type="datetime-local"
                  [(ngModel)]="expiresAt"
                  required
                />
              </label>
            } @else {
              <label>
                2차 승인 관리자 UUID
                <input
                  name="approvedByUserId"
                  [(ngModel)]="approvedByUserId"
                  autocomplete="off"
                  required
                />
              </label>
            }
            <label>
              사용자 안내 사유
              <textarea
                name="publicReason"
                [(ngModel)]="publicReason"
                rows="3"
                minlength="10"
                required
              ></textarea>
            </label>
            <label>
              내부 운영 사유
              <textarea
                name="internalReason"
                [(ngModel)]="internalReason"
                rows="3"
                minlength="10"
                required
              ></textarea>
            </label>
          } @else {
            <label>
              관리자 사유
              <textarea
                name="reason"
                [(ngModel)]="reason"
                rows="4"
                minlength="10"
                required
              ></textarea>
            </label>
          }
          <label>
            현재 비밀번호
            <input
              name="password"
              type="password"
              [(ngModel)]="password"
              autocomplete="current-password"
              required
            />
          </label>
          <label class="check">
            <input name="ack" type="checkbox" [(ngModel)]="acknowledged" />
            영향과 되돌림 가능성을 확인했습니다.
          </label>
          @if (error()) {
            <p class="error" role="alert">{{ error() }}</p>
          }
          <coupon-button
            type="submit"
            variant="danger"
            [fullWidth]="true"
            [disabled]="busy() || !validRequest() || !password || !acknowledged"
            >재인증하고 실행</coupon-button
          >
        </form>
      } @else {
        <section class="success" role="status">
          <span aria-hidden="true">✓</span>
          <h2>작업을 접수했습니다</h2>
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
      align-items: center;
      min-height: 44px;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    dl {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
    dl div {
      padding: 0.65rem;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dd {
      margin: 0.3rem 0 0;
      overflow-wrap: anywhere;
      font-weight: 800;
    }
    .reversibility {
      padding: 1rem;
      border: 1px solid var(--coupon-color-danger);
      border-radius: var(--coupon-radius-sm);
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
    select,
    textarea {
      min-height: 44px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
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
      color: var(--coupon-color-danger);
      font-weight: 800;
    }
    .success {
      display: grid;
      justify-items: center;
      gap: 0.5rem;
      text-align: center;
    }
    .success > span {
      color: var(--coupon-color-success);
      font-size: 2.5rem;
    }
    @media (max-width: 767px) {
      dl {
        grid-template-columns: 1fr;
      }
    }
  `,
})
export class AdminHighRiskActionComponent {
  private readonly route = inject(ActivatedRoute);
  private readonly auth = inject(AuthSessionService);
  private readonly api = inject(AdminPhaseFourApi);

  readonly title =
    this.route.snapshot.queryParamMap.get("title") ?? "고위험 작업";
  readonly userId = this.route.snapshot.queryParamMap.get("userId") ?? "";
  readonly action =
    this.route.snapshot.queryParamMap.get("action") === "suspend"
      ? "suspend"
      : "revoke-sessions";
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly complete = signal(false);
  caseId = "";
  reason = "";
  sanctionType: "TEMPORARY" | "PERMANENT" = "TEMPORARY";
  expiresAt = "";
  publicReason = "";
  internalReason = "";
  approvedByUserId = "";
  password = "";
  acknowledged = false;

  actionLabel(): string {
    return this.action === "suspend" ? "회원 제재" : "모든 세션 폐기";
  }

  validRequest(): boolean {
    if (!isUuid(this.userId)) return false;
    if (this.action === "revoke-sessions") {
      return (
        this.reason.trim().length >= 10 &&
        (this.caseId.length === 0 || isUuid(this.caseId))
      );
    }
    return (
      isUuid(this.caseId) &&
      this.publicReason.trim().length >= 10 &&
      this.internalReason.trim().length >= 10 &&
      (this.sanctionType === "TEMPORARY"
        ? Boolean(this.expiresAt)
        : isUuid(this.approvedByUserId))
    );
  }

  async submit(): Promise<void> {
    if (this.busy() || !this.acknowledged || !this.validRequest()) return;
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
    this.api.userAction(this.request()).subscribe({
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

  private request(): AdminUserActionRequest {
    if (this.action === "revoke-sessions") {
      return {
        action: "revoke-sessions",
        userId: this.userId,
        reason: this.reason.trim(),
        caseId: this.caseId || null,
      };
    }
    return {
      action: "suspend",
      userId: this.userId,
      sanctionType: this.sanctionType,
      caseId: this.caseId,
      publicReason: this.publicReason.trim(),
      internalReason: this.internalReason.trim(),
      expiresAt:
        this.sanctionType === "TEMPORARY"
          ? new Date(this.expiresAt).toISOString()
          : null,
      approvedByUserId:
        this.sanctionType === "PERMANENT" ? this.approvedByUserId : null,
    };
  }
}

function isUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
    value,
  );
}
