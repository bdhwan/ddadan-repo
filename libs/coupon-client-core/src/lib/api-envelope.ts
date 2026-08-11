import type { ApiSuccessDto } from "@coupon/contracts";
import { map, type Observable } from "rxjs";

export interface ApiDataWithMeta<T> {
  data: T;
  request_id: string;
  transaction_id?: string;
}

export function apiData<T>(response: ApiSuccessDto<T>): ApiDataWithMeta<T> {
  return {
    data: response.data,
    request_id: response.request_id,
    ...(response.transaction_id
      ? { transaction_id: response.transaction_id }
      : {}),
  };
}

export function mapApiData<T>() {
  return (source: Observable<ApiSuccessDto<T>>): Observable<T> =>
    source.pipe(map((response) => response.data));
}
