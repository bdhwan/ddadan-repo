import { ChangeDetectionStrategy, Component } from "@angular/core";
import { CouponFeatureStateComponent } from "@coupon/ui";

@Component({
  selector: "coupon-store-feature-state",
  imports: [CouponFeatureStateComponent],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-feature-state />
    <section class="desktop-table" aria-labelledby="desktop-table-title">
      <h2 id="desktop-table-title">데스크톱 데이터 표 스텁</h2>
      <p>1280px 이상에서 보이는 Phase 1 목록 레이아웃입니다.</p>
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th scope="col">식별번호</th>
              <th scope="col">상태</th>
              <th scope="col">요약</th>
              <th scope="col">업데이트</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td colspan="4">현재 표시할 데이터가 없습니다.</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  `,
  styles: `
    .desktop-table {
      display: none;
      margin-top: 1.5rem;
    }
    .desktop-table h2 {
      margin: 0;
      font-size: var(--coupon-font-size-lg);
    }
    .desktop-table p {
      margin: 0.35rem 0 1rem;
      color: var(--coupon-color-text-muted);
    }
    .table-wrap {
      overflow-x: auto;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    table {
      width: 100%;
      min-width: 760px;
      border-collapse: collapse;
    }
    th,
    td {
      padding: 0.8rem 1rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    @media (min-width: 1280px) {
      .desktop-table {
        display: block;
      }
    }
  `,
})
export class StoreFeatureStateComponent {}
