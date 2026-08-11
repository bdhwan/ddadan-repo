import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormBuilder, ReactiveFormsModule, Validators } from "@angular/forms";
import { ActivatedRoute, Router } from "@angular/router";
import type {
  AdminLedgerKind,
  AdminTransactionDetailDto,
} from "@coupon/contracts";
import { formatKoreaDateTime, formatWon } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponEmptyStateComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { AdminTransactionsApi } from "./admin-transactions.api";

@Component({
  selector: "coupon-admin-transaction-explorer",
  imports: [
    ReactiveFormsModule,
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
      title="거래 탐색"
      description="거래 ID 하나로 적립·사용·취소·보정 원장과 상태 변화를 추적합니다."
      eyebrow="Support & Audit"
    />
    <form class="search" [formGroup]="form" (ngSubmit)="search()">
      <label class="transaction-id"
        >거래 ID<input
          formControlName="transaction_id"
          placeholder="UUID 전체 값을 입력"
          autocomplete="off"
        /><small
          >문의 화면에 표시된 거래 ID를 그대로 붙여 넣으세요.</small
        ></label
      ><label
        >거래 유형<select formControlName="type">
          <option value="">전체</option>
          <option value="EARN">적립</option>
          <option value="REDEEM">사용</option>
          <option value="VOID">취소</option>
          <option value="ADJUSTMENT">보정</option>
        </select></label
      ><label>기준일<input type="date" formControlName="date" /></label
      ><coupon-button
        type="submit"
        [disabled]="form.controls.transaction_id.invalid"
        >조회</coupon-button
      >
    </form>
    <div class="privacy" role="note">
      <span aria-hidden="true">⚿</span>
      <p>
        <strong>민감정보 기본 마스킹</strong> · 고객·상점·외부 주문 참조의
        원문은 이 화면에 표시하지 않습니다. 조회 자체가 감사 로그에 기록됩니다.
      </p>
    </div>
    @if (!searched()) {
      <coupon-empty-state
        title="거래 ID로 사건 전체를 찾아보세요"
        description="필터와 페이지는 URL에 남아 새로고침하거나 링크를 공유해도 유지됩니다."
      />
    } @else if (loading()) {
      <coupon-card
        ><coupon-skeleton
          [lines]="9"
          label="원장과 감사 타임라인을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="거래를 찾지 못했어요"
        [description]="error()!"
        [requestId]="requestId()"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (data(); as tx) {
      <section class="summary">
        <div>
          <coupon-badge [status]="statusBadge(tx.status)" [label]="tx.status">{{
            tx.status
          }}</coupon-badge>
          <h2>{{ kindLabel(tx.transaction_type) }} 거래</h2>
          <code>{{ tx.transaction_id }}</code>
        </div>
        <dl>
          <div>
            <dt>상점</dt>
            <dd>{{ tx.store_name }} · {{ tx.store_reference_masked }}</dd>
          </div>
          <div>
            <dt>고객</dt>
            <dd>{{ tx.customer_reference_masked }}</dd>
          </div>
          <div>
            <dt>외부 주문</dt>
            <dd>{{ tx.external_order_ref_masked ?? "없음" }}</dd>
          </div>
          <div>
            <dt>주문 금액</dt>
            <dd>
              {{ tx.gross_amount ? won(tx.gross_amount.amount) : "해당 없음" }}
            </dd>
          </div>
          <div>
            <dt>최종 갱신</dt>
            <dd>{{ date(tx.updated_at) }} · v{{ tx.version }}</dd>
          </div>
          <div>
            <dt>요청 ID</dt>
            <dd>
              <code>{{ tx.request_id }}</code>
            </dd>
          </div>
        </dl>
      </section>
      <div class="content-grid">
        <section aria-labelledby="ledger-title">
          <h2 id="ledger-title">불변 원장</h2>
          <div class="table-wrap">
            <table>
              <caption class="sr-only">
                거래 연결 원장
              </caption>
              <thead>
                <tr>
                  <th>유형</th>
                  <th>증감</th>
                  <th>발생 시각</th>
                  <th>사유</th>
                  <th>행위자</th>
                </tr>
              </thead>
              <tbody>
                @for (entry of tx.ledgers; track entry.id) {
                  <tr>
                    <td>
                      <coupon-badge
                        [status]="entry.amount < 0 ? 'warning' : 'neutral'"
                        [label]="kindLabel(entry.kind)"
                        >{{ kindLabel(entry.kind) }}</coupon-badge
                      >
                    </td>
                    <td>
                      <strong
                        >{{ entry.amount > 0 ? "+" : ""
                        }}{{ entry.amount }}</strong
                      >
                    </td>
                    <td>{{ date(entry.occurred_at) }}</td>
                    <td>{{ entry.reason }}</td>
                    <td>{{ entry.actor_reference_masked }}</td>
                  </tr>
                } @empty {
                  <tr>
                    <td colspan="5">연결된 원장이 없습니다.</td>
                  </tr>
                }
              </tbody>
            </table>
          </div>
        </section>
        <section aria-labelledby="timeline-title">
          <h2 id="timeline-title">상태 타임라인</h2>
          <ol class="timeline">
            @for (event of tx.timeline; track event.id) {
              <li>
                <span aria-hidden="true">{{ timelineIcon(event.status) }}</span>
                <div>
                  <strong>{{ event.title }}</strong>
                  <p>{{ event.description }}</p>
                  <time>{{ date(event.occurred_at) }}</time>
                  @if (event.request_id) {
                    <code>{{ event.request_id }}</code>
                  }
                </div>
              </li>
            } @empty {
              <li>
                <span aria-hidden="true">·</span>
                <div><strong>기록 없음</strong></div>
              </li>
            }
          </ol>
        </section>
      </div>
      <nav class="pagination" aria-label="거래 목록 페이지">
        <coupon-button
          variant="secondary"
          [disabled]="page() <= 1"
          (click)="changePage(-1)"
          >이전</coupon-button
        ><span>{{ page() }}페이지</span
        ><coupon-button variant="secondary" (click)="changePage(1)"
          >다음</coupon-button
        >
      </nav>
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .search {
      display: grid;
      grid-template-columns: minmax(16rem, 2fr) 1fr 1fr auto;
      align-items: end;
      gap: 0.7rem;
      margin-bottom: 1rem;
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    label {
      display: grid;
      gap: 0.3rem;
      font-weight: 800;
    }
    label small {
      color: var(--coupon-color-text-muted);
      font-weight: 400;
    }
    input,
    select {
      min-height: 44px;
      padding: 0.6rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .privacy {
      display: flex;
      gap: 0.6rem;
      margin-bottom: 1rem;
      padding: 0.75rem;
      border-left: 4px solid var(--coupon-color-primary);
      background: var(--coupon-color-surface-muted);
    }
    .privacy p {
      margin: 0;
    }
    .summary {
      display: grid;
      gap: 1rem;
      padding: 1.25rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .summary h2 {
      margin: 0.6rem 0 0.2rem;
    }
    .summary code {
      overflow-wrap: anywhere;
    }
    .summary dl {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      margin: 0;
    }
    .summary dl div {
      padding: 0.6rem;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dt {
      color: var(--coupon-color-text-muted);
    }
    dd {
      margin: 0;
    }
    .content-grid {
      display: grid;
      gap: 1.25rem;
      margin-top: 1.25rem;
    }
    .content-grid h2 {
      font-size: 1.2rem;
    }
    .table-wrap {
      overflow-x: auto;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    table {
      width: 100%;
      min-width: 780px;
      border-collapse: collapse;
    }
    th,
    td {
      padding: 0.7rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    .timeline {
      display: grid;
      gap: 0;
      margin: 0;
      padding: 1rem;
      list-style: none;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .timeline li {
      display: grid;
      grid-template-columns: 2rem 1fr;
      gap: 0.7rem;
      min-height: 6rem;
    }
    .timeline li > span {
      display: grid;
      place-items: center;
      align-self: start;
      width: 2rem;
      height: 2rem;
      border: 2px solid var(--coupon-color-primary);
      border-radius: 50%;
      color: var(--coupon-color-primary);
      font-weight: 900;
    }
    .timeline li:not(:last-child) > span:after {
      content: "";
      position: absolute;
      height: 4rem;
      border-left: 2px solid var(--coupon-color-border);
      transform: translateY(3rem);
    }
    .timeline p {
      margin: 0.2rem 0;
      color: var(--coupon-color-text-muted);
    }
    .timeline time,
    .timeline code {
      display: block;
      font-size: 0.85rem;
    }
    .pagination {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 1rem;
      margin-top: 1rem;
    }
    .sr-only {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0, 0, 0, 0);
    }
    @media (max-width: 1100px) {
      .search {
        grid-template-columns: 1fr 1fr;
      }
      .transaction-id {
        grid-column: 1/-1;
      }
    }
    @media (min-width: 1280px) {
      .content-grid {
        grid-template-columns: minmax(0, 1.6fr) minmax(22rem, 1fr);
      }
      .summary {
        grid-template-columns: 1fr 2fr;
      }
    }
  `,
})
export class AdminTransactionExplorerComponent implements OnInit {
  private readonly api = inject(AdminTransactionsApi);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly fb = inject(FormBuilder);
  private readonly destroyRef = inject(DestroyRef);
  readonly loading = signal(false);
  readonly searched = signal(false);
  readonly data = signal<AdminTransactionDetailDto | null>(null);
  readonly error = signal<string | null>(null);
  readonly requestId = signal<string | null>(null);
  readonly page = signal(1);
  readonly form = this.fb.nonNullable.group({
    transaction_id: [
      "",
      [
        Validators.required,
        Validators.pattern(
          /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
        ),
      ],
    ],
    type: [""],
    date: [""],
  });
  ngOnInit(): void {
    this.route.queryParamMap
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe((params) => {
        this.form.patchValue(
          {
            transaction_id: params.get("transaction_id") ?? "",
            type: params.get("type") ?? "",
            date: params.get("date") ?? "",
          },
          { emitEvent: false },
        );
        this.page.set(Math.max(1, Number(params.get("page")) || 1));
        if (this.form.controls.transaction_id.valid) {
          this.searched.set(true);
          this.load();
        }
      });
  }
  search(): void {
    if (this.form.invalid) {
      this.form.markAllAsTouched();
      return;
    }
    const v = this.form.getRawValue();
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: {
        transaction_id: v.transaction_id,
        type: v.type || null,
        date: v.date || null,
        page: 1,
      },
      queryParamsHandling: "merge",
    });
  }
  load(): void {
    const id = this.form.controls.transaction_id.value;
    if (!id) return;
    this.loading.set(true);
    this.error.set(null);
    this.data.set(null);
    this.api
      .load(id)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (data) => {
          this.data.set(data);
          this.requestId.set(data.request_id);
          this.loading.set(false);
        },
        error: (error: unknown) => {
          this.error.set("거래 ID를 확인하거나 접근 권한을 확인해 주세요.");
          this.requestId.set(
            typeof error === "object" && error !== null && "request_id" in error
              ? String((error as { request_id: unknown }).request_id)
              : null,
          );
          this.loading.set(false);
        },
      });
  }
  changePage(delta: number): void {
    const next = Math.max(1, this.page() + delta);
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: { page: next },
      queryParamsHandling: "merge",
    });
  }
  date(v: string): string {
    return formatKoreaDateTime(v);
  }
  won(v: number): string {
    return formatWon(v);
  }
  kindLabel(v: AdminLedgerKind): string {
    return { EARN: "적립", REDEEM: "사용", VOID: "취소", ADJUSTMENT: "보정" }[
      v
    ];
  }
  statusBadge(v: string): "success" | "warning" | "danger" | "neutral" {
    return v.includes("SUCCESS") || v.includes("COMPLETED")
      ? "success"
      : v.includes("FAIL") || v.includes("REJECT")
        ? "danger"
        : v.includes("PENDING")
          ? "warning"
          : "neutral";
  }
  timelineIcon(v: string): string {
    return v.includes("FAIL") ? "×" : v.includes("PENDING") ? "…" : "✓";
  }
}
