import { HttpClient, HttpParams } from "@angular/common/http";
import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import type {
  ConsumerNotificationDto,
  ConsumerNotificationListResponseDto,
} from "@coupon/contracts";
import { visibilityAwarePoll } from "@coupon/client-core";
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
@Component({
  selector: "coupon-notifications",
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
  template: `<coupon-page-header
      title="알림"
      description="거래·혜택·보안·운영 소식을 30초마다 확인합니다."
      eyebrow="앱 내 알림"
      ><coupon-button variant="secondary" (click)="load()"
        >새로고침</coupon-button
      ></coupon-page-header
    >
    @if (loading() && !version()) {
      <coupon-card
        ><coupon-skeleton [lines]="7" label="알림을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error() && !version()) {
      <coupon-error-state
        title="알림을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (items().length === 0) {
      <coupon-empty-state
        title="새 알림이 없어요"
        description="알림을 삭제하거나 읽어도 거래·쿠폰·도장 기록은 유지됩니다."
      />
    } @else {
      <ol class="list">
        @for (item of items(); track item.id) {
          <li [class.unread]="!item.read_at">
            <div>
              <coupon-badge
                [status]="item.category === 'SECURITY' ? 'warning' : 'neutral'"
                [label]="category(item.category)"
                >{{ category(item.category) }}</coupon-badge
              ><time>{{ date(item.created_at) }}</time>
            </div>
            <h2>{{ item.title }}</h2>
            <p>{{ item.body }}</p>
          </li>
        }
      </ol>
      <p class="synced" role="status">
        마지막 동기화 {{ updatedAt() ? date(updatedAt()!) : "확인 중" }}
      </p>
    }`,
  styles: `
    :host {
      display: block;
    }
    .list {
      display: grid;
      gap: 0.7rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .list li {
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .list li.unread {
      border-left: 5px solid var(--coupon-color-primary);
    }
    .list li > div {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.5rem;
    }
    .list time,
    .list p,
    .synced {
      color: var(--coupon-color-text-muted);
    }
    .list h2 {
      margin: 0.7rem 0 0.2rem;
      font-size: 1rem;
    }
    .list p {
      margin: 0;
    }
    .synced {
      text-align: right;
      font-size: 0.875rem;
    }
  `,
})
export class NotificationsComponent implements OnInit {
  private readonly http = inject(HttpClient);
  private readonly destroyRef = inject(DestroyRef);
  readonly items = signal<ConsumerNotificationDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly version = signal<number | null>(null);
  readonly updatedAt = signal<string | null>(null);
  ngOnInit(): void {
    visibilityAwarePoll(30_000)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.load());
  }
  load(): void {
    if (!navigator.onLine) {
      this.error.set("온라인 연결 후 알림을 동기화할 수 있습니다.");
      this.loading.set(false);
      return;
    }
    let params = new HttpParams();
    if (this.version() !== null)
      params = params.set("version", this.version()!);
    if (this.updatedAt()) params = params.set("updated_at", this.updatedAt()!);
    this.loading.set(true);
    this.http
      .get<ConsumerNotificationListResponseDto>(
        "/api/coupon/v1/me/notifications",
        { params },
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (r) => {
          this.items.set(
            [
              ...new Map(
                [...this.items(), ...r.items].map((i) => [i.id, i]),
              ).values(),
            ].sort(
              (a, b) => Date.parse(b.created_at) - Date.parse(a.created_at),
            ),
          );
          this.version.set(r.version);
          this.updatedAt.set(r.updated_at);
          this.loading.set(false);
          this.error.set(null);
        },
        error: () => {
          this.error.set("최신 알림을 가져오지 못했습니다.");
          this.loading.set(false);
        },
      });
  }
  date(v: string): string {
    return formatKoreaDateTime(v);
  }
  category(v: ConsumerNotificationDto["category"]): string {
    return {
      TRANSACTION: "거래",
      BENEFIT: "혜택",
      SECURITY: "보안",
      OPERATIONS: "운영",
    }[v];
  }
}
