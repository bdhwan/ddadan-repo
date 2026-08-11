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
import type { AdminCampaignDto } from "@coupon/contracts";
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
  selector: "coupon-admin-campaigns",
  imports: [
    RouterLink,
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
      title="캠페인 운영"
      description="대상 스냅샷·처리·발급·사용 수를 구분하고 고위험 작업은 별도 재인증 화면에서 실행합니다."
      eyebrow="High-risk operations"
      ><coupon-button variant="secondary" (click)="load()"
        >새로고침</coupon-button
      ></coupon-page-header
    >
    @if (loading()) {
      <coupon-card
        ><coupon-skeleton
          [lines]="8"
          label="캠페인 운영 현황을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="캠페인 현황을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (items().length === 0) {
      <coupon-empty-state
        title="조회된 캠페인이 없습니다"
        description="필터를 바꾸거나 나중에 다시 확인하세요."
      />
    } @else {
      <div class="table-wrap">
        <table>
          <caption class="sr-only">
            운영 캠페인 목록
          </caption>
          <thead>
            <tr>
              <th scope="col">캠페인</th>
              <th scope="col">상태</th>
              <th scope="col">대상/처리</th>
              <th scope="col">발급/사용</th>
              <th scope="col">최종 갱신</th>
              <th scope="col">고위험 작업</th>
            </tr>
          </thead>
          <tbody>
            @for (campaign of items(); track campaign.id) {
              <tr>
                <td>
                  <strong>{{ campaign.name }}</strong
                  ><br /><span>{{ campaign.store_name }}</span>
                </td>
                <td>
                  <coupon-badge
                    [status]="
                      campaign.status === 'CANCELLED'
                        ? 'danger'
                        : campaign.status === 'PAUSED'
                          ? 'warning'
                          : 'neutral'
                    "
                    [label]="campaign.status"
                    >{{ campaign.status }}</coupon-badge
                  >
                </td>
                <td>
                  스냅샷
                  {{
                    campaign.snapshot_target_count === null
                      ? "확정 중"
                      : campaign.snapshot_target_count + "명"
                  }}<br />처리 {{ campaign.processed_count }}명
                </td>
                <td>
                  발급 {{ campaign.issued_count }}건<br />사용
                  {{ campaign.used_count }}건
                </td>
                <td>{{ date(campaign.updated_at) }}</td>
                <td>
                  <div class="actions">
                    <a
                      [routerLink]="[
                        '/campaigns',
                        campaign.id,
                        'emergency-action',
                      ]"
                      [queryParams]="actionParams(campaign, 'stop')"
                      >긴급 중단</a
                    ><a
                      [routerLink]="[
                        '/campaigns',
                        campaign.id,
                        'emergency-action',
                      ]"
                      [queryParams]="actionParams(campaign, 'revoke')"
                      >대량 회수</a
                    >
                  </div>
                </td>
              </tr>
            }
          </tbody>
        </table>
      </div>
    }
    <p class="risk-note" role="note">
      <strong>되돌림 가능성:</strong> 긴급 중단은 상태 검증 후 재개할 수 있지만,
      이미 완료된 회수는 자동 되돌림이 불가능합니다. 두 작업 모두 일반 확인
      모달이 아닌 재인증 화면을 사용합니다.
    </p>
  `,
  styles: `
    :host {
      display: block;
    }
    .table-wrap {
      overflow-x: auto;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    table {
      width: 100%;
      min-width: 980px;
      border-collapse: collapse;
    }
    th,
    td {
      padding: 0.75rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
      vertical-align: middle;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    td span {
      color: var(--coupon-color-text-muted);
    }
    .actions {
      display: flex;
      gap: 0.5rem;
    }
    .actions a {
      display: inline-grid;
      place-items: center;
      min-height: 44px;
      padding: 0.45rem 0.7rem;
      border: 1px solid var(--coupon-color-danger);
      border-radius: var(--coupon-radius-sm);
      color: var(--coupon-color-danger);
      font-weight: 800;
      text-decoration: none;
    }
    .risk-note {
      padding: 0.8rem;
      border-left: 4px solid var(--coupon-color-warning);
      background: var(--coupon-color-surface-muted);
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
export class AdminCampaignsComponent implements OnInit {
  private readonly api = inject(AdminOperationsApi);
  private readonly destroyRef = inject(DestroyRef);
  readonly items = signal<AdminCampaignDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  ngOnInit(): void {
    this.load();
  }
  load(): void {
    this.loading.set(true);
    this.api
      .campaigns()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (response) => {
          this.items.set(response.items);
          this.loading.set(false);
          this.error.set(null);
        },
        error: () => {
          this.error.set("운영 API 연결을 확인해 주세요.");
          this.loading.set(false);
        },
      });
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }
  actionParams(
    campaign: AdminCampaignDto,
    action: "stop" | "revoke",
  ): Record<string, string | number> {
    return {
      action,
      name: campaign.name,
      store: campaign.store_name,
      issued: campaign.issued_count,
      used: campaign.used_count,
      revoke_count: campaign.estimated_revoke_count,
      reversible: String(action === "stop" && campaign.reversible_after_stop),
    };
  }
}
