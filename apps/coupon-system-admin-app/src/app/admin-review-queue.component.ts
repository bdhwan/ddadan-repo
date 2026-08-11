import { ChangeDetectionStrategy, Component, signal } from "@angular/core";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponEmptyStateComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";

@Component({
  selector: "coupon-admin-review-queue",
  imports: [
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponEmptyStateComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="상점 검수 큐"
      description="신청, 증빙, 중복 신호를 검토합니다. 민감정보 원문은 기본 마스킹됩니다."
      eyebrow="Operations"
    >
      <coupon-button variant="secondary" (click)="state.set('loading')"
        >새로고침</coupon-button
      >
    </coupon-page-header>
    <div class="filters" aria-label="검수 필터">
      <label
        >상태<select>
          <option>검수 대기</option>
          <option>보완 필요</option>
        </select></label
      >
      <label>검색<input type="search" placeholder="상점명·신청 ID" /></label>
    </div>
    <coupon-card>
      @switch (state()) {
        @case ("loading") {
          <coupon-skeleton [lines]="7" />
        }
        @case ("error") {
          <coupon-error-state
            requestId="review-demo"
            (retry)="state.set('loading')"
          />
        }
        @case ("empty") {
          <coupon-empty-state
            title="검수 대기 상점이 없습니다"
            description="새 신청이 들어오면 이곳에 표시됩니다."
          />
        }
        @default {
          <div class="table-wrap">
            <table>
              <caption class="sr-only">
                상점 검수 대기 목록
              </caption>
              <thead>
                <tr>
                  <th scope="col">신청</th>
                  <th scope="col">상점</th>
                  <th scope="col">민감정보</th>
                  <th scope="col">중복 신호</th>
                  <th scope="col">상태</th>
                  <th scope="col">작업</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td>review_8f2…</td>
                  <td>성수 커피랩</td>
                  <td>사업자 123-••-67890<br />대표 김•수</td>
                  <td><span aria-hidden="true">⚠</span> 주소 유사 1건</td>
                  <td>
                    <coupon-badge status="warning" label="검수 대기"
                      >검수 대기</coupon-badge
                    >
                  </td>
                  <td>
                    <coupon-button variant="secondary" [disabled]="true"
                      >상세 검토</coupon-button
                    >
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        }
      }
      <div
        class="state-controls"
        role="group"
        aria-label="검수 큐 상태 미리보기"
      >
        <coupon-button variant="secondary" (click)="state.set('ready')"
          >목록</coupon-button
        >
        <coupon-button variant="secondary" (click)="state.set('empty')"
          >빈 상태</coupon-button
        >
        <coupon-button variant="secondary" (click)="state.set('error')"
          >오류</coupon-button
        >
      </div>
    </coupon-card>
  `,
  styles: `
    .filters {
      display: flex;
      flex-wrap: wrap;
      gap: 0.75rem;
      margin-bottom: 1rem;
    }
    label {
      display: grid;
      gap: 0.25rem;
      font-weight: 700;
    }
    input,
    select {
      min-height: 44px;
      padding: 0.55rem 0.7rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-surface);
      color: var(--coupon-color-text);
    }
    .table-wrap {
      overflow-x: auto;
    }
    table {
      width: 100%;
      min-width: 760px;
      border-collapse: collapse;
    }
    th,
    td {
      padding: 0.8rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
      vertical-align: middle;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    .state-controls {
      display: flex;
      gap: 0.5rem;
      margin-top: 1rem;
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
export class AdminReviewQueueComponent {
  readonly state = signal<"ready" | "loading" | "empty" | "error">("ready");
}
