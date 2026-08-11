import { ChangeDetectionStrategy, Component } from "@angular/core";
import { FormsModule } from "@angular/forms";
import { RouterLink } from "@angular/router";
import { CouponCardComponent, CouponPageHeaderComponent } from "@coupon/ui";

@Component({
  selector: "coupon-admin-campaigns",
  imports: [
    FormsModule,
    RouterLink,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="캠페인 운영"
      description="캠페인 식별자로 긴급 중단 또는 대량 회수 재인증 화면에 진입합니다."
      eyebrow="High-risk operations"
    />
    <coupon-card>
      <section aria-labelledby="campaign-action-title">
        <h2 id="campaign-action-title">캠페인 고위험 작업</h2>
        <p>
          서버는 관리자 캠페인 목록 API를 제공하지 않습니다. 따라서 존재하지
          않는
          <code>GET /admin/campaigns</code>를 호출하지 않고, 감사 가능한 캠페인
          ID로 기획서 §6.4의 긴급 작업만 시작합니다.
        </p>
        <label>
          캠페인 UUID
          <input
            [(ngModel)]="campaignId"
            autocomplete="off"
            placeholder="00000000-0000-0000-0000-000000000000"
          />
        </label>
        <label>
          화면에 표시할 캠페인 이름
          <input [(ngModel)]="campaignName" maxlength="80" />
        </label>
        @if (campaignId && !validCampaignId()) {
          <p class="error" role="alert">올바른 캠페인 UUID를 입력해 주세요.</p>
        }
        <div class="actions">
          <a
            [class.disabled]="!canContinue()"
            [attr.aria-disabled]="!canContinue()"
            [attr.tabindex]="canContinue() ? 0 : -1"
            [routerLink]="
              canContinue()
                ? ['/campaigns', campaignId, 'emergency-action']
                : null
            "
            [queryParams]="actionParams('stop')"
            >긴급 중단 검토</a
          >
          <a
            [class.disabled]="!canContinue()"
            [attr.aria-disabled]="!canContinue()"
            [attr.tabindex]="canContinue() ? 0 : -1"
            [routerLink]="
              canContinue()
                ? ['/campaigns', campaignId, 'emergency-action']
                : null
            "
            [queryParams]="actionParams('revoke')"
            >대량 회수 검토</a
          >
        </div>
      </section>
    </coupon-card>
    <p class="risk-note" role="note">
      <strong>목록 확인:</strong> 대상·처리·발급 진행은 지원되는 작업 큐와 거래
      탐색에서 확인합니다. 고위험 작업은 별도 재인증 화면에서 영향과 되돌림
      가능성을 다시 확인합니다.
    </p>
  `,
  styles: `
    :host {
      display: grid;
      max-width: 56rem;
      gap: 1rem;
    }
    section,
    label {
      display: grid;
      gap: 0.5rem;
    }
    section {
      gap: 1rem;
    }
    h2,
    p {
      margin: 0;
    }
    label {
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
    .risk-note {
      padding: 0.8rem;
      border-left: 4px solid var(--coupon-color-warning);
      background: var(--coupon-color-surface-muted);
    }
  `,
})
export class AdminCampaignsComponent {
  campaignId = "";
  campaignName = "";

  validCampaignId(): boolean {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
      this.campaignId,
    );
  }

  canContinue(): boolean {
    return this.validCampaignId() && this.campaignName.trim().length > 0;
  }

  actionParams(action: "stop" | "revoke"): Record<string, string | number> {
    return {
      action,
      name: this.campaignName.trim() || "캠페인",
      store: "거래 탐색에서 확인",
      issued: 0,
      used: 0,
      revoke_count: 0,
      reversible: String(action === "stop"),
    };
  }
}
