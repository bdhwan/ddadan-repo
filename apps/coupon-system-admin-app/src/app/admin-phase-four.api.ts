import { HttpClient, HttpHeaders, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  AdminAuditLogDto,
  AdminCaseDto,
  AdminMemberDto,
  AdminNotificationDeliveryDto,
  AdminOperationsOverviewDto,
  AdminStoreReviewDto,
  ApiSuccessDto,
  CursorPageDto,
  StoreReviewStatusDto,
} from "@coupon/contracts";
import { map, type Observable } from "rxjs";
import type { AdminListQuery } from "./admin-list-query";

export type AdminResourceKind = "members" | "notifications" | "cases" | "audit";
export type AdminResourceRow =
  | AdminMemberDto
  | AdminNotificationDeliveryDto
  | AdminCaseDto
  | AdminAuditLogDto;

interface OperationalMetricsTransport {
  process: {
    requests: number;
    error_rate: number;
    latency_p95_ms: number | null;
  };
  queues: {
    campaign_backlog: number;
    outbox_unpublished_age_secs: number;
    outbox_unpublished_count: number;
    dead_letters_total: number;
    stalled_jobs: number;
  };
  notifications: {
    pending_deliveries: number;
    retrying_deliveries: number;
    oldest_pending_age_secs: number;
    provider_failure_rate_1h: number;
    permanent_failures_1h: number;
  };
}

interface StoreReviewTransport {
  id: string;
  store_id: string;
  store_name: string;
  store_status: string;
  status:
    | "PENDING"
    | "APPROVED"
    | "CHANGES_REQUESTED"
    | "REJECTED"
    | "CANCELLED";
  submission_snapshot: Record<string, unknown>;
  submitted_at: string;
  reviewer_user_id: string | null;
  decided_at: string | null;
  public_reason: string | null;
  created_at: string;
}

interface AdminCaseTransport {
  id: string;
  case_number: number;
  case_type: string;
  status: string;
  title: string;
  description: string;
  subject_user_id: string | null;
  subject_store_id: string | null;
  resolution_type: string | null;
  public_resolution: string | null;
  internal_resolution: string | null;
  updated_at: string;
  version: number;
}

interface AuditLogTransport {
  id: string;
  actor_type: string;
  actor_user_id: string | null;
  action: string;
  resource_type: string;
  resource_id: string | null;
  reason: string | null;
  occurred_at: string;
  chain_intact: boolean;
}

interface PageTransport<T> {
  items: T[];
  next_cursor: string | null;
  has_more: boolean;
}

