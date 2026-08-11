import { HttpClient, HttpHeaders } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  CancelRedemptionRequestDto,
  CatalogItemListResponseDto,
  ConfirmRedemptionRequestDto,
  CreateStampTransactionRequestDto,
  OwnerDashboardResponseDto,
  RedemptionPreviewRequestDto,
  RedemptionPreviewResponseDto,
  RedemptionResponseDto,
  ResolvedCustomerDto,
  ScanResolveRequestDto,
  StampPreviewRequestDto,
  StampPreviewResponseDto,
  StampTransactionResponseDto,
} from "@coupon/contracts";
import { map, type Observable } from "rxjs";
import { CatalogApi } from "./catalog.api";

interface CustomerTransport {
  alias: string;
  masked_name: string;
}

interface ScanResolutionTransport {
  customer: CustomerTransport;
  qr_expires_at: string;
  policy: {
    version_no: number;
    reward_title: string;
  };
  stamp_board: { available: number };
}

interface StampPreviewTransport {
  preview_id: string;
  customer: CustomerTransport;
  expected_stamps: number;
  stamp_board_after: { available: number };
  stamps_expire_at: string;
  policy: { reward_title: string };
  warnings: Array<{ code: string; message: string }>;
  blockers: Array<{ code: string; message: string }>;
}

interface StampTransactionTransport {
  transaction_id: string;
  quantity: number;
  stamp_board: { available: number };
  issued_rewards: Array<{ title?: string }>;
  confirmed_at: string;
}

interface ReservationTransport {
  reservation_id: string;
  coupon_id: string;
  coupon_title: string;
  expected_discount_amount: number;
  payable_amount: number;
  reserved_at: string;
  expires_at: string;
}

interface RedemptionTransport {
  redemption_id: string;
  coupon_id: string;
  discount_amount: number;
  payable_amount: number;
  status: "CONFIRMED" | "VOIDED" | "REQUIRES_ADMIN_REVIEW";
  confirmed_at: string;
  voided_at: string | null;
  voidable_until: string | null;
}

@Injectable({ providedIn: "root" })
export class StoreOperationsApi {
  private readonly http = inject(HttpClient);
  private readonly catalogApi = inject(CatalogApi);
  private readonly base = "/api/coupon/v1/owner";
  private readonly ownerSessionId = createUuid();

  resolve(payload: ScanResolveRequestDto): Observable<ResolvedCustomerDto> {
    return this.http
      .post<
        ApiSuccessDto<ScanResolutionTransport>
      >(`${this.base}/scan/resolve`, scanPayload(payload))
      .pipe(
        map((response) => ({
          scan_session_id: credentialValue(payload),
          customer_reference_masked: response.data.customer.alias,
          display_name_masked: response.data.customer.masked_name,
          available_stamp_count: response.data.stamp_board.available,
          qr_expires_at: response.data.qr_expires_at,
          policy_version: response.data.policy.version_no,
          request_id: response.request_id,
        })),
      );
  }

  preview(
    payload: StampPreviewRequestDto,
  ): Observable<StampPreviewResponseDto> {
    return this.http
      .post<ApiSuccessDto<StampPreviewTransport>>(
        `${this.base}/stamp-transactions/preview`,
        {
          ...credentialPayload(payload.scan_session_id),
          order: payload.order,
        },
      )
      .pipe(
        map((response) => ({
          preview_id: response.data.preview_id,
          customer_reference_masked: response.data.customer.alias,
          display_name_masked: response.data.customer.masked_name,
          expected_stamp_count: response.data.expected_stamps,
          balance_after: response.data.stamp_board_after.available,
          stamp_expires_at: response.data.stamps_expire_at,
          reward_description: response.data.policy.reward_title,
          limits: response.data.blockers.map((issue) => issue.message),
          duplicate_warning:
            response.data.warnings.map((issue) => issue.message).join(" ") ||
            null,
          request_id: response.request_id,
        })),
      );
  }

  submit(
    payload: CreateStampTransactionRequestDto,
    idempotencyKey: string,
  ): Observable<StampTransactionResponseDto> {
    return this.http
      .post<ApiSuccessDto<StampTransactionTransport>>(
        `${this.base}/stamp-transactions`,
        {
          ...scanPayload(payload),
          preview_id: payload.preview_id,
          order: payload.order,
        },
        { headers: idempotencyHeaders(idempotencyKey) },
      )
      .pipe(
        map((response) => ({
          transaction_id: response.data.transaction_id,
          stamp_count: response.data.quantity,
          balance_after: response.data.stamp_board.available,
          reward_issued: response.data.issued_rewards.length > 0,
          reward_description: response.data.issued_rewards[0]?.title ?? null,
          processed_at: response.data.confirmed_at,
          request_id: response.request_id,
        })),
      );
  }

