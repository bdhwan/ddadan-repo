import { HttpClient, HttpHeaders } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  CampaignBenefitDto,
  CampaignImpactActionRequestDto,
  OwnerCampaignDto,
  OwnerCampaignListResponseDto,
  SaveCampaignRequestDto,
} from "@coupon/contracts";
import { map, type Observable } from "rxjs";

interface CampaignTransport {
  id: string;
  store_id: string;
  status: OwnerCampaignDto["status"];
  version_no: number;
  name: string;
  benefit: {
    benefit_type: "FIXED_AMOUNT" | "PERCENTAGE" | "FREE_ITEM";
    fixed_amount: number | null;
    percentage: number | null;
    maximum_discount_amount: number | null;
    free_item_ids: string[];
  };
  issue_mode: "DIRECT" | "FIRST_COME";
  total_quantity:
    | { mode: "LIMITED"; quantity: number }
    | { mode: "UNLIMITED"; operational_cap: number };
  per_user_quantity: number;
  per_business_day_quantity: number | null;
  issued_count: number;
  revoked_count: number;
  remaining_quantity: number;
  audience_size: number | null;
  issue_starts_at: string;
  issue_ends_at: string;
  usable_from: string | null;
  usable_until: string | null;
  created_at: string;
  updated_at: string;
  version: number;
}

interface CampaignPageTransport {
  items: CampaignTransport[];
  next_cursor: string | null;
  has_more: boolean;
}

@Injectable({ providedIn: "root" })
export class CampaignsApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/owner/campaigns";

  list(
    _version?: number,
    _updatedAt?: string,
  ): Observable<OwnerCampaignListResponseDto> {
    return this.http.get<ApiSuccessDto<CampaignPageTransport>>(this.base).pipe(
      map((response) => {
        const items = response.data.items.map((campaign) =>
          adaptCampaign(campaign, response.request_id),
        );
        return {
          items,
          next_cursor: response.data.next_cursor,
          request_id: response.request_id,
          version: Math.max(0, ...items.map((item) => item.version)),
          updated_at: items[0]?.updated_at ?? new Date(0).toISOString(),
        };
      }),
    );
  }

  create(
    payload: SaveCampaignRequestDto,
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    return this.http
      .post<
        ApiSuccessDto<CampaignTransport>
      >(this.base, createTransport(payload), { headers: idempotencyHeaders(idempotencyKey) })
      .pipe(
        map((response) => adaptCampaign(response.data, response.request_id)),
      );
  }

  publish(id: string, idempotencyKey: string): Observable<OwnerCampaignDto> {
    return this.http
      .post<
        ApiSuccessDto<CampaignTransport>
      >(`${this.base}/${id}/publish`, {}, { headers: idempotencyHeaders(idempotencyKey) })
      .pipe(
        map((response) => adaptCampaign(response.data, response.request_id)),
      );
  }

  action(
    id: string,
    action: "pause" | "resume" | "cancel",
    payload: CampaignImpactActionRequestDto,
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    const body =
      action === "resume"
        ? {}
        : action === "cancel"
          ? {
              reason: payload.reason,
              revoke_policy: payload.revoke_issued_coupons
                ? "REVOKE_UNUSED"
                : "KEEP_ISSUED",
            }
          : { reason: payload.reason };
    return this.http
      .post<
        ApiSuccessDto<CampaignTransport>
      >(`${this.base}/${id}/${action}`, body, { headers: idempotencyHeaders(idempotencyKey) })
      .pipe(
        map((response) => adaptCampaign(response.data, response.request_id)),
      );
  }

  updateQuantity(
    campaign: Pick<OwnerCampaignDto, "id" | "version">,
    totalQuantity: number | null,
    perUserQuantity: number,
    idempotencyKey: string,
  ): Observable<OwnerCampaignDto> {
    return this.http
      .patch<ApiSuccessDto<CampaignTransport>>(
        `${this.base}/${campaign.id}`,
        {
          total_quantity:
            totalQuantity === null
              ? { mode: "UNLIMITED", operational_cap: 1_000_000 }
              : { mode: "LIMITED", quantity: totalQuantity },
          per_user_quantity: perUserQuantity,
          version: campaign.version,
        },
        { headers: idempotencyHeaders(idempotencyKey) },
      )
      .pipe(
        map((response) => adaptCampaign(response.data, response.request_id)),
      );
  }
}

