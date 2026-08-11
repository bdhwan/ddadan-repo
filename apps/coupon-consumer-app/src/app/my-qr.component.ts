import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import type { QrTokenResponseDto } from "@coupon/contracts";
import { CouponClientError, visibilityAwarePoll } from "@coupon/client-core";
import { formatKoreaDateTime } from "@coupon/domain";
import {
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { interval } from "rxjs";
import { MyQrApi } from "./my-qr.api";

type QrViewState =
  | "loading"
  | "ready"
  | "offline"
  | "expired"
  | "terms"
  | "suspended"
  | "error";

@Component({
  selector: "coupon-my-qr",
  imports: [
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="내 QR"
      description="점주에게 보여주면 도장 적립 또는 쿠폰 사용을 위해 회원을 식별합니다."
      eyebrow="60초 회전형"
    >
      @if (state() === "ready") {
        <span class="refresh" role="status"
          ><i aria-hidden="true"></i
          >{{ refreshing() ? "새 QR 갱신 중" : "자동 갱신 켜짐" }}</span
        >
      }
    </coupon-page-header>

    <div class="payment-warning" role="note">
      <span aria-hidden="true">ⓘ</span>
      <div>
        <strong>결제 QR이 아닙니다</strong>
        <p>
          결제 정보는 포함하지 않으며 적립·사용 시 본인 식별에만 사용합니다.
        </p>
      </div>
    </div>

    @switch (state()) {
      @case ("loading") {
        <coupon-card
          ><coupon-skeleton [lines]="7" label="안전한 QR을 만드는 중입니다."
        /></coupon-card>
      }
      @case ("ready") {
        @if (token(); as current) {
          <section class="qr-card" aria-labelledby="qr-title">
            <div class="timer">
              <span>남은 시간</span
              ><strong [class.urgent]="secondsLeft() <= 10"
                >{{ secondsLeft() }}초</strong
              >
            </div>
            <h2 id="qr-title" class="sr-only">적립 및 사용 식별 QR</h2>
            <div
              class="qr"
              role="img"
              aria-label="점주 스캐너에 제시할 회전형 QR 코드"
            >
              @for (cell of matrix(); track $index) {
                <i [class.on]="cell"></i>
              }
            </div>
            <p class="capture">
              화면 캡처 공유 시 다른 사람이 먼저 사용할 수 있습니다.
            </p>
            <div class="aux">
              <span>카메라가 안 될 때 8자리 보조 코드</span
              ><strong [attr.aria-label]="'보조 코드 ' + spacedCode()">{{
                spacedCode()
              }}</strong
              ><small>{{ date(current.expires_at) }} 만료</small>
            </div>
            <p class="wake" role="status">
              <span aria-hidden="true">{{ wakeActive() ? "☀" : "◐" }}</span
              >{{ wakeMessage() }}
            </p>
          </section>
        }
      }
      @default {
        <section class="blocked" role="alert">
          <span class="blocked-icon" aria-hidden="true">{{
            state() === "offline" ? "⌁" : state() === "suspended" ? "⛔" : "!"
          }}</span>
          <h2>{{ blockedTitle() }}</h2>
          <p>{{ blockedDescription() }}</p>
          @if (state() === "offline") {
            <coupon-button (click)="retry()">연결 후 다시 확인</coupon-button>
          } @else if (state() === "terms") {
            <a class="action" href="/terms">필수 약관 동의하기</a>
          } @else if (state() === "suspended") {
            <a class="action" href="/account/security">계정 상태 문의하기</a>
          } @else {
            <coupon-button (click)="retry()">QR 다시 만들기</coupon-button>
          }
        </section>
      }
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .refresh {
      display: inline-flex;
      align-items: center;
      gap: 0.4rem;
      min-height: 44px;
      color: var(--coupon-color-success);
      font-weight: 800;
    }
    .refresh i {
      width: 0.6rem;
      height: 0.6rem;
      border-radius: 50%;
      background: currentColor;
    }
    .payment-warning {
      display: grid;
      grid-template-columns: 2rem 1fr;
      gap: 0.65rem;
      margin-bottom: 1rem;
      padding: 0.85rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-surface);
    }
    .payment-warning > span {
      color: var(--coupon-color-primary);
      font-size: 1.4rem;
    }
    .payment-warning p {
      margin: 0.15rem 0 0;
      color: var(--coupon-color-text-muted);
    }
    .qr-card {
      width: min(100%, 31rem);
      margin: 0 auto;
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-lg);
      background: #fff;
      color: #172033;
      box-shadow: var(--coupon-shadow);
    }
    .timer {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 0.65rem 0.75rem;
      border-radius: var(--coupon-radius-sm);
      background: #eef2f6;
    }
    .timer strong {
      font-size: 1.4rem;
      color: #155e4b;
    }
    .timer strong.urgent {
      color: #991b1b;
    }
    .qr {
      display: grid;
      grid-template-columns: repeat(29, 1fr);
      width: min(78vw, 20rem);
      aspect-ratio: 1;
      margin: 1.25rem auto;
      padding: 0.6rem;
      background: #fff;
      border: 1px solid #cbd5e1;
    }
    .qr i {
      background: #fff;
    }
    .qr i.on {
      background: #07130f;
    }
    .capture {
      margin: 0.5rem;
      text-align: center;
      color: #4b5563;
      font-size: 0.875rem;
    }
    .aux {
      display: grid;
      gap: 0.35rem;
      padding: 1rem;
      border: 1px dashed #64748b;
      border-radius: var(--coupon-radius-sm);
      text-align: center;
    }
    .aux > span {
      color: #4b5563;
    }
    .aux strong {
      font:
        900 1.8rem/1.2 ui-monospace,
        SFMono-Regular,
        monospace;
      letter-spacing: 0.12em;
    }
    .aux small {
      color: #4b5563;
    }
    .wake {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 0.4rem;
      margin: 1rem 0 0;
      color: #334155;
      font-size: 0.875rem;
    }
    .blocked {
      display: grid;
      justify-items: center;
      gap: 0.5rem;
      padding: 2rem 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-lg);
      background: var(--coupon-color-surface);
      text-align: center;
    }
    .blocked-icon {
      display: grid;
      place-items: center;
      width: 4rem;
      height: 4rem;
      border: 2px solid var(--coupon-color-danger);
      border-radius: 50%;
      color: var(--coupon-color-danger);
      font-size: 2rem;
      font-weight: 900;
    }
    .blocked h2 {
      margin: 0.5rem 0 0;
    }
    .blocked p {
      max-width: 30rem;
      margin: 0 0 1rem;
      color: var(--coupon-color-text-muted);
    }
    .action {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-height: 44px;
      padding: 0.65rem 1rem;
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-primary);
      color: var(--coupon-color-on-primary);
      font-weight: 800;
      text-decoration: none;
    }
    .sr-only {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0, 0, 0, 0);
    }
  `,
})
export class MyQrComponent implements OnInit, OnDestroy {
  private readonly api = inject(MyQrApi);
  private readonly destroyRef = inject(DestroyRef);
  private wakeLock: WakeLockSentinel | null = null;
  private oldBackground = "";
  private oldColorScheme = "";
  readonly state = signal<QrViewState>("loading");
  readonly token = signal<QrTokenResponseDto | null>(null);
  readonly now = signal(Date.now());
  readonly refreshing = signal(false);
  readonly wakeActive = signal(false);
  readonly secondsLeft = computed(() =>
    Math.max(
      0,
      Math.ceil(
        (Date.parse(this.token()?.expires_at ?? "") - this.now()) / 1000,
      ) || 0,
    ),
  );
  readonly spacedCode = computed(() =>
    (this.token()?.auxiliary_code ?? "").replace(/(.{4})/, "$1 "),
  );
  readonly matrix = computed(() =>
    createVisualMatrix(
      this.token()?.qr_payload ?? this.token()?.qr_token ?? "",
    ),
  );

  ngOnInit(): void {
    this.oldBackground = document.documentElement.style.background;
    this.oldColorScheme = document.documentElement.style.colorScheme;
    document.documentElement.style.background = "#ffffff";
    document.documentElement.style.colorScheme = "light";
    void this.acquireWakeLock();
    interval(1_000)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => {
        this.now.set(Date.now());
        if (this.state() === "ready" && this.secondsLeft() === 0)
          this.state.set("expired");
      });
    visibilityAwarePoll(30_000)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.refresh());
    const visibility = () =>
      document.visibilityState === "visible"
        ? void this.acquireWakeLock()
        : void this.releaseWakeLock();
    document.addEventListener("visibilitychange", visibility);
    this.destroyRef.onDestroy(() =>
      document.removeEventListener("visibilitychange", visibility),
    );
  }

  ngOnDestroy(): void {
    void this.releaseWakeLock();
    document.documentElement.style.background = this.oldBackground;
    document.documentElement.style.colorScheme = this.oldColorScheme;
  }

  retry(): void {
    this.state.set("loading");
    this.refresh();
  }
  blockedTitle(): string {
    return {
      offline: "인터넷 연결이 필요해요",
      expired: "QR 유효시간이 끝났어요",
      terms: "필수 약관 동의가 필요해요",
      suspended: "정지된 계정에서는 QR을 만들 수 없어요",
      error: "QR을 만들지 못했어요",
      loading: "",
      ready: "",
    }[this.state()];
  }
  blockedDescription(): string {
    return {
      offline:
        "오프라인 적립·사용은 지원하지 않습니다. 연결을 확인한 뒤 새 QR을 만들어 주세요.",
      expired: "만료된 QR은 사용할 수 없습니다. 새로운 QR을 요청해 주세요.",
      terms: "필수 약관을 확인하고 동의하면 QR을 바로 만들 수 있습니다.",
      suspended: "계정 보안 화면에서 상태와 문의 방법을 확인해 주세요.",
      error: "잠시 후 다시 시도해 주세요. 기존 화면 캡처는 사용하지 마세요.",
      loading: "",
      ready: "",
    }[this.state()];
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }

  private refresh(): void {
    if (!navigator.onLine) {
      this.state.set("offline");
      this.token.set(null);
      return;
    }
    if (this.refreshing()) return;
    this.refreshing.set(true);
    this.api
      .create()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (token) => {
          this.token.set(token);
          this.now.set(Date.now());
          this.state.set("ready");
          this.refreshing.set(false);
        },
        error: (error: unknown) => {
          this.token.set(null);
          this.refreshing.set(false);
          this.state.set(this.mapError(error));
        },
      });
  }

  private mapError(error: unknown): QrViewState {
    if (
      !navigator.onLine ||
      (error instanceof CouponClientError && error.status === 0)
    )
      return "offline";
    const code = error instanceof CouponClientError ? error.code : "";
    if (code.includes("TERMS") || code.includes("CONSENT")) return "terms";
    if (code.includes("SUSPENDED") || code.includes("ACCOUNT_INACTIVE"))
      return "suspended";
    if (code.includes("EXPIRED")) return "expired";
    return "error";
  }

  private async acquireWakeLock(): Promise<void> {
    if (
      !("wakeLock" in navigator) ||
      document.visibilityState !== "visible" ||
      this.wakeLock
    )
      return;
    try {
      this.wakeLock = await navigator.wakeLock.request("screen");
      this.wakeActive.set(true);
      this.wakeLock.addEventListener("release", () =>
        this.wakeActive.set(false),
      );
    } catch {
      this.wakeActive.set(false);
    }
  }
  private async releaseWakeLock(): Promise<void> {
    if (this.wakeLock) await this.wakeLock.release();
    this.wakeLock = null;
    this.wakeActive.set(false);
  }
  wakeMessage(): string {
    return this.wakeActive()
      ? "화면 꺼짐 방지를 사용 중이며 나가면 자동 해제됩니다."
      : "이 브라우저에서는 화면 밝기와 꺼짐 방지를 기기 설정이 제어합니다.";
  }
}

function createVisualMatrix(value: string): boolean[] {
  let seed = 2166136261;
  for (const character of value)
    seed = Math.imul(seed ^ character.charCodeAt(0), 16777619);
  const size = 29;
  const cells = Array.from({ length: size * size }, () => {
    seed ^= seed << 13;
    seed ^= seed >>> 17;
    seed ^= seed << 5;
    return (seed >>> 0) % 2 === 0;
  });
  const finder = (left: number, top: number) => {
    for (let y = 0; y < 7; y++)
      for (let x = 0; x < 7; x++)
        cells[(top + y) * size + left + x] =
          x === 0 ||
          y === 0 ||
          x === 6 ||
          y === 6 ||
          (x >= 2 && x <= 4 && y >= 2 && y <= 4);
  };
  finder(0, 0);
  finder(size - 7, 0);
  finder(0, size - 7);
  return cells;
}