@Injectable({ providedIn: "root" })
export class AdminPhaseFourApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/admin";
  private readonly cursors = new Map<string, string>();

  overview(): Observable<AdminOperationsOverviewDto> {
    return this.http
      .get<ApiSuccessDto<OperationalMetricsTransport>>(`${this.base}/metrics`)
      .pipe(map((response) => adaptOverview(response.data)));
  }

  storeReviews(
    query: AdminListQuery,
  ): Observable<CursorPageDto<AdminStoreReviewDto>> {
    const cursor = this.cursorFor("reviews", query);
    return this.http
      .get<
        ApiSuccessDto<PageTransport<StoreReviewTransport>>
      >(`${this.base}/store-reviews`, { params: listParams(query, cursor, true) })
      .pipe(
        map((response) => {
          this.rememberCursor("reviews", query, response.data.next_cursor);
          return {
            items: response.data.items.map(adaptStoreReview),
            next_cursor: response.data.next_cursor,
            has_more: response.data.has_more,
          };
        }),
      );
  }

  decideStoreReview(
    reviewId: string,
    decision: StoreReviewStatusDto,
    reason: string,
  ): Observable<AdminStoreReviewDto> {
    const transportDecision =
      decision === "NEEDS_MORE_INFO" ? "CHANGES_REQUESTED" : decision;
    return this.http
      .post<ApiSuccessDto<StoreReviewTransport>>(
        `${this.base}/store-reviews/${encodeURIComponent(reviewId)}/decision`,
        {
          decision: transportDecision,
          public_reason:
            decision === "NEEDS_MORE_INFO" || decision === "REJECTED"
              ? reason
              : null,
          reason,
        },
        { headers: idempotencyHeaders() },
      )
      .pipe(map((response) => adaptStoreReview(response.data)));
  }

  resources(
    kind: AdminResourceKind,
    query: AdminListQuery,
  ): Observable<CursorPageDto<AdminResourceRow>> {
    if (kind === "members" || kind === "notifications") {
      return this.http.get<never>(
        `${this.base}/${kind === "members" ? "members" : "notifications"}`,
        { params: listParams(query, undefined, true) },
      );
    }

    const cursor = this.cursorFor(kind, query);
    if (kind === "cases") {
      return this.http
        .get<
          ApiSuccessDto<PageTransport<AdminCaseTransport>>
        >(`${this.base}/cases`, { params: listParams(query, cursor, true) })
        .pipe(
          map((response) => {
            this.rememberCursor(kind, query, response.data.next_cursor);
            return {
              items: response.data.items.map(adaptCase),
              next_cursor: response.data.next_cursor,
              has_more: response.data.has_more,
            };
          }),
        );
    }

    return this.http
      .get<
        ApiSuccessDto<PageTransport<AuditLogTransport>>
      >(`${this.base}/audit-logs`, { params: listParams(query, cursor, false) })
      .pipe(
        map((response) => {
          this.rememberCursor(kind, query, response.data.next_cursor);
          return {
            items: response.data.items.map(adaptAudit),
            next_cursor: response.data.next_cursor,
            has_more: response.data.has_more,
          };
        }),
      );
  }

  highRiskAction(
    endpoint: string,
    reason: string,
  ): Observable<{ status: string }> {
    const safeEndpoint = endpoint
      .split("/")
      .filter((segment) => /^[a-z0-9_-]+$/i.test(segment))
      .join("/");
    return this.http
      .post<
        ApiSuccessDto<unknown>
      >(`${this.base}/${safeEndpoint}`, { reason, case_id: null }, { headers: idempotencyHeaders() })
      .pipe(map(() => ({ status: "ACCEPTED" })));
  }

  private cursorFor(kind: string, query: AdminListQuery): string | undefined {
    if (query.page <= 1) return undefined;
    return this.cursors.get(cursorKey(kind, query, query.page));
  }

  private rememberCursor(
    kind: string,
    query: AdminListQuery,
    cursor: string | null,
  ): void {
    if (cursor) {
      this.cursors.set(cursorKey(kind, query, query.page + 1), cursor);
    }
  }
}

function adaptOverview(
  metrics: OperationalMetricsTransport,
): AdminOperationsOverviewDto {
  const workerStatus = metrics.queues.stalled_jobs > 0 ? "DEGRADED" : "HEALTHY";
  const notificationStatus =
    metrics.notifications.provider_failure_rate_1h >= 0.1 ||
    metrics.notifications.permanent_failures_1h > 0
      ? "DEGRADED"
      : "HEALTHY";
  const backlog =
    metrics.queues.campaign_backlog + metrics.queues.outbox_unpublished_count;
  return {
    components: [
      {
        name: "API",
        status: metrics.process.error_rate >= 0.05 ? "DEGRADED" : "HEALTHY",
        detail: `요청 ${metrics.process.requests}건 · p95 ${metrics.process.latency_p95_ms ?? "측정 중"}ms`,
      },
      {
        name: "DB",
        status: "HEALTHY",
        detail: "운영 지표 쿼리 응답 정상",
      },
      {
        name: "REDIS",
        status: "HEALTHY",
        detail: "API readiness 및 worker lease 대상",
      },
      {
        name: "WORKER",
        status: workerStatus,
        detail: `stalled ${metrics.queues.stalled_jobs}건 · outbox 최장 ${metrics.queues.outbox_unpublished_age_secs}초`,
      },
      {
        name: "NOTIFICATIONS",
        status: notificationStatus,
        detail: `provider 실패율 ${(metrics.notifications.provider_failure_rate_1h * 100).toFixed(2)}%`,
      },
    ],
    backlog,
    notification_backlog:
      metrics.notifications.pending_deliveries +
      metrics.notifications.retrying_deliveries,
    error_rate: metrics.process.error_rate,
    checked_at: new Date().toISOString(),
  };
}

