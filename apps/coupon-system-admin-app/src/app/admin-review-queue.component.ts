import {
  ChangeDetectionStrategy,
  Component,
  OnDestroy,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { FormsModule } from "@angular/forms";
import { ActivatedRoute, Router } from "@angular/router";
import type {
  AdminStoreReviewDto,
  StoreReviewStatusDto,
} from "@coupon/contracts";
import { formatKoreaDateTime } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponEmptyStateComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import type { Subscription } from "rxjs";
import { AdminPhaseFourApi } from "./admin-phase-four.api";
import {
  adminListQueryParams,
  normalizeAdminListQuery,
  type AdminListQuery,
} from "./admin-list-query";

@Component({
  selector: "coupon-admin-review-queue",
  imports: [
    FormsModule,
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
      description="신청·증빙·중복 신호를 마스킹한 상태로 검토합니다."
      eyebrow="Operations"
    >
      <coupon-button variant="secondary" (click)="load(query())"
        >새로고침</coupon-button
      >
    </coupon-page-header>
    <coupon-card>
      <form class="filters" (submit)="$event.preventDefault(); applyFilters()">
        <label
          >상태<select name="filter" [(ngModel)]="draftFilter">
            <option value="PENDING">검수 대기</option>
            <option value="NEEDS_MORE_INFO">보완 필요</option>
            <option value="ALL">전체</option>
          </select></label
        >
        <label
          >검색<input
            name="search"
            type="search"
            [(ngModel)]="draftSearch"
            placeholder="상점명·신청 ID"
        /></label>
        <coupon-button type="submit">필터 적용</coupon-button>
      </form>
      <p class="masking">사업자번호·대표자명 원문은 기본 마스킹됩니다.</p>
    </coupon-card>
    @if (loading()) {
      <coupon-card
        ><coupon-skeleton [lines]="8" label="검수 큐를 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="검수 큐를 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load(query())"
      />
    } @else if (items().length === 0) {
      <coupon-empty-state
        title="검수 대기 상점이 없습니다"
        description="새 신청이 들어오면 여기에 표시됩니다."
      />
    } @else {
      <div class="table-wrap">
        <table>
          <caption class="sr-only">
            상점 검수 목록
          </caption>
          <thead>
            <tr>
              <th>신청·상점</th>
              <th>마스킹 정보</th>
              <th>증빙</th>
              <th>중복 신호</th>
              <th>상태</th>
              <th>결정</th>
            </tr>
          </thead>
          <tbody>
            @for (review of items(); track review.id) {
              <tr>
                <td>
                  <strong>{{ review.store_name }}</strong
                  ><br /><code>{{ review.id.slice(0, 8) }}…</code
                  ><br /><small>{{ date(review.submitted_at) }}</small>
                </td>
                <td>
                  {{ review.business_number_masked }}<br />{{
                    review.owner_name_masked
                  }}
                </td>
                <td>{{ review.evidence_count }}건</td>
                <td>
                  {{
                    review.duplicate_signals.length
                      ? review.duplicate_signals.join(", ")
                      : "감지됨 없음"
                  }}
                </td>
                <td>
                  <coupon-badge
                    [status]="
                      review.status === 'PENDING' ? 'warning' : 'neutral'
                    "
                    [label]="review.status"
                    >{{ review.status }}</coupon-badge
                  >
                </td>
                <td>
                  <coupon-button variant="secondary" (click)="select(review)"
                    >검토·결정</coupon-button
                  >
                </td>
              </tr>
            }
          </tbody>
        </table>
      </div>
      <nav class="pagination" aria-label="검수 큐 페이지">
        <coupon-button
          variant="secondary"
          [disabled]="query().page <= 1"
          (click)="goPage(query().page - 1)"
          >이전</coupon-button
        ><span>{{ query().page }} 페이지</span
        ><coupon-button
          variant="secondary"
          [disabled]="!hasMore()"
          (click)="goPage(query().page + 1)"
          >다음</coupon-button
        >
      </nav>
    }
    @if (selected(); as review) {
      <coupon-card class="decision">
        <h2>{{ review.store_name }} 검수 결정</h2>
        <label
          >결정<select [(ngModel)]="decision">
            <option value="APPROVED">승인</option>
            <option value="NEEDS_MORE_INFO">보완 요청</option>
            <option value="REJECTED">거절</option>
          </select></label
        >
        <label
          >공개 가능 사유<textarea [(ngModel)]="reason" rows="3"></textarea>
        </label>
        <p>승인·보완·거절 결정과 증빙 조회는 모두 감사 로그에 남습니다.</p>
        <div class="actions">
          <coupon-button variant="secondary" (click)="selected.set(null)"
            >취소</coupon-button
          ><coupon-button
            [disabled]="saving() || reason.trim().length < 5"
            (click)="decide()"
            >결정 저장</coupon-button
          >
        </div>
      </coupon-card>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
    }
    .filters,
    .actions,
    .pagination {
      display: flex;
      flex-wrap: wrap;
      align-items: end;
      gap: 0.75rem;
    }
    label {
      display: grid;
      gap: 0.3rem;
      font-weight: 700;
    }
    input,
    select,
    textarea {
      min-height: 44px;
      padding: 0.55rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .masking,
    small,
    .decision p {
      color: var(--coupon-color-text-muted);
    }
    .table-wrap {
      overflow-x: auto;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
    }
    table {
      width: 100%;
      min-width: 1050px;
      border-collapse: collapse;
      background: var(--coupon-color-surface);
    }
    th,
    td {
      padding: 0.75rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
      vertical-align: top;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    .pagination {
      justify-content: center;
    }
    .decision {
      border-color: var(--coupon-color-warning);
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
export class AdminReviewQueueComponent implements OnInit, OnDestroy {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly api = inject(AdminPhaseFourApi);
  private subscription?: Subscription;

  readonly query = signal<AdminListQuery>({
    filter: "PENDING",
    search: "",
    page: 1,
  });
  readonly items = signal<AdminStoreReviewDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly hasMore = signal(false);
  readonly selected = signal<AdminStoreReviewDto | null>(null);
  readonly saving = signal(false);
  draftFilter = "PENDING";
  draftSearch = "";
  decision: StoreReviewStatusDto = "APPROVED";
  reason = "";

  ngOnInit(): void {
    this.subscription = this.route.queryParamMap.subscribe((params) => {
      const query = normalizeAdminListQuery({
        filter: params.get("filter") ?? "PENDING",
        search: params.get("search"),
        page: params.get("page"),
      });
      this.query.set(query);
      this.draftFilter = query.filter;
      this.draftSearch = query.search;
      this.load(query);
    });
  }

  ngOnDestroy(): void {
    this.subscription?.unsubscribe();
  }

  applyFilters(): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: adminListQueryParams({
        filter: this.draftFilter,
        search: this.draftSearch,
        page: 1,
      }),
    });
  }

  goPage(page: number): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: adminListQueryParams({ ...this.query(), page }),
    });
  }

  load(query: AdminListQuery): void {
    this.loading.set(true);
    this.api.storeReviews(query).subscribe({
      next: (page) => {
        this.items.set(page.items);
        this.hasMore.set(page.has_more);
        this.loading.set(false);
        this.error.set(null);
      },
      error: () => {
        this.loading.set(false);
        this.error.set("현재 필터의 검수 큐를 조회할 수 없습니다.");
      },
    });
  }

  select(review: AdminStoreReviewDto): void {
    this.selected.set(review);
    this.decision = "APPROVED";
    this.reason = "";
  }

  decide(): void {
    const review = this.selected();
    if (!review || this.reason.trim().length < 5) return;
    this.saving.set(true);
    this.api
      .decideStoreReview(review.id, this.decision, this.reason.trim())
      .subscribe({
        next: (updated) => {
          this.items.update((items) =>
            items.map((item) => (item.id === updated.id ? updated : item)),
          );
          this.selected.set(null);
          this.saving.set(false);
        },
        error: () => {
          this.error.set(
            "검수 결정을 저장하지 못했습니다. 버전과 권한을 확인해 주세요.",
          );
          this.saving.set(false);
        },
      });
  }

  date(value: string): string {
    return formatKoreaDateTime(value);
  }
}
