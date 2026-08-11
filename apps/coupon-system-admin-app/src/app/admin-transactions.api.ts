import { HttpClient } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  AdminLedgerKind,
  AdminTransactionDetailDto,
  ApiSuccessDto,
} from "@coupon/contracts";
import { map, Observable } from "rxjs";

interface AdminTransactionTransport {
  transaction_id: string;
  store_id: string;
  store_name: string;
  customer_key: string;
  customer_masked_name: string;
  business_day: string;
  quantity: number;
  status: string;
  external_order_ref: string | null;
  confirmed_at: string;
  voided_at: string | null;
  version: number;
  ledger: Array<{
    id: string;
    event_type: string;
    quantity_delta: number;
    reason_code: string;
    actor_type: string;
    actor_user_id: string | null;
    occurred_at: string;
  }>;
  timeline: Array<{
    source: string;
    event: string;
    detail: unknown;
    occurred_at: string;
  }>;
}

@Injectable({ providedIn: "root" })
export class AdminTransactionsApi {
  private readonly http = inject(HttpClient);
  load(id: string): Observable<AdminTransactionDetailDto> {
    return this.http
      .get<
        ApiSuccessDto<AdminTransactionTransport>
      >(`/api/coupon/v1/admin/transactions/${encodeURIComponent(id)}`)
      .pipe(map((response) => adaptTransaction(response)));
  }
}

function adaptTransaction(
  response: ApiSuccessDto<AdminTransactionTransport>,
): AdminTransactionDetailDto {
  const transaction = response.data;
  return {
    transaction_id: transaction.transaction_id,
    transaction_type: transaction.voided_at ? "VOID" : "EARN",
    status: transaction.status,
    store_name: transaction.store_name,
    store_reference_masked: maskReference(transaction.store_id),
    customer_reference_masked:
      transaction.customer_masked_name ||
      maskReference(transaction.customer_key),
    external_order_ref_masked: transaction.external_order_ref
      ? maskReference(transaction.external_order_ref)
      : null,
    gross_amount: null,
    ledgers: transaction.ledger.map((entry) => ({
      id: entry.id,
      kind: ledgerKind(entry.event_type),
      amount: entry.quantity_delta,
      occurred_at: entry.occurred_at,
      reason: entry.reason_code,
      actor_reference_masked: entry.actor_user_id
        ? `${entry.actor_type} ${maskReference(entry.actor_user_id)}`
        : entry.actor_type,
    })),
    timeline: transaction.timeline.map((event, index) => ({
      id: `${transaction.transaction_id}-${index}`,
      status: event.event,
      title: event.event,
      description: describeDetail(event.source, event.detail),
      occurred_at: event.occurred_at,
      request_id: null,
    })),
    created_at: transaction.confirmed_at,
    updated_at: transaction.voided_at ?? transaction.confirmed_at,
    version: transaction.version,
    request_id: response.request_id,
  };
}

function ledgerKind(eventType: string): AdminLedgerKind {
  const normalized = eventType.toUpperCase();
  if (normalized.includes("VOID") || normalized.includes("CANCEL"))
    return "VOID";
  if (normalized.includes("REDEEM") || normalized.includes("SPEND")) {
    return "REDEEM";
  }
  if (normalized.includes("ADJUST")) return "ADJUSTMENT";
  return "EARN";
}

function maskReference(value: string): string {
  if (value.length <= 8) return `${value.slice(0, 2)}***`;
  return `${value.slice(0, 4)}…${value.slice(-4)}`;
}

function describeDetail(source: string, detail: unknown): string {
  if (detail === null || detail === undefined) return source;
  if (typeof detail === "string") return `${source} · ${detail}`;
  return `${source} · ${JSON.stringify(detail)}`;
}
