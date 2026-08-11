import { HttpClient } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type { AdminTransactionDetailDto } from "@coupon/contracts";
import { Observable } from "rxjs";
@Injectable({ providedIn: "root" })
export class AdminTransactionsApi {
  private readonly http = inject(HttpClient);
  load(id: string): Observable<AdminTransactionDetailDto> {
    return this.http.get<AdminTransactionDetailDto>(
      `/api/coupon/v1/admin/transactions/${encodeURIComponent(id)}`,
    );
  }
}
