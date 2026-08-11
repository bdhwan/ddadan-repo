import { ChangeDetectionStrategy, Component } from "@angular/core";
import {
  CouponBadgeComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";

@Component({
  selector: "coupon-terms",
  imports: [
    CouponBadgeComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <coupon-page-header
        title="약관과 동의"
        description="필수 약관과 선택 동의를 구분해 현재 버전을 보여드립니다."
        eyebrow="Terms"
      />
      <section aria-labelledby="required-heading">
        <h2 id="required-heading">필수 약관</h2>
        <coupon-card>
          <div>
            <strong>서비스 이용약관</strong
            ><coupon-badge status="warning" label="필수">필수</coupon-badge>
          </div>
          <p>버전 2026-08-10 · 시행 2026. 8. 10.</p>
          <a href="/legal/terms/2026-08-10" target="_blank" rel="noopener"
            >전문 보기 <span class="sr-only">(새 창)</span></a
          >
        </coupon-card>
        <coupon-card>
          <div>
            <strong>개인정보 수집·이용</strong
            ><coupon-badge status="warning" label="필수">필수</coupon-badge>
          </div>
          <p>버전 2026-08-10 · 시행 2026. 8. 10.</p>
          <a href="/legal/privacy/2026-08-10" target="_blank" rel="noopener"
            >전문 보기 <span class="sr-only">(새 창)</span></a
          >
        </coupon-card>
      </section>
      <section aria-labelledby="optional-heading">
        <h2 id="optional-heading">선택 동의</h2>
        <coupon-card>
          <div>
            <strong>위치 기반 검색·마케팅·외부 알림</strong
            ><coupon-badge status="neutral" label="선택">선택</coupon-badge>
          </div>
          <p>동의하지 않아도 쿠폰·도장·앱 내 거래 알림은 이용할 수 있습니다.</p>
          <a href="/account/notifications">알림·마케팅 동의 관리</a>
        </coupon-card>
      </section>
    </main>
  `,
  styles: `
    main,
    section {
      display: grid;
      gap: 1rem;
    }
    main {
      width: min(100% - 2rem, 48rem);
      margin: 0 auto;
      padding: 2rem 0;
    }
    coupon-card > div {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 1rem;
    }
    p {
      color: var(--coupon-color-text-muted);
    }
    a {
      display: inline-flex;
      min-height: 44px;
      align-items: center;
      color: var(--coupon-color-primary);
      font-weight: 700;
    }
    .sr-only {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0, 0, 0, 0);
    }
  `,
})
export class TermsComponent {}
