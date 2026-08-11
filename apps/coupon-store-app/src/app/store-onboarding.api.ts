import { HttpClient } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  BusinessHoursDto,
  OwnerStoreDto,
  OwnerStoreResponseDto,
  SaveOwnerStoreRequestDto,
  SubmitOwnerStoreReviewRequestDto,
  SubmitOwnerStoreReviewResponseDto,
} from "@coupon/contracts";
import { map, type Observable } from "rxjs";

interface StoreTransport {
  id: string;
  status: "DRAFT" | "PENDING_REVIEW" | "ACTIVE" | "SUSPENDED" | "CLOSED";
  name: string;
  slug: string;
  description: string | null;
  address: unknown;
  timezone: string;
  business_hours: unknown;
  business_profile_complete: boolean;
  latest_review: {
    status:
      | "PENDING"
      | "APPROVED"
      | "CHANGES_REQUESTED"
      | "REJECTED"
      | "CANCELLED";
    public_reason: string | null;
    submitted_at: string;
  } | null;
  created_at: string;
  updated_at: string;
  version: number;
}

@Injectable({ providedIn: "root" })
export class StoreOnboardingApi {
  private readonly http = inject(HttpClient);
  private readonly baseUrl = "/api/coupon/v1/owner/store";

  load(): Observable<OwnerStoreResponseDto> {
    return this.http
      .get<ApiSuccessDto<StoreTransport>>(this.baseUrl)
      .pipe(map(adaptStoreEnvelope));
  }

  create(payload: SaveOwnerStoreRequestDto): Observable<OwnerStoreResponseDto> {
    return this.http
      .post<ApiSuccessDto<StoreTransport>>(this.baseUrl, {
        name: payload.name ?? "새 상점",
        slug: slugFor(payload.name ?? "store"),
        description: payload.description ?? null,
        address: payload.address ?? {},
      })
      .pipe(map(adaptStoreEnvelope));
  }

  update(payload: SaveOwnerStoreRequestDto): Observable<OwnerStoreResponseDto> {
    const businessHours = payload.business_hours ?? [];
    return this.http
      .patch<ApiSuccessDto<StoreTransport>>(this.baseUrl, {
        name: payload.name,
        description: payload.description,
        address: payload.address ?? {},
        timezone: payload.timezone,
        business_hours: businessHours,
        business_profile:
          payload.business_registration_number && payload.representative_name
            ? {
                registration_no: payload.business_registration_number,
                representative_name: payload.representative_name,
                business_address: payload.address ?? null,
              }
            : undefined,
        version: payload.version,
      })
      .pipe(map(adaptStoreEnvelope));
  }

  submitReview(
    _payload: SubmitOwnerStoreReviewRequestDto,
  ): Observable<SubmitOwnerStoreReviewResponseDto> {
    return this.http
      .post<ApiSuccessDto<StoreTransport>>(`${this.baseUrl}/submit-review`, {
        note: null,
      })
      .pipe(map(adaptStoreEnvelope));
  }
}

function adaptStoreEnvelope(
  response: ApiSuccessDto<StoreTransport>,
): OwnerStoreResponseDto & SubmitOwnerStoreReviewResponseDto {
  return {
    store: adaptStore(response.data),
    request_id: response.request_id,
    transaction_id: response.transaction_id ?? response.request_id,
  };
}

function adaptStore(store: StoreTransport): OwnerStoreDto {
  const reviewStatusMap: Record<
    NonNullable<StoreTransport["latest_review"]>["status"],
    OwnerStoreDto["review_status"]
  > = {
    PENDING: "IN_REVIEW",
    APPROVED: "APPROVED",
    CHANGES_REQUESTED: "CHANGES_REQUESTED",
    REJECTED: "REJECTED",
    CANCELLED: "DRAFT",
  };
  const reviewStatus: OwnerStoreDto["review_status"] = store.latest_review
    ? reviewStatusMap[store.latest_review.status]
    : store.status === "ACTIVE"
      ? "APPROVED"
      : store.status === "SUSPENDED"
        ? "SUSPENDED"
        : "DRAFT";
  return {
    id: store.id,
    slug: store.slug,
    name: store.name,
    description: store.description,
    business_registration_number_masked: store.business_profile_complete
      ? "저장됨(마스킹)"
      : null,
    representative_name_masked: store.business_profile_complete
      ? "저장됨(마스킹)"
      : null,
    address: addressText(store.address),
    timezone: "Asia/Seoul",
    business_hours: businessHours(store.business_hours),
    review_status: reviewStatus,
    review_reason: store.latest_review?.public_reason ?? null,
    submitted_at: store.latest_review?.submitted_at ?? null,
    created_at: store.created_at,
    updated_at: store.updated_at,
    version: store.version,
  };
}

function addressText(address: unknown): string | null {
  if (typeof address === "string") return address || null;
  if (typeof address === "object" && address !== null) {
    const values = Object.values(address).filter(
      (value): value is string => typeof value === "string" && value.length > 0,
    );
    return values.join(" ") || null;
  }
  return null;
}

function businessHours(hours: unknown): BusinessHoursDto[] {
  return Array.isArray(hours) ? (hours as BusinessHoursDto[]) : [];
}

function slugFor(name: string): string {
  let hash = 0;
  for (const character of name)
    hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  return `store-${hash.toString(36)}`;
}
