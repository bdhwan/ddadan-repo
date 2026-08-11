import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ActivatedRoute, RouterLink } from "@angular/router";
import { CouponCardComponent, CouponPageHeaderComponent } from "@coupon/ui";

@Component({
  selector: "coupon-store-detail",
  imports: [RouterLink, CouponCardComponent, CouponPageHeaderComponent],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      [title]="slug ? '상점 상세' : '공개 상점 찾기'"
      description="공개 상점과 캠페인을 둘러보는 기능입니다."
      eyebrow="Public stores"
    />
    <coupon-card>
      <section class="coming-soon" role="status" aria-labelledby="store-ready">
        <span aria-hidden="true">◇</span>
        <h2 id="store-ready">공개 상점 기능은 준비 중입니다</h2>
        <p>
          기획된 <code>GET /public/stores</code>와
          <code>GET /public/stores/:slug</code> API가 아직 서버에 제공되지 않아
          요청을 보내지 않습니다.
        </p>
        @if (slug) {
          <p>
            요청한 상점 주소: <strong>{{ slug }}</strong>
          </p>
        }
        <p>
          상점 검색·상세·관심 등록은 API 제공 후 활성화됩니다. 현재 사용 가능한
          쿠폰과 도장은 <a routerLink="/wallet">지갑에서 확인</a>해 주세요.
        </p>
      </section>
    </coupon-card>
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
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
      max-width: 40rem;
      color: var(--coupon-color-text-muted);
    }
    a {
      display: inline-flex;
      align-items: center;
      min-height: 44px;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    code,
    strong {
      color: var(--coupon-color-text);
    }
  `,
})
export class StoreDetailComponent {
  private readonly route = inject(ActivatedRoute);
  readonly slug = this.route.snapshot.paramMap.get("slug");
}
