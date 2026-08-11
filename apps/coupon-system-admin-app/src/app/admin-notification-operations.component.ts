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
  selector: "coupon-admin-notification-operations",
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
      title="알림 운영"
      description="발송 backlog와 provider 실패 신호를 지원되는 운영 지표로 확인합니다."
      eyebrow="Notifications"
    >
      <coupon-button variant="secondary" (click)="load()"
        >새로고침</coupon-button
      >
    </coupon-page-header>
    @if (loading() && !overview()) {
      <coupon-card>
        <coupon-skeleton
          [lines]="5"
          label="알림 운영 지표를 불러오는 중입니다."
        />
      </coupon-card>
    } @else if (error() && !overview()) {
      <coupon-error-state
        title="알림 운영 지표를 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (overview(); as data) {
      <section class="metrics" aria-label="알림 운영 지표">
        <coupon-card>
          <span>알림 채널 상태</span>
          <coupon-badge
            [status]="badge(notificationStatus(data)?.status)"
            [label]="statusLabel(notificationStatus(data)?.status)"
            >{{ statusLabel(notificationStatus(data)?.status) }}</coupon-badge
          >
          <p>{{ notificationStatus(data)?.detail ?? "상세 지표 확인 중" }}</p>
        </coupon-card>
        <coupon-card>
          <span>발송 backlog</span>
          <strong
            >{{ data.notification_backlog.toLocaleString("ko-KR") }}건</strong
          >
          <p>대기와 재시도 중인 알림 합계입니다.</p>
        </coupon-card>
      </section>
      <coupon-card>
        <h2>현재 제공 범위</h2>
        <p>
          서버에 없는 <code>GET /admin/notifications</code>는 호출하지 않습니다.
          템플릿별 발송·콜백 상세 목록은 해당 API가 정식 계약에 추가된 뒤
          활성화합니다.
        </p>
      </coupon-card>
      <p class="checked" role="status">점검 시각 {{ date(data.checked_at) }}</p>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
    }
    .metrics {
      display: grid;
      gap: 1rem;
    }
    .metrics coupon-card {
      display: grid;
      gap: 0.5rem;
      border-top: 5px solid var(--coupon-color-primary);
    }
    h2,
    p {
      margin: 0;
    }
    strong {
      font-size: 1.6rem;
    }
    p,
    .checked {
      color: var(--coupon-color-text-muted);
    }
    .checked {
      text-align: right;
    }
    @media (min-width: 768px) {
      .metrics {
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }
    }
  `,
})
export class AdminNotificationOperationsComponent implements OnInit {
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
        this.error.set("현재 알림 운영 지표를 조회할 수 없습니다.");
      },
    });
  }

  notificationStatus(
    overview: AdminOperationsOverviewDto,
  ): ComponentStatusDto | undefined {
    return overview.components.find((item) => item.name === "NOTIFICATIONS");
  }

  badge(
    status: ComponentStatusDto["status"] | undefined,
  ): "success" | "warning" | "danger" {
    return status === "HEALTHY"
      ? "success"
      : status === "DEGRADED"
        ? "warning"
        : "danger";
  }

  statusLabel(status: ComponentStatusDto["status"] | undefined): string {
    return status
      ? { HEALTHY: "정상", DEGRADED: "기능 저하", DOWN: "중단" }[status]
      : "확인 필요";
  }

  date(value: string): string {
    return formatKoreaDateTime(value);
  }
}
