import { Injectable, signal } from "@angular/core";
import { Router } from "@angular/router";
import { SwPush } from "@angular/service-worker";

interface PushPayload {
  notification?: { title?: string };
  data?: { url?: string };
}

@Injectable({ providedIn: "root" })
export class PushMessagesService {
  readonly announcement = signal("");

  constructor(swPush: SwPush, router: Router) {
    swPush.messages.subscribe((message) => {
      const payload = message as PushPayload;
      this.announcement.set(
        payload.notification?.title
          ? `새 푸시 알림: ${payload.notification.title}`
          : "새 푸시 알림이 도착했습니다.",
      );
    });
    swPush.notificationClicks.subscribe(({ notification }) => {
      const data = notification.data as { url?: unknown } | undefined;
      const target =
        typeof data?.url === "string" ? data.url : "/notifications";
      if (target.startsWith("/")) {
        void router.navigateByUrl(target);
      }
    });
  }
}
