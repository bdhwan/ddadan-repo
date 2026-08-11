import { ChangeDetectionStrategy, Component } from "@angular/core";
import { CouponCardComponent, CouponPageHeaderComponent } from "@coupon/ui";

@Component({
  selector: "coupon-account-withdraw",
  imports: [CouponCardComponent, CouponPageHeaderComponent],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <coupon-page-header
        title="계정 탈퇴"
        description="탈퇴 영향과 보존 범위를 확인하는 화면입니다."
        eyebrow="Account"
      />
      <coupon-card>
        <section class="coming-soon" role="status" aria-labelledby="title">
          <span aria-hidden="true">◇</span>
          <h2 id="title">계정 탈퇴는 준비 중입니다</h2>
          <p>
            기획된 <code>POST /me/withdrawal</code> API가 아직 서버에 제공되지
            않아 탈퇴 요청을 전송하지 않습니다. API가 준비되기 전에는 오류나
            완료 화면을 가장하지 않습니다.
          </p>
          <p>
            출시 전에는 고객 지원을 통해 보존 대상 거래·민원 기록과 처리 절차를
            안내받아 주세요.
          </p>
        </section>
      </coupon-card>
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
    .coming-soon {
      display: grid;
      justify-items: center;
      gap: 0.75rem;
      text-align: center;
    }
    .coming-soon > span {
      color: var(--coupon-color-primary);
      font-size: 2.5rem;
    }
    h2,
    p {
      margin: 0;
    }
    p {
      max-width: 38rem;
      color: var(--coupon-color-text-muted);
    }
    code {
      color: var(--coupon-color-text);
      font-weight: 800;
    }
  `,
})
export class AccountWithdrawComponent {}