function adaptStoreReview(review: StoreReviewTransport): AdminStoreReviewDto {
  const snapshot = review.submission_snapshot;
  return {
    id: review.id,
    store_id: review.store_id,
    store_name: review.store_name,
    owner_name_masked: maskedSnapshotValue(snapshot, [
      "owner_name",
      "representative_name",
    ]),
    business_number_masked: maskedSnapshotValue(snapshot, [
      "business_number",
      "registration_number",
    ]),
    submitted_at: review.submitted_at,
    status:
      review.status === "CHANGES_REQUESTED"
        ? "NEEDS_MORE_INFO"
        : review.status === "CANCELLED"
          ? "REJECTED"
          : review.status,
    evidence_count: collectionSize(
      snapshot["evidence"] ?? snapshot["documents"],
    ),
    duplicate_signals: stringCollection(snapshot["duplicate_signals"]),
    version: 0,
  };
}

function adaptCase(item: AdminCaseTransport): AdminCaseDto {
  const subject =
    item.subject_user_id ?? item.subject_store_id ?? "대상 미지정";
  return {
    id: item.id,
    category: item.case_type,
    status: item.status,
    subject_masked: maskReference(subject),
    evidence_count: 0,
    party_message_count: item.description ? 1 : 0,
    resolution: item.public_resolution ?? item.resolution_type,
    requires_approval: item.status === "PENDING_APPROVAL",
    updated_at: item.updated_at,
  };
}

function adaptAudit(item: AuditLogTransport): AdminAuditLogDto {
  return {
    id: item.id,
    actor_masked: item.actor_user_id
      ? `${item.actor_type} ${maskReference(item.actor_user_id)}`
      : item.actor_type,
    action: item.action,
    resource: item.resource_id
      ? `${item.resource_type} ${maskReference(item.resource_id)}`
      : item.resource_type,
    reason: item.reason,
    occurred_at: item.occurred_at,
    retention_locked: !item.chain_intact,
  };
}

function listParams(
  query: AdminListQuery,
  cursor: string | undefined,
  supportsStatus: boolean,
): HttpParams {
  let params = new HttpParams();
  if (supportsStatus && query.filter !== "ALL") {
    const status =
      query.filter === "NEEDS_MORE_INFO" ? "CHANGES_REQUESTED" : query.filter;
    params = params.set("status", status);
  }
  if (cursor) params = params.set("cursor", cursor);
  return params;
}

function cursorKey(kind: string, query: AdminListQuery, page: number): string {
  return `${kind}:${query.filter}:${query.search}:${page}`;
}

function idempotencyHeaders(): HttpHeaders {
  return new HttpHeaders({ "Idempotency-Key": createUuid() });
}

function maskedSnapshotValue(
  snapshot: Record<string, unknown>,
  keys: string[],
): string {
  const value = keys
    .map((key) => snapshot[key])
    .find((item) => typeof item === "string");
  return typeof value === "string" ? maskReference(value) : "마스킹 정보 없음";
}

function collectionSize(value: unknown): number {
  return Array.isArray(value) ? value.length : 0;
}

function stringCollection(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

function maskReference(value: string): string {
  if (value.length <= 8) return `${value.slice(0, 2)}***`;
  return `${value.slice(0, 4)}…${value.slice(-4)}`;
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