function adaptCampaign(
  campaign: CampaignTransport,
  requestId: string,
): OwnerCampaignDto {
  const totalQuantity =
    campaign.total_quantity.mode === "LIMITED"
      ? campaign.total_quantity.quantity
      : null;
  return {
    id: campaign.id,
    name: campaign.name,
    status: campaign.status,
    issuance_method: campaign.issue_mode,
    benefit: adaptBenefit(campaign.benefit),
    benefit_label: benefitLabel(campaign.benefit),
    issuance_starts_at: campaign.issue_starts_at,
    issuance_ends_at: campaign.issue_ends_at,
    usable_from: campaign.usable_from ?? campaign.issue_starts_at,
    usable_until: campaign.usable_until ?? campaign.issue_ends_at,
    total_quantity: totalQuantity,
    per_user_quantity: campaign.per_user_quantity,
    per_business_day_quantity: campaign.per_business_day_quantity,
    issued_count: campaign.issued_count,
    used_count: 0,
    snapshot_target_count: campaign.audience_size,
    processed_count: campaign.issued_count + campaign.revoked_count,
    failed_count: 0,
    immutable_fields: [
      "benefit",
      "issue_mode",
      "issue_starts_at",
      "usable_until",
    ],
    request_id: requestId,
    created_at: campaign.created_at,
    updated_at: campaign.updated_at,
    version: campaign.version,
  } as OwnerCampaignDto & { created_at: string };
}

function adaptBenefit(
  benefit: CampaignTransport["benefit"],
): CampaignBenefitDto {
  if (benefit.benefit_type === "FIXED_AMOUNT") {
    return {
      type: "FIXED",
      discount_amount: benefit.fixed_amount ?? 0,
      currency: "KRW",
    };
  }
  if (benefit.benefit_type === "PERCENTAGE") {
    return {
      type: "PERCENTAGE",
      percentage: benefit.percentage ?? 0,
      maximum_discount_amount: benefit.maximum_discount_amount ?? 0,
      currency: "KRW",
    };
  }
  return {
    type: "FREE_ITEM",
    eligible_catalog_item_ids: benefit.free_item_ids,
  };
}

function benefitLabel(benefit: CampaignTransport["benefit"]): string {
  if (benefit.benefit_type === "FIXED_AMOUNT")
    return `${(benefit.fixed_amount ?? 0).toLocaleString("ko-KR")}원 할인`;
  if (benefit.benefit_type === "PERCENTAGE")
    return `${benefit.percentage ?? 0}% 할인`;
  return "지정 품목 무료";
}

function createTransport(
  payload: SaveCampaignRequestDto,
): Record<string, unknown> {
  return {
    name: payload.name,
    customer_description: payload.name,
    benefit: transportBenefit(payload.benefit),
    minimum_order_amount: payload.minimum_order_amount.amount,
    eligible_item_ids: payload.eligible_catalog_item_ids,
    eligible_category_ids: [],
    excluded_item_ids: payload.excluded_catalog_item_ids,
    audience_type: {
      ALL_FAVORITES: "FAVORITE_CUSTOMERS",
      SEGMENT: "RECENT_VISITORS",
      SPECIFIC_CUSTOMERS: "SPECIFIC_USERS",
    }[payload.audience_type],
    audience_criteria:
      payload.audience_type === "SEGMENT"
        ? { recent_visit_days: 30 }
        : payload.audience_type === "SPECIFIC_CUSTOMERS"
          ? { user_ids: [] }
          : {},
    issue_mode:
      payload.issuance_method === "FIRST_COME" ? "FIRST_COME" : "DIRECT",
    total_quantity:
      payload.total_quantity === null
        ? { mode: "UNLIMITED", operational_cap: 1_000_000 }
        : { mode: "LIMITED", quantity: payload.total_quantity },
    per_user_quantity: payload.per_user_quantity,
    per_business_day_quantity: payload.per_business_day_quantity,
    issue_starts_at: payload.issuance_starts_at,
    issue_ends_at: payload.issuance_ends_at,
    usable_from: payload.usable_from,
    usable_until: payload.usable_until,
    notification_channels: [
      ...(payload.notify_in_app ? ["IN_APP"] : []),
      ...(payload.notify_push ? ["WEB_PUSH"] : []),
    ],
    restore_quantity_on_revoke: payload.restore_quantity_on_revoke,
  };
}

function transportBenefit(
  benefit: CampaignBenefitDto,
): Record<string, unknown> {
  if (benefit.type === "FIXED")
    return {
      benefit_type: "FIXED_AMOUNT",
      fixed_amount: benefit.discount_amount,
      free_item_ids: [],
    };
  if (benefit.type === "PERCENTAGE")
    return {
      benefit_type: "PERCENTAGE",
      percentage: benefit.percentage,
      maximum_discount_amount: benefit.maximum_discount_amount,
      free_item_ids: [],
    };
  return {
    benefit_type: "FREE_ITEM",
    free_item_ids: benefit.eligible_catalog_item_ids,
  };
}

function idempotencyHeaders(key: string): HttpHeaders {
  return new HttpHeaders({ "Idempotency-Key": key });
}