  previewRedemption(
    payload: RedemptionPreviewRequestDto,
    idempotencyKey: string,
  ): Observable<RedemptionPreviewResponseDto> {
    return this.http
      .post<ApiSuccessDto<ReservationTransport>>(
        `${this.base}/redemptions/preview`,
        {
          ...credentialPayload(payload.scan_session_id),
          coupon_id: payload.coupon_id,
          owner_session_id: this.ownerSessionId,
          order: payload.order,
        },
        { headers: idempotencyHeaders(idempotencyKey) },
      )
      .pipe(
        map((response) => ({
          redemption_id: response.data.reservation_id,
          coupon_id: response.data.coupon_id,
          coupon_inquiry_reference: response.data.coupon_id
            .slice(0, 8)
            .toUpperCase(),
          customer_reference_masked: "스캔 고객",
          display_name_masked: "마스킹됨",
          benefit_label: response.data.coupon_title,
          expected_discount_amount: response.data.expected_discount_amount,
          payable_amount: response.data.payable_amount,
          currency: "KRW",
          conditions: [],
          reserved_at: response.data.reserved_at,
          reservation_expires_at: response.data.expires_at,
          request_id: response.request_id,
        })),
      );
  }

  confirmRedemption(
    reservationId: string,
    payload: ConfirmRedemptionRequestDto,
    idempotencyKey: string,
  ): Observable<RedemptionResponseDto> {
    return this.http
      .post<
        ApiSuccessDto<RedemptionTransport>
      >(`${this.base}/redemptions/${reservationId}/confirm`, { order: payload.order, owner_session_id: this.ownerSessionId }, { headers: idempotencyHeaders(idempotencyKey) })
      .pipe(map(adaptRedemption));
  }

  cancelRedemption(
    redemptionId: string,
    payload: CancelRedemptionRequestDto,
    idempotencyKey: string,
  ): Observable<RedemptionResponseDto> {
    return this.http
      .post<ApiSuccessDto<RedemptionTransport>>(
        `${this.base}/redemptions/${redemptionId}/cancel`,
        {
          reason: payload.reason,
          restore_coupon: payload.restore_if_eligible,
        },
        { headers: idempotencyHeaders(idempotencyKey) },
      )
      .pipe(map(adaptRedemption));
  }

  catalog(): Observable<CatalogItemListResponseDto> {
    return this.catalogApi.list();
  }

  dashboard(
    _version?: number,
    _updatedAt?: string,
  ): Observable<OwnerDashboardResponseDto> {
    return this.http
      .get<ApiSuccessDto<OwnerDashboardResponseDto>>(`${this.base}/analytics`, {
        params: { scope: "today" },
      })
      .pipe(map((response) => response.data));
  }
}

function adaptRedemption(
  response: ApiSuccessDto<RedemptionTransport>,
): RedemptionResponseDto {
  return {
    transaction_id: response.transaction_id ?? response.data.redemption_id,
    redemption_id: response.data.redemption_id,
    coupon_id: response.data.coupon_id,
    discount_amount: response.data.discount_amount,
    payable_amount: response.data.payable_amount,
    currency: "KRW",
    status: response.data.status === "CONFIRMED" ? "USED" : "CANCELLED",
    processed_at: response.data.voided_at ?? response.data.confirmed_at,
    cancellable_until: response.data.voidable_until,
    request_id: response.request_id,
  };
}

function scanPayload(payload: { qr_token?: string; auxiliary_code?: string }): {
  qr_token: string | null;
  fallback_code: string | null;
} {
  return {
    qr_token: payload.qr_token ?? null,
    fallback_code: payload.auxiliary_code ?? null,
  };
}

function credentialValue(payload: ScanResolveRequestDto): string {
  return payload.qr_token
    ? `qr:${payload.qr_token}`
    : `fallback:${payload.auxiliary_code ?? ""}`;
}

function credentialPayload(value: string): {
  qr_token: string | null;
  fallback_code: string | null;
} {
  return value.startsWith("fallback:")
    ? { qr_token: null, fallback_code: value.slice("fallback:".length) }
    : {
        qr_token: value.startsWith("qr:") ? value.slice(3) : value,
        fallback_code: null,
      };
}

function idempotencyHeaders(key: string): HttpHeaders {
  return new HttpHeaders({ "Idempotency-Key": key });
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
