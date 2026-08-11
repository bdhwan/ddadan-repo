import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormsModule } from "@angular/forms";
import type { AdminJobDto } from "@coupon/contracts";
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
import { AdminOperationsApi } from "./admin-operations.api";

@Component({
  selector: "coupon-admin-jobs",
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
      title="작업 큐"
      description="작업 키, 시도, 체크포인트, 최종 오류를 확인하고 사유를 남겨 재처리합니다."
      eyebrow="Workers"
      ><coupon-button variant="secondary" (click)="load()"
        >새로고침</coupon-button
      ></coupon-page-header
    >
    @if (loading()) {
      <coupon-card
        ><coupon-skeleton [lines]="9" label="작업 큐를 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="작업 큐를 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (items().length === 0) {
      <coupon-empty-state
        title="작업 큐가 비어 있습니다"
        description="실행 중이거나 실패한 작업이 없습니다."
      />
    } @else {
      <div class="table-wrap">
        <table>
          <caption class="sr-only">
            작업 큐 목록
          </caption>
          <thead>
            <tr>
              <th>작업 키</th>
              <th>유형/상태</th>
              <th>시도</th>
              <th>체크포인트</th>
              <th>오류</th>
              <th>재처리</th>
            </tr>
          </thead>
          <tbody>
            @for (job of items(); track job.id) {
              <tr>
                <td>
                  <code>{{ job.job_key }}</code
                  ><br /><small>{{ date(job.updated_at) }}</small>
                </td>
                <td>
                  {{ job.job_type }}<br /><coupon-badge
                    [status]="
                      job.status === 'FAILED'
                        ? 'danger'
                        : job.status === 'RUNNING'
                          ? 'success'
                          : 'neutral'
                    "
                    [label]="job.status"
                    >{{ job.status }}</coupon-badge
                  >
                </td>
                <td>{{ job.attempts }} / {{ job.max_attempts }}</td>
                <td>
                  <code>{{ job.checkpoint ?? "시작 전" }}</code>
                </td>
                <td class="job-error">{{ job.last_error ?? "없음" }}</td>
                <td>
                  <coupon-button
                    variant="secondary"
                    [disabled]="!job.retryable"
                    (click)="selectRetry(job)"
                    >재처리</coupon-button
                  >
                </td>
              </tr>
            }
          </tbody>
        </table>
      </div>
    }
    @if (retryTarget(); as job) {
      <section class="retry-panel" aria-labelledby="retry-title">
        <h2 id="retry-title">작업 재처리</h2>
        <p>
          <code>{{ job.job_key }}</code
          >의 체크포인트 <strong>{{ job.checkpoint ?? "시작 전" }}</strong
          >에서 안전하게 재개합니다.
        </p>
        <p class="risk">
          <strong>되돌림:</strong> 재처리 요청은 취소할 수 없지만, 작업 키와
          체크포인트가 중복 처리를 방지합니다.
        </p>
        <label
          >재처리 사유<textarea [(ngModel)]="retryReason" rows="3"></textarea>
        </label>
        @if (retryError()) {
          <p class="error" role="alert">{{ retryError() }}</p>
        }
        <div class="actions">
          <coupon-button variant="secondary" (click)="retryTarget.set(null)"
            >취소</coupon-button
          ><coupon-button
            [disabled]="retryReason.trim().length < 5 || retrying()"
            (click)="retry()"
            >{{ retrying() ? "요청 중…" : "재처리 요청" }}</coupon-button
          >
        </div>
      </section>
    }
  `,
  styles: `
    :host {
      display: block;
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
      padding: 0.7rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
      vertical-align: top;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    code {
      overflow-wrap: anywhere;
    }
    small {
      color: var(--coupon-color-text-muted);
    }
    .job-error {
      max-width: 24rem;
      white-space: pre-wrap;
    }
    .retry-panel {
      display: grid;
      gap: 0.8rem;
      margin-top: 1rem;
      padding: 1rem;
      border: 1px solid var(--coupon-color-warning);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .retry-panel h2,
    .retry-panel p {
      margin: 0;
    }
    .risk {
      padding: 0.7rem;
      background: var(--coupon-color-surface-muted);
    }
    label {
      display: grid;
      gap: 0.35rem;
      font-weight: 800;
    }
    textarea {
      min-height: 88px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .actions {
      display: flex;
      justify-content: flex-end;
      gap: 0.6rem;
    }
    .error {
      color: var(--coupon-color-danger);
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
export class AdminJobsComponent implements OnInit {
  private readonly api = inject(AdminOperationsApi);
  private readonly destroyRef = inject(DestroyRef);
  readonly items = signal<AdminJobDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly retryTarget = signal<AdminJobDto | null>(null);
  readonly retrying = signal(false);
  readonly retryError = signal<string | null>(null);
  retryReason = "";
  ngOnInit(): void {
    this.load();
  }
  load(): void {
    this.loading.set(true);
    this.api
      .jobs()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (response) => {
          this.items.set(response.items);
          this.loading.set(false);
          this.error.set(null);
        },
        error: () => {
          this.loading.set(false);
          this.error.set("작업 큐 API 연결을 확인해 주세요.");
        },
      });
  }
  selectRetry(job: AdminJobDto): void {
    this.retryTarget.set(job);
    this.retryReason = "";
    this.retryError.set(null);
  }
  retry(): void {
    const job = this.retryTarget();
    if (!job || this.retryReason.trim().length < 5 || this.retrying()) return;
    this.retrying.set(true);
    this.api
      .retryJob(job.id, { reason: this.retryReason.trim() }, createUuid())
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => {
          this.retryTarget.set(null);
          this.retrying.set(false);
          this.load();
        },
        error: () => {
          this.retryError.set(
            "재처리를 요청하지 못했습니다. 작업 상태와 retryable 여부를 다시 확인하세요.",
          );
          this.retrying.set(false);
        },
      });
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }
}

function createUuid(): string {
  return typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (character) => {
        const random = Math.floor(Math.random() * 16);
        return (character === "x" ? random : (random & 3) | 8).toString(16);
      });
}
