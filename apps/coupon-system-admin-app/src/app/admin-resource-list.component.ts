import {
  ChangeDetectionStrategy,
  Component,
  OnDestroy,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { FormsModule } from "@angular/forms";
import { ActivatedRoute, Router, RouterLink } from "@angular/router";
import type {
  AdminAuditLogDto,
  AdminCaseDto,
  AdminMemberDto,
  AdminNotificationDeliveryDto,
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
import {
  AdminPhaseFourApi,
  type AdminResourceKind,
  type AdminResourceRow,
} from "./admin-phase-four.api";
import {
  adminListQueryParams,
  normalizeAdminListQuery,
  type AdminListQuery,
} from "./admin-list-query";

const COPY: Record<AdminResourceKind, { title: string; description: string }> =
  {
    members: {
      title: "회원·상점",
      description:
        "상태·역할·제재·세션·관련 사건을 마스킹한 상태로 조회합니다.",
    },
    notifications: {
      title: "알림 운영",
      description: "템플릿 버전·발송·provider callback·영구 실패를 확인합니다.",
    },
    cases: {
      title: "민원",
      description: "분류·증거·당사자 메시지·해결·승인 상태를 확인합니다.",
    },
    audit: {
      title: "감사 로그",
      description: "관리자 조회·변경 로그와 보존 잠금을 필터링합니다.",
    },
  };

@Component({
  selector: "coupon-admin-resource-list",
  imports: [
    FormsModule,
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
      [title]="copy.title"
      [description]="copy.description"
      eyebrow="Operations"
    >
      <coupon-button variant="secondary" (click)="load(query())"
        >새로고침</coupon-button
      >
    </coupon-page-header>
    <coupon-card>
      <form class="filters" (submit)="$event.preventDefault(); applyFilters()">
        <label
          >상태·유형<select name="filter" [(ngModel)]="draftFilter">
            @for (option of filterOptions(); track option.value) {
              <option [value]="option.value">{{ option.label }}</option>
            }
          </select></label
        >
        <label
          >검색<input
            name="search"
            type="search"
            [(ngModel)]="draftSearch"
            [placeholder]="searchPlaceholder()"
        /></label>
        <coupon-button type="submit">필터 적용</coupon-button>
      </form>
      <p class="masking">
        <strong>민감정보 마스킹 적용 중</strong> · 원문은 별도 권한·사유·감사
        로그 없이 노출되지 않습니다.
      </p>
    </coupon-card>

    @if (loading()) {
      <coupon-card
        ><coupon-skeleton [lines]="9" label="운영 목록을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="목록을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load(query())"
      />
    } @else if (rows().length === 0) {
      <coupon-empty-state
        title="조회된 항목이 없습니다"
        description="URL에 저장된 필터를 바꾸거나 다시 확인해 주세요."
      />
    } @else {
      <div class="table-wrap">
        <table>
          <caption class="sr-only">
            {{
              copy.title
            }}
            목록
          </caption>
          <thead>
            <tr>
              <th scope="col">대상</th>
              <th scope="col">상태</th>
              <th scope="col">버전·근거</th>
              <th scope="col">세부</th>
              <th scope="col">작업</th>
            </tr>
          </thead>
          <tbody>
            @for (row of rows(); track row.id) {
              <tr>
                <td>
                  <strong>{{ primary(row) }}</strong
                  ><br /><code>{{ row.id.slice(0, 8) }}…</code>
                </td>
                <td>
                  <coupon-badge [status]="badge(row)" [label]="status(row)">{{
                    status(row)
                  }}</coupon-badge>
                </td>
                <td>{{ evidence(row) }}</td>
                <td>{{ detail(row) }}</td>
                <td>
                  @if (action(row); as item) {
                    <a
                      class="action"
                      [routerLink]="['/high-risk-action']"
                      [queryParams]="item.query"
                      >{{ item.label }}</a
                    >
                  } @else {
                    <span>조회 전용</span>
                  }
                </td>
              </tr>
            }
          </tbody>
        </table>
      </div>
      <nav class="pagination" aria-label="목록 페이지">
        <coupon-button
          variant="secondary"
          [disabled]="query().page <= 1"
          (click)="goPage(query().page - 1)"
          >이전</coupon-button
        >
        <span>{{ query().page }} 페이지</span>
        <coupon-button
          variant="secondary"
          [disabled]="!hasMore()"
          (click)="goPage(query().page + 1)"
          >다음</coupon-button
        >
      </nav>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: 1rem;
    }
    .filters {
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
    select {
      min-height: 44px;
      padding: 0.55rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .masking {
      margin-bottom: 0;
      color: var(--coupon-color-text-muted);
    }
    .table-wrap {
      overflow-x: auto;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
    }
    table {
      width: 100%;
      min-width: 980px;
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
    .action {
      display: inline-flex;
      min-height: 44px;
      align-items: center;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    .pagination {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 1rem;
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
export class AdminResourceListComponent implements OnInit, OnDestroy {
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly api = inject(AdminPhaseFourApi);
  private subscription?: Subscription;

  readonly kind = (this.route.snapshot.data["kind"] ??
    "members") as AdminResourceKind;
  readonly copy = COPY[this.kind];
  readonly query = signal<AdminListQuery>({
    filter: "ALL",
    search: "",
    page: 1,
  });
  readonly rows = signal<AdminResourceRow[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly hasMore = signal(false);
  draftFilter = "ALL";
  draftSearch = "";

  ngOnInit(): void {
    this.subscription = this.route.queryParamMap.subscribe((params) => {
      const query = normalizeAdminListQuery({
        filter: params.get("filter"),
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
    this.api.resources(this.kind, query).subscribe({
      next: (page) => {
        this.rows.set(page.items);
        this.hasMore.set(page.has_more);
        this.loading.set(false);
        this.error.set(null);
      },
      error: () => {
        this.loading.set(false);
        this.error.set("필터 조건의 운영 목록을 조회할 수 없습니다.");
      },
    });
  }

  filterOptions(): Array<{ value: string; label: string }> {
    return this.kind === "members"
      ? [
          { value: "ALL", label: "전체" },
          { value: "ACTIVE", label: "활성" },
          { value: "SUSPENDED", label: "제재" },
        ]
      : this.kind === "notifications"
        ? [
            { value: "ALL", label: "전체" },
            { value: "DELIVERED", label: "발송 완료" },
            { value: "FAILED_PERMANENT", label: "영구 실패" },
          ]
        : this.kind === "cases"
          ? [
              { value: "ALL", label: "전체" },
              { value: "OPEN", label: "처리 중" },
              { value: "PENDING_APPROVAL", label: "승인 대기" },
              { value: "RESOLVED", label: "해결" },
            ]
          : [
              { value: "ALL", label: "전체" },
              { value: "READ", label: "조회" },
              { value: "CHANGE", label: "변경" },
              { value: "LOCKED", label: "보존 잠금" },
            ];
  }

  searchPlaceholder(): string {
    return this.kind === "audit" ? "관리자·자원·사유" : "마스킹 키·사건 ID";
  }

  primary(row: AdminResourceRow): string {
    if ("display_name_masked" in row)
      return `${row.display_name_masked} · ${row.identifier_masked}`;
    if ("template_code" in row)
      return `${row.template_code} v${row.template_version}`;
    if ("category" in row) return `${row.category} · ${row.subject_masked}`;
    return `${row.actor_masked} → ${row.resource}`;
  }

  status(row: AdminResourceRow): string {
    if ("permanent_failure" in row)
      return row.permanent_failure ? "FAILED_PERMANENT" : row.status;
    if ("retention_locked" in row)
      return row.retention_locked ? "LOCKED" : row.action;
    return row.status;
  }

  badge(row: AdminResourceRow): "success" | "warning" | "danger" | "neutral" {
    const status = this.status(row);
    if (/FAILED|SUSPENDED|REJECTED/.test(status)) return "danger";
    if (/PENDING|OPEN|LOCKED/.test(status)) return "warning";
    if (/ACTIVE|DELIVERED|RESOLVED/.test(status)) return "success";
    return "neutral";
  }

  evidence(row: AdminResourceRow): string {
    if ("roles" in row)
      return `역할 ${row.roles.join(", ")} · 관련 사건 ${row.incident_count}건`;
    if ("template_version" in row)
      return `템플릿 ${row.template_version} · ${row.channel}`;
    if ("evidence_count" in row)
      return `증거 ${row.evidence_count}건 · 메시지 ${row.party_message_count}건`;
    return row.reason ?? "사유 기록 없음";
  }

  detail(row: AdminResourceRow): string {
    if ("store_name" in row) return row.store_name ?? "상점 없음";
    if ("callback_status" in row)
      return `${row.recipient_masked} · callback ${row.callback_status ?? "대기"}`;
    if ("resolution" in row) return row.resolution ?? "해결 방식 미정";
    return `${formatKoreaDateTime(row.occurred_at)} · ${row.action}`;
  }

  action(
    row: AdminResourceRow,
  ): { label: string; query: Record<string, string> } | null {
    if (this.kind === "members" && "display_name_masked" in row) {
      return {
        label: "제재·세션 폐기",
        query: {
          title: "회원 제재·세션 폐기",
          target: row.display_name_masked,
          endpoint: `users/${row.id}/revoke-sessions`,
          reversible: "false",
        },
      };
    }
    if (
      this.kind === "cases" &&
      "requires_approval" in row &&
      row.requires_approval
    ) {
      return {
        label: "해결 승인",
        query: {
          title: "민원 해결 승인",
          target: row.subject_masked,
          endpoint: `cases/${row.id}/approve`,
          reversible: "false",
        },
      };
    }
    return null;
  }
}
