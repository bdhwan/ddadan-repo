import { HttpClient, HttpHeaders, HttpParams } from "@angular/common/http";
import { inject, Injectable } from "@angular/core";
import type {
  ApiSuccessDto,
  MarkNotificationsReadRequestDto,
  NotificationPageDto,
} from "@coupon/contracts";
import { map } from "rxjs";
import type { Observable } from "rxjs";

interface NotificationTransport {
  id: string;
  store_id: string | null;
  type: string;
  purpose: "TRANSACTIONAL" | "INFORMATIONAL" | "MARKETING" | "SECURITY";
  title: string;
  body: string;
  occurred_at: string;
  read_at: string | null;
}

interface NotificationPageTransport {
  items: NotificationTransport[];
  next_cursor: string | null;
  has_more: boolean;
}

@Injectable({ providedIn: "root" })
export class NotificationsApi {
  private readonly http = inject(HttpClient);
  private readonly endpoint = "/api/coupon/v1/me/notifications";

  list(cursor?: string): Observable<NotificationPageDto> {
    const params = cursor
      ? new HttpParams().set("cursor", cursor)
      : new HttpParams();
    return this.http
      .get<ApiSuccessDto<NotificationPageTransport>>(this.endpoint, { params })
      .pipe(
        map((response) => ({
          items: response.data.items.map((item) => ({
            id: item.id,
            event_id: null,
            category: notificationCategory(item.purpose),
            type: item.type,
            title: item.title,
            body: item.body,
            read_at: item.read_at,
            created_at: item.occurred_at,
            version: 0,
          })),
          next_cursor: response.data.next_cursor,
          has_more: response.data.has_more,
        })),
      );
  }

  markRead(notificationId: string): Observable<void> {
    const payload: MarkNotificationsReadRequestDto = {
      notification_ids: [notificationId],
      all: false,
      action: "MARK_READ",
    };
    return this.http
      .patch<ApiSuccessDto<unknown>>(this.endpoint, payload, {
        headers: new HttpHeaders({ "Idempotency-Key": createUuid() }),
      })
      .pipe(map(() => undefined));
  }
}

function notificationCategory(
  purpose: NotificationTransport["purpose"],
): "TRANSACTION" | "BENEFIT" | "SECURITY" | "OPERATIONS" {
  if (purpose === "TRANSACTIONAL") return "TRANSACTION";
  if (purpose === "MARKETING") return "BENEFIT";
  if (purpose === "SECURITY") return "SECURITY";
  return "OPERATIONS";
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
