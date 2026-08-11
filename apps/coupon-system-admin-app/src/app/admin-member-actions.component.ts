import { ChangeDetectionStrategy, Component } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { RouterLink } from "@angular/router";
import { CouponCardComponent, CouponPageHeaderComponent } from "@coupon/ui";

@Component({
  selector: "coupon-admin-member-actions",
  imports: [
    FormsModule,
    RouterLink,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="회원·상점"
      description="감사 가능한 회원 식별자로 제재 또는 관리자 세션 폐기를 시작합니다."
      eyebrow="Members"
    />
    <coupon-card>
      <section aria-labelledby="member-action-title">
        <h2 id="member-action-title">회원 고위험 작업</h2>
        <p>
          서버는 회원 목록 API를 제공하지 않습니다. 존재하지 않는
          <code>GET /admin/members</code> 대신 민원·거래에서 확인한 회원 UUID를
          사용합니다.
        </p>
        <label>
          회원 UUID
          <input
            [(ngModel)]="userId"
            autocomplete="off"
            placeholder="00000000-0000-0000-0000-000000000000"
          />
        </label>
        @if (userId && !validUserId()) {
          <p class="error" role="alert">올바른 회원 UUID를 입력해 주세요.</p>
        }
        <div class="actions">
          <a
            [class.disabled]="!validUserId()"
            [attr.aria-disabled]="!validUserId()"
            [attr.tabindex]="validUserId() ? 0 : -1"
            [routerLink]="validUserId() ? ['/high-risk-action'] : null"
            [queryParams]="params('revoke-sessions')"
            >모든 세션 폐기 검토</a
          >
          <a
            [class.disabled]="!validUserId()"
            [attr.aria-disabled]="!validUserId()"
            [attr.tabindex]="validUserId() ? 0 : -1"
            [routerLink]="validUserId() ? ['/high-risk-action'] : null"
            [queryParams]="params('suspend')"
            >회원 제재 검토</a
          >
        </div>
      </section>
    </coupon-card>
  `,
  styles: `
    :host,
    section,
    label {
      display: grid;
      gap: 1rem;
    }
    :host {
      max-width: 56rem;
    }
    h2,
    p {
      margin: 0;
    }
    label {
      gap: 0.4rem;
      font-weight: 800;
    }
    input {
      min-height: 44px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .actions {
      display: flex;
      flex-wrap: wrap;
      gap: 0.75rem;
    }
    .actions a {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-height: 44px;
      padding: 0.65rem 1rem;
      border: 2px solid var(--coupon-color-danger);
      border-radius: var(--coupon-radius-sm);
      color: var(--coupon-color-danger);
      font-weight: 800;
      text-decoration: none;
    }
    .actions a.disabled {
      opacity: 0.55;
      cursor: not-allowed;
    }
    .error {
      color: var(--coupon-color-danger);
      font-weight: 800;
    }
  `,
})
export class AdminMemberActionsComponent {
  userId = "";

  validUserId(): boolean {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      this.userId,
    );
  }

  params(action: "revoke-sessions" | "suspend"): Record<string, string> {
    return {
      action,
      userId: this.userId,
      target: this.userId,
      title: action === "suspend" ? "회원 제재" : "관리자 세션 폐기",
      reversible: "false",
    };
  }
}
