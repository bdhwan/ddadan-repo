import { HttpClient } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  CatalogItemDto,
  CatalogItemListResponseDto,
  SaveCatalogItemRequestDto,
} from "@coupon/contracts";
import { map, type Observable } from "rxjs";

interface CatalogItemTransport {
  id: string;
  name: string;
  sku: string | null;
  category_name: string | null;
  status: "ACTIVE" | "INACTIVE";
  reference_price: number | null;
  created_at: string;
  updated_at: string;
  version: number;
}

@Injectable({ providedIn: "root" })
export class CatalogApi {
  private readonly http = inject(HttpClient);
  private readonly base = "/api/coupon/v1/owner/catalog/items";

  list(): Observable<CatalogItemListResponseDto> {
    return this.http
      .get<ApiSuccessDto<{ items: CatalogItemTransport[] }>>(this.base, {
        params: { include_inactive: true },
      })
      .pipe(
        map((response) => ({
          items: response.data.items.map(adaptItem),
          request_id: response.request_id,
          version: Math.max(
            0,
            ...response.data.items.map((item) => item.version),
          ),
          updated_at:
            response.data.items[0]?.updated_at ?? new Date(0).toISOString(),
        })),
      );
  }

  create(payload: SaveCatalogItemRequestDto): Observable<CatalogItemDto> {
    return this.http
      .post<
        ApiSuccessDto<CatalogItemTransport>
      >(this.base, transportPayload(payload))
      .pipe(map((response) => adaptItem(response.data)));
  }

  update(
    id: string,
    payload: SaveCatalogItemRequestDto,
  ): Observable<CatalogItemDto> {
    return this.http
      .patch<
        ApiSuccessDto<CatalogItemTransport>
      >(`${this.base}/${id}`, transportPayload(payload))
      .pipe(map((response) => adaptItem(response.data)));
  }
}

function adaptItem(item: CatalogItemTransport): CatalogItemDto {
  return {
    id: item.id,
    name: item.name,
    sku: item.sku,
    category: item.category_name ?? "미분류",
    active: item.status === "ACTIVE",
    reference_price:
      item.reference_price === null
        ? null
        : { amount: item.reference_price, currency: "KRW" },
    created_at: item.created_at,
    updated_at: item.updated_at,
    version: item.version,
  } as CatalogItemDto & { created_at: string };
}

function transportPayload(
  payload: SaveCatalogItemRequestDto,
): Record<string, unknown> {
  return {
    name: payload.name,
    sku: payload.sku,
    reference_price: payload.reference_price?.amount ?? null,
    status: payload.active ? "ACTIVE" : "INACTIVE",
    version: payload.version,
  };
}
