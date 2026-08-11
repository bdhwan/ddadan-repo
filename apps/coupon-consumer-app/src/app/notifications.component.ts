import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import type { NotificationCategory, NotificationDto } from "@coupon/contracts";
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
import { NotificationsApi } from "./notifications.api";
import { mergeNotifications, optimisticRead } from "./notifications-state";

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
  template: `
    <coupon-page-header
      title="알림"
      description="거래·혜택·보안·운영 소식을 30초마다 확인합니다."
      eyebrow="앱 내 알림"
    >
      <coupon-button variant="secondary" (click)="load(true)">
        새로고침
      </coupon-button>
    </coupon-page-header>

    <coupon-card class="record-note">
      <strong>알림은 기록의 바로가기입니다.</strong>
      <p>알림을 지우거나 읽어도 거래·쿠폰·도장 기록은 지워지지 않습니다.</p>
    </coupon-card>

    @if (loading() && items().length === 0) {
      <coupon-card>
        <coupon-skeleton [lines]="7" label="알림을 불러오는 중입니다." />
      </coupon-card>
    } @else if (error() && items().length === 0) {
      <coupon-error-state
        title="알림을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load(true)"
      />
    } @else if (items().length === 0) {
      <coupon-empty-state
        title="새 알림이 없어요"
        description="적립·사용·보안 소식이 생기면 여기에 남겨 드릴게요."
      />
    } @else {
      <ol class="list" aria-label="알림 목록">
        @for (item of items(); track item.id) {
          <li [class.unread]="!item.read_at">
            <button
              type="button"
              class="notification"
              [attr.aria-label]="notificationLabel(item)"
              (click)="markRead(item)"
            >
              <span class="meta">
                <coupon-badge
                  [status]="
                    item.category === 'SECURITY' ? 'warning' : 'neutral'
                  "
                  [label]="category(item.category)"
                >
                  {{ category(item.category) }}
                </coupon-badge>
                <time [attr.datetime]="item.created_at">{{
                  date(item.created_at)
                }}</time>
              </span>
              <strong>{{ item.title }}</strong>
              <span class="body">{{ item.body }}</span>
              @if (!item.read_at) {
                <span class="unread-label">읽지 않음 · 선택하면 읽음 처리</span>
              }
            </button>
          </li>
        }
      </ol>
      @if (nextCursor()) {
        <coupon-button
          variant="secondary"
          [disabled]="loading()"
          (click)="load(false)"
        >
          더 보기
        </coupon-button>
      }
      <p class="synced" role="status" aria-live="polite">
        {{ statusMessage() }}
      </p>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
    }
    .record-note {
      border-left: 5px solid var(--coupon-color-primary);
    }
    .record-note p {
      margin: 0.35rem 0 0;
      color: var(--coupon-color-text-muted);
    }
    .list {
      display: grid;
      gap: 0.7rem;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .list li {
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
      overflow: hidden;
    }
    .list li.unread {
      border-left: 5px solid var(--coupon-color-primary);
    }
    .notification {
      display: grid;
      width: 100%;
      min-height: 44px;
      gap: 0.45rem;
      padding: 1rem;
      border: 0;
      background: transparent;
      color: inherit;
      text-align: left;
      cursor: pointer;
    }
    .meta {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.5rem;
    }
    time,
    .body,
    .synced {
      color: var(--coupon-color-text-muted);
    }
    .unread-label {
      color: var(--coupon-color-primary);
      font-size: 0.875rem;
      font-weight: 700;
    }
    .synced {
      text-align: right;
      font-size: 0.875rem;
    }
  `,
})
export class NotificationsComponent implements OnInit {
  private readonly api = inject(NotificationsApi);
  private readonly destroyRef = inject(DestroyRef);

  readonly items = signal<NotificationDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly nextCursor = signal<string | null>(null);
  readonly statusMessage = signal("알림을 확인하는 중입니다.");

  ngOnInit(): void {
    visibilityAwarePoll(30_000)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.load(true));
  }

  load(replace: boolean): void {
    if (!navigator.onLine) {
      this.error.set("온라인 연결 후 알림을 동기화할 수 있습니다.");
      this.loading.set(false);
      return;
    }
    this.loading.set(true);
    this.api
      .list(replace ? undefined : (this.nextCursor() ?? undefined))
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (page) => {
          this.items.set(
            replace ? page.items : mergeNotifications(this.items(), page.items),
          );
          this.nextCursor.set(page.next_cursor);
          this.loading.set(false);
          this.error.set(null);
          this.statusMessage.set("최신 알림과 동기화했습니다.");
        },
        error: () => {
          this.error.set("최신 알림을 가져오지 못했습니다.");
          this.statusMessage.set("알림 동기화에 실패했습니다.");
          this.loading.set(false);
        },
      });
  }

  markRead(item: NotificationDto): void {
    if (item.read_at) return;
    this.items.set(
      optimisticRead(this.items(), item.id, new Date().toISOString()),
    );
    this.statusMessage.set(`'${item.title}' 알림을 읽음으로 표시했습니다.`);
    this.api
      .markRead(item.id)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => {
          this.statusMessage.set("읽음 상태가 저장됐습니다.");
        },
        error: () => {
          this.statusMessage.set(
            "읽음 저장에 실패해 서버 상태로 다시 맞춥니다.",
          );
          this.load(true);
        },
      });
  }

  date(value: string): string {
    return formatKoreaDateTime(value);
  }

  category(value: NotificationCategory): string {
    return {
      TRANSACTION: "거래",
      BENEFIT: "혜택",
      SECURITY: "보안",
      OPERATIONS: "운영",
    }[value];
  }

  notificationLabel(item: NotificationDto): string {
    return `${this.category(item.category)} 알림, ${item.title}, ${item.read_at ? "읽음" : "읽지 않음"}`;
  }
}
