import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { RouterOutlet } from "@angular/router";
import { PushMessagesService } from "./push-messages.service";

@Component({
  selector: "coupon-consumer-root",
  imports: [RouterOutlet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<a class="skip-link" href="#main-content">본문으로 건너뛰기</a>
    <p class="push-announcement" role="status" aria-live="polite">
      {{ push.announcement() }}
    </p>
    <router-outlet />`,
  styles: `
    .push-announcement {
      position: fixed;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
    }
  `,
})
export class AppComponent {
  protected readonly push = inject(PushMessagesService);
}
