import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { RouterLink } from "@angular/router";
import type {
  OwnerDashboardMetricDto,
  OwnerDashboardResponseDto,
} from "@coupon/contracts";
import { visibilityAwarePoll } from "@coupon/client-core";
import { formatKoreaDateTime } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponCardComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { StoreOperationsApi } from "./store-operations.api";

@Component({
  selector: "coupon-store-dashboard",
  imports: [
    RouterLink,
    CouponBadgeComponent,
    CouponCardComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="오늘"
      description="현장 거래와 발송 상태를 빠르게 확인하세요."
      eyebrow="운영 요약"
    >
      @if (data()) {
        <span class="updated">마지막 갱신 {{ date(data()!.updated_at) }}</span>
      }
    </coupon-page-header>
    <div class="quick">
      <a routerLink="/scan"
        ><span aria-hidden="true">▦</span><strong>QR 스캔</strong
        ><small>도장 적립 시작</small></a
      ><a routerLink="/loyalty"
        ><span aria-hidden="true">◆</span><strong>정책 확인</strong
        ><small>현재·예약 버전</small></a
      >
    </div>
    @if (loading() && !data()) {
      <coupon-card
        ><coupon-skeleton [lines]="7" label="오늘 현황을 집계하는 중입니다."
      /></coupon-card>
    } @else if (error() && !data()) {
      <coupon-error-state
        title="오늘 현황을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (data(); as dashboard) {
      <section class="metrics" aria-label="오늘 거래 수">
        <article>
          <span aria-hidden="true">＋</span>
          <p>오늘 적립</p>
          <strong>{{ metric(dashboard.earned) }}</strong>
        </article>
        <article>
          <span aria-hidden="true">−</span>
          <p>오늘 사용</p>
          <strong>{{ metric(dashboard.redeemed) }}</strong>
        </article>
        <article>
          <span aria-hidden="true">↩</span>
          <p>오늘 취소</p>
          <strong>{{ metric(dashboard.voided) }}</strong>
        </article>
        <article>
          <span aria-hidden="true">◎</span>
          <p>활성 캠페인</p>
          <strong>{{ dashboard.active_campaign_count }}개</strong>
        </article>
      </section>
      <section class="health" aria-labelledby="health-title">
        <h2 id="health-title">운영 상태</h2>
        <div class="health-grid">
          <coupon-card
            ><div class="health-row">
              <span class="health-icon" aria-hidden="true">{{
                healthIcon(dashboard.queue_health)
              }}</span>
              <div>
                <h3>작업 큐</h3>
                <p>{{ healthDescription("queue", dashboard.queue_health) }}</p>
              </div>
              <coupon-badge
                [status]="healthStatus(dashboard.queue_health)"
                [label]="healthLabel(dashboard.queue_health)"
                >{{ healthLabel(dashboard.queue_health) }}</coupon-badge
              >
            </div></coupon-card
          ><coupon-card
            ><div class="health-row">
              <span class="health-icon" aria-hidden="true">{{
                healthIcon(dashboard.delivery_health)
              }}</span>
              <div>
                <h3>알림 발송</h3>
                <p>
                  {{ healthDescription("delivery", dashboard.delivery_health) }}
                </p>
              </div>
              <coupon-badge
                [status]="healthStatus(dashboard.delivery_health)"
                [label]="healthLabel(dashboard.delivery_health)"
                >{{ healthLabel(dashboard.delivery_health) }}</coupon-badge
              >
            </div></coupon-card
          >
        </div>
      </section>
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .updated {
      display: inline-flex;
      align-items: center;
      min-height: 44px;
      color: var(--coupon-color-text-muted);
      font-size: 0.875rem;
    }
    .quick {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0.75rem;
      margin-bottom: 1.25rem;
    }
    .quick a {
      display: grid;
      grid-template-columns: 2.5rem 1fr;
      align-items: center;
      min-height: 72px;
      padding: 0.75rem;
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-primary);
      color: var(--coupon-color-on-primary);
      text-decoration: none;
    }
    .quick a > span {
      grid-row: 1/3;
      font-size: 1.7rem;
    }
    .quick small {
      grid-column: 2;
    }
    .metrics {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: 0.75rem;
    }
    .metrics article {
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .metrics article > span {
      color: var(--coupon-color-primary);
      font-size: 1.4rem;
    }
    .metrics p {
      margin: 0.5rem 0 0.1rem;
      color: var(--coupon-color-text-muted);
    }
    .metrics strong {
      font-size: 1.5rem;
    }
    .health {
      margin-top: 1.5rem;
    }
    .health-grid {
      display: grid;
      gap: 0.75rem;
    }
    .health-row {
      display: grid;
      grid-template-columns: 2.5rem 1fr auto;
      align-items: center;
      gap: 0.7rem;
    }
    .health-icon {
      font-size: 1.6rem;
    }
    .health h3 {
      margin: 0;
    }
    .health p {
      margin: 0.15rem 0;
      color: var(--coupon-color-text-muted);
    }
    @media (min-width: 768px) {
      .quick {
        grid-template-columns: repeat(2, minmax(14rem, 20rem));
      }
      .metrics {
        grid-template-columns: repeat(4, 1fr);
      }
      .health-grid {
        grid-template-columns: 1fr 1fr;
      }
    }
  `,
})
export class StoreDashboardComponent implements OnInit {
  private readonly api = inject(StoreOperationsApi);
  private readonly destroyRef = inject(DestroyRef);
  private inFlight = false;
  readonly data = signal<OwnerDashboardResponseDto | null>(null);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  ngOnInit(): void {
    visibilityAwarePoll(5_000)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.load());
  }
  load(): void {
    if (this.inFlight) return;
    this.inFlight = true;
    this.loading.set(true);
    this.error.set(null);
    this.api
      .dashboard(this.data()?.version, this.data()?.updated_at)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (data) => {
          this.data.set(data);
          this.loading.set(false);
          this.inFlight = false;
        },
        error: () => {
          this.error.set("서버 연결을 확인한 뒤 다시 시도해 주세요.");
          this.loading.set(false);
          this.inFlight = false;
        },
      });
  }
  metric(value: OwnerDashboardMetricDto): string {
    return value.aggregation_status === "PENDING" || value.value === null
      ? "집계 중"
      : `${value.value}건`;
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }
  healthLabel(value: OwnerDashboardResponseDto["queue_health"]): string {
    return { HEALTHY: "정상", DELAYED: "지연", ERROR: "이상" }[value];
  }
  healthIcon(value: OwnerDashboardResponseDto["queue_health"]): string {
    return value === "HEALTHY" ? "✓" : value === "DELAYED" ? "⌛" : "!";
  }
  healthStatus(
    value: OwnerDashboardResponseDto["queue_health"],
  ): "success" | "warning" | "danger" {
    return value === "HEALTHY"
      ? "success"
      : value === "DELAYED"
        ? "warning"
        : "danger";
  }
  healthDescription(
    kind: "queue" | "delivery",
    value: OwnerDashboardResponseDto["queue_health"],
  ): string {
    if (value === "HEALTHY")
      return kind === "queue"
        ? "대기 중인 비정상 작업이 없습니다."
        : "발송 실패율이 정상 범위입니다.";
    return value === "DELAYED"
      ? "일부 처리가 늦어지고 있습니다. 거래 결과에는 영향이 없습니다."
      : "운영 확인이 필요한 이상이 감지되었습니다.";
  }
}
