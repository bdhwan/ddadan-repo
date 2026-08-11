import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { FormsModule } from "@angular/forms";
import type {
  OwnerAnalyticsDto,
  OwnerAnalyticsMetricDto,
} from "@coupon/contracts";
import { formatKoreaDateTime } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { AnalyticsApi } from "./analytics.api";
import { canShowAnalyticsDetail, metricDisplay } from "./analytics-state";

@Component({
  selector: "coupon-analytics",
  imports: [
    FormsModule,
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="통계"
      description="실시간 잠정치와 일 배치 확정치를 구분해 봅니다."
      eyebrow="Analytics"
    />
    <coupon-card>
      <form (submit)="$event.preventDefault(); load()">
        <label
          >시작일<input type="date" name="from" [(ngModel)]="from"
        /></label>
        <label>종료일<input type="date" name="to" [(ngModel)]="to" /></label>
        <coupon-button type="submit" [disabled]="loading()"
          >기간 적용</coupon-button
        >
      </form>
      <p class="mvp-note">CSV 개인 목록 내보내기는 MVP에 포함되지 않습니다.</p>
    </coupon-card>

    @if (loading() && !analytics()) {
      <coupon-card
        ><coupon-skeleton [lines]="8" label="통계를 집계하는 중입니다."
      /></coupon-card>
    } @else if (error() && !analytics()) {
      <coupon-error-state
        title="통계를 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (analytics(); as data) {
      <div class="legend" aria-label="집계 기준 범례">
        <coupon-badge status="warning" label="실시간 잠정치"
          >실시간 잠정치</coupon-badge
        >
        <span
          >오늘 {{ dateTime(data.provisional_as_of) }}까지 계속 변할 수
          있습니다.</span
        >
        <coupon-badge status="success" label="일 배치 확정치"
          >일 배치 확정치</coupon-badge
        >
        <span
          >{{ data.confirmed_through ?? "첫 배치 집계 중" }}까지
          확정됐습니다.</span
        >
      </div>
      <section class="metrics" aria-label="기간 지표">
        @for (metric of data.metrics; track metric.key) {
          <coupon-card
            [class.pending]="metric.aggregation_status === 'PENDING'"
          >
            <span>{{ metric.label }}</span>
            <strong>{{ display(metric) }}</strong>
            <small>{{
              metric.aggregation_status === "PENDING"
                ? "배치 준비 중 · 0이 아님"
                : "확인 가능"
            }}</small>
          </coupon-card>
        }
      </section>
      <section aria-labelledby="breakdown-heading">
        <h2 id="breakdown-heading">세부 구분</h2>
        @if (showDetail(data)) {
          <ul class="breakdown">
            @for (item of data.breakdowns; track item.label) {
              <li>
                <span>{{ item.label }}</span
                ><strong>{{ item.value.toLocaleString("ko-KR") }}</strong>
              </li>
            }
          </ul>
        } @else {
          <coupon-card class="privacy-notice" role="note">
            <strong>세부 구분을 숨겼습니다.</strong>
            <p>
              집단 크기 {{ data.observed_group_size }}명이 개인정보 최소 기준
              {{ data.minimum_group_size }}명보다 작아 개별 분류를 보여주지
              않습니다.
            </p>
          </coupon-card>
        }
      </section>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
    }
    form,
    .legend {
      display: flex;
      flex-wrap: wrap;
      align-items: end;
      gap: 1rem;
    }
    label {
      display: grid;
      gap: 0.35rem;
      font-weight: 700;
    }
    input {
      min-height: 44px;
      padding: 0.55rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .mvp-note,
    .legend span,
    small,
    .privacy-notice p {
      color: var(--coupon-color-text-muted);
    }
    .metrics {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
      gap: 0.75rem;
    }
    .metrics coupon-card {
      display: grid;
      gap: 0.35rem;
      border-top: 5px solid var(--coupon-color-success);
    }
    .metrics coupon-card.pending {
      border-top-color: var(--coupon-color-warning);
    }
    .metrics strong {
      font-size: 1.55rem;
    }
    .breakdown {
      display: grid;
      gap: 0.5rem;
      padding: 0;
      list-style: none;
    }
    .breakdown li {
      display: flex;
      justify-content: space-between;
      min-height: 44px;
      padding: 0.75rem;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    .privacy-notice {
      border-left: 5px solid var(--coupon-color-warning);
    }
  `,
})
export class AnalyticsComponent implements OnInit {
  private readonly api = inject(AnalyticsApi);

  readonly analytics = signal<OwnerAnalyticsDto | null>(null);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  from = dateInput(-30);
  to = dateInput(0);

  ngOnInit(): void {
    this.load();
  }

  load(): void {
    this.loading.set(true);
    this.api.load(this.from, this.to).subscribe({
      next: (analytics) => {
        this.analytics.set(analytics);
        this.loading.set(false);
        this.error.set(null);
      },
      error: () => {
        this.loading.set(false);
        this.error.set("해당 기간의 집계 상태를 확인하지 못했습니다.");
      },
    });
  }

  display(metric: OwnerAnalyticsMetricDto): string {
    return metricDisplay(metric);
  }

  showDetail(analytics: OwnerAnalyticsDto): boolean {
    return canShowAnalyticsDetail(analytics);
  }

  dateTime(value: string): string {
    return formatKoreaDateTime(value);
  }
}

function dateInput(offsetDays: number): string {
  const date = new Date();
  date.setDate(date.getDate() + offsetDays);
  return date.toISOString().slice(0, 10);
}
