import type { Rfc3339Timestamp, Uuid } from "./phase-one.dto";

/** Every successful coupon API response uses this transport envelope. */
export interface ApiSuccessDto<T> {
  data: T;
  request_id: string;
  transaction_id?: Uuid;
}

export type NotificationCategory =
  | "TRANSACTION"
  | "BENEFIT"
  | "SECURITY"
  | "OPERATIONS";

export interface NotificationDto {
  id: Uuid;
  event_id: Uuid | null;
  category: NotificationCategory;
  type: string;
  title: string;
  body: string;
  read_at: Rfc3339Timestamp | null;
  created_at: Rfc3339Timestamp;
  version: number;
}

export type NotificationPageDto =
  import("./phase-one.dto").CursorPageDto<NotificationDto>;

export interface MarkNotificationsReadRequestDto {
  notification_ids: Uuid[];
  all: boolean;
  action: "MARK_READ" | "MARK_UNREAD" | "DISMISS";
}

export type ConsentScopeDto =
  | "TERMS_OF_SERVICE"
  | "PRIVACY_POLICY"
  | "LOCATION_BASED_SEARCH"
  | "TRANSACTIONAL_WEB_PUSH"
  | "KAKAO_INFORMATIONAL"
  | "MARKETING_ALL"
  | "MARKETING_STORE";

export interface ConsentStateDto {
  scope: ConsentScopeDto;
  store_id: Uuid | null;
  granted: boolean;
  required: boolean;
  document_version: string | null;
  decided_at: Rfc3339Timestamp | null;
}

export interface ConsentsDataDto {
  consents: ConsentStateDto[];
}

export interface ConsentChangeDto {
  scope: ConsentScopeDto;
  store_id: Uuid | null;
  action: "GRANTED" | "REVOKED";
  document_version: string | null;
  source: string;
}

export interface UpdateConsentsRequestDto {
  consents: ConsentChangeDto[];
}

export type BrowserPermissionState =
  | "granted"
  | "denied"
  | "default"
  | "unsupported";

export interface OwnerAnalyticsMetricDto {
  key: "EARNED" | "REWARDS" | "CAMPAIGNS" | "VOIDS" | "ADJUSTMENTS";
  label: string;
  value: number | null;
  aggregation_status: "READY" | "PENDING";
}

export interface OwnerAnalyticsBreakdownDto {
  label: string;
  value: number;
}

export interface OwnerAnalyticsDto {
  period_from: string;
  period_to: string;
  provisional_as_of: Rfc3339Timestamp;
  confirmed_through: string | null;
  minimum_group_size: number;
  observed_group_size: number;
  detail_suppressed: boolean;
  metrics: OwnerAnalyticsMetricDto[];
  breakdowns: OwnerAnalyticsBreakdownDto[];
}

export interface ComponentStatusDto {
  name: "API" | "DB" | "REDIS" | "WORKER" | "NOTIFICATIONS";
  status: "HEALTHY" | "DEGRADED" | "DOWN";
  detail: string;
}

export interface AdminOperationsOverviewDto {
  components: ComponentStatusDto[];
  backlog: number;
  notification_backlog: number;
  error_rate: number;
  checked_at: Rfc3339Timestamp;
}

export type StoreReviewStatusDto =
  | "PENDING"
  | "NEEDS_MORE_INFO"
  | "APPROVED"
  | "REJECTED";

export interface AdminStoreReviewDto {
  id: Uuid;
  store_id: Uuid;
  store_name: string;
  owner_name_masked: string;
  business_number_masked: string;
  submitted_at: Rfc3339Timestamp;
  status: StoreReviewStatusDto;
  evidence_count: number;
  duplicate_signals: string[];
  version: number;
}

export interface AdminMemberDto {
  id: Uuid;
  display_name_masked: string;
  identifier_masked: string;
  status: string;
  roles: string[];
  store_name: string | null;
  incident_count: number;
  version: number;
}

export interface AdminNotificationDeliveryDto {
  id: Uuid;
  template_code: string;
  template_version: string;
  channel: string;
  status: string;
  callback_status: string | null;
  recipient_masked: string;
  permanent_failure: boolean;
  created_at: Rfc3339Timestamp;
}

export interface AdminCaseDto {
  id: Uuid;
  category: string;
  status: string;
  subject_masked: string;
  evidence_count: number;
  party_message_count: number;
  resolution: string | null;
  requires_approval: boolean;
  updated_at: Rfc3339Timestamp;
}

export interface AdminAuditLogDto {
  id: Uuid;
  actor_masked: string;
  action: string;
  resource: string;
  reason: string | null;
  occurred_at: Rfc3339Timestamp;
  retention_locked: boolean;
}

export interface HighRiskReauthenticationDto {
  reason: string;
  impact_acknowledged: boolean;
}
