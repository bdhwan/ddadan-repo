import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import type {
  AdminOperationsOverviewDto,
  ComponentStatusDto,
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
import { AdminPhaseFourApi } from "./admin-phase-four.api";

@Component({
  selector: "coupon-admin-operations-overview",
  imports: [
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
      title="운영 현황"
      description="API·DB·Redis·worker·알림 상태와 backlog·오류율을 한 곳에서 확인합니다."
      eyebrow="Operations"
      ><coupon-button variant="secondary" (click)="load()"
        >새로고침</coupon-button
      ></coupon-page-header
    >
    @if (loading() && !overview()) {
      <coupon-card
        ><coupon-skeleton [lines]="8" label="운영 상태를 확인하는 중입니다."
      /></coupon-card>
    } @else if (error() && !overview()) {
      <coupon-error-state
        title="운영 상태를 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (overview(); as data) {
      <section class="components" aria-label="구성요소 상태">
        @for (component of data.components; track component.name) {
          <coupon-card>
            <div>
              <h2>{{ componentLabel(component.name) }}</h2>
              <coupon-badge
                [status]="badge(component.status)"
                [label]="statusLabel(component.status)"
                >{{ statusLabel(component.status) }}</coupon-badge
              >
            </div>
            <p>{{ component.detail }}</p>
          </coupon-card>
        }
      </section>
      <section class="metrics" aria-label="운영 지표">
        <coupon-card
          ><span>전체 backlog</span
          ><strong>{{ data.backlog.toLocaleString("ko-KR") }}</strong
          ><small>대기 작업</small></coupon-card
        >
        <coupon-card
          ><span>알림 backlog</span
          ><strong>{{
            data.notification_backlog.toLocaleString("ko-KR")
          }}</strong
          ><small>발송 대기</small></coupon-card
        >
        <coupon-card
          ><span>5분 오류율</span
          ><strong>{{ (data.error_rate * 100).toFixed(2) }}%</strong
          ><small>API 요청</small></coupon-card
        >
      </section>
      <p class="checked" role="status">점검 시각 {{ date(data.checked_at) }}</p>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
    }
    .components,
    .metrics {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
      gap: 0.75rem;
    }
    coupon-card > div {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.75rem;
    }
    h2 {
      margin: 0;
      font-size: 1rem;
    }
    p,
    small,
    .checked {
      color: var(--coupon-color-text-muted);
    }
    .metrics coupon-card {
      display: grid;
      gap: 0.3rem;
      border-top: 5px solid var(--coupon-color-primary);
    }
    .metrics strong {
      font-size: 1.65rem;
    }
    .checked {
      text-align: right;
    }
  `,
})
export class AdminOperationsOverviewComponent implements OnInit {
  private readonly api = inject(AdminPhaseFourApi);
  readonly overview = signal<AdminOperationsOverviewDto | null>(null);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);

  ngOnInit(): void {
    this.load();
  }

  load(): void {
    this.loading.set(true);
    this.api.overview().subscribe({
      next: (overview) => {
        this.overview.set(overview);
        this.loading.set(false);
        this.error.set(null);
      },
      error: () => {
        this.loading.set(false);
        this.error.set("현재 운영 지표를 조회할 수 없습니다.");
      },
    });
  }

  badge(
    status: ComponentStatusDto["status"],
  ): "success" | "warning" | "danger" {
    return status === "HEALTHY"
      ? "success"
      : status === "DEGRADED"
        ? "warning"
        : "danger";
  }

  statusLabel(status: ComponentStatusDto["status"]): string {
    return { HEALTHY: "정상", DEGRADED: "기능 저하", DOWN: "중단" }[status];
  }

  componentLabel(name: ComponentStatusDto["name"]): string {
    return {
      API: "API",
      DB: "PostgreSQL",
      REDIS: "Redis",
      WORKER: "Worker",
      NOTIFICATIONS: "알림 채널",
    }[name];
  }

  date(value: string): string {
    return formatKoreaDateTime(value);
  }
}
