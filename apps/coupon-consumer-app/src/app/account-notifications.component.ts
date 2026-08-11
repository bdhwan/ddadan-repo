import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import type {
  BrowserPermissionState,
  ConsentScopeDto,
  ConsentStateDto,
} from "@coupon/contracts";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { AccountApi } from "./account.api";
import {
  currentBrowserPermission,
  optimisticConsent,
  permissionCopy,
} from "./notification-settings-state";

const LABELS: Record<ConsentScopeDto, string> = {
  TERMS_OF_SERVICE: "서비스 이용약관",
  PRIVACY_POLICY: "개인정보 수집·이용",
  LOCATION_BASED_SEARCH: "위치 기반 상점 검색",
  TRANSACTIONAL_WEB_PUSH: "거래·보안 Web Push",
  KAKAO_INFORMATIONAL: "카카오 정보성 알림",
  MARKETING_ALL: "전체 마케팅",
  MARKETING_STORE: "상점별 마케팅",
};

@Component({
  selector: "coupon-account-notifications",
  imports: [
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <coupon-page-header
        title="알림 설정"
        description="앱 내 필수 알림과 외부 채널 동의를 따로 관리합니다."
        eyebrow="Account"
      />

      <coupon-card class="permission">
        <div>
          <h2>브라우저 푸시 권한</h2>
          <coupon-badge
            [status]="
              permission() === 'granted'
                ? 'success'
                : permission() === 'denied'
                  ? 'danger'
                  : 'warning'
            "
            [label]="permissionLabel()"
            >{{ permissionLabel() }}</coupon-badge
          >
        </div>
        <p>{{ permissionDescription() }}</p>
        @if (permission() === "default") {
          <coupon-button (click)="requestPermission()"
            >푸시 권한 요청</coupon-button
          >
        }
      </coupon-card>

      @if (loading()) {
        <coupon-card
          ><coupon-skeleton [lines]="6" label="동의 상태를 확인하는 중입니다."
        /></coupon-card>
      } @else if (error()) {
        <coupon-error-state
          title="동의 상태를 불러오지 못했어요"
          [description]="error()!"
          [retryable]="true"
          (retry)="load()"
        />
      } @else {
        <section aria-labelledby="channel-heading">
          <h2 id="channel-heading">목적·채널별 동의</h2>
          <p>
            거래·보안 앱 내 알림은 끄지 않으며, 푸시와 알림톡만 선택할 수
            있습니다.
          </p>
          <div class="settings">
            @for (consent of generalConsents(); track consent.scope) {
              <coupon-card>
                <label>
                  <span>
                    <strong>{{ label(consent.scope) }}</strong>
                    <small
                      >{{ consent.required ? "필수" : "선택" }} ·
                      {{ channel(consent.scope) }}</small
                    >
                  </span>
                  <input
                    type="checkbox"
                    [checked]="consent.granted"
                    [disabled]="consent.required || saving()"
                    (change)="toggle(consent, $any($event.target).checked)"
                  />
                </label>
              </coupon-card>
            }
          </div>
        </section>

        <section aria-labelledby="store-heading">
          <h2 id="store-heading">상점별 마케팅</h2>
          @if (storeConsents().length === 0) {
            <coupon-card
              >관심 상점의 마케팅 동의가 없습니다. 관심 상점을 추가하면 여기서
              개별로 철회할 수 있습니다.</coupon-card
            >
          } @else {
            <div class="settings">
              @for (consent of storeConsents(); track consent.store_id) {
                <coupon-card>
                  <label>
                    <span
                      ><strong>상점 {{ maskedStore(consent.store_id) }}</strong
                      ><small>마케팅 푸시·알림톡</small></span
                    >
                    <input
                      type="checkbox"
                      [checked]="consent.granted"
                      [disabled]="saving()"
                      (change)="toggle(consent, $any($event.target).checked)"
                    />
                  </label>
                </coupon-card>
              }
            </div>
          }
        </section>
      }

      <p class="status" role="status" aria-live="polite">{{ status() }}</p>
    </main>
  `,
  styles: `
    main {
      display: grid;
      width: min(100% - 2rem, 52rem);
      gap: 1rem;
      margin: 0 auto;
      padding: 2rem 0;
    }
    .permission > div,
    label {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 1rem;
    }
    .permission h2,
    section h2 {
      margin: 0;
    }
    .permission p,
    section > p,
    small,
    .status {
      color: var(--coupon-color-text-muted);
    }
    .settings {
      display: grid;
      gap: 0.65rem;
    }
    label {
      min-height: 44px;
      cursor: pointer;
    }
    label > span {
      display: grid;
    }
    input[type="checkbox"] {
      width: 2rem;
      height: 2rem;
      accent-color: var(--coupon-color-primary);
    }
    .status {
      min-height: 1.5rem;
    }
  `,
})
export class AccountNotificationsComponent implements OnInit {
  private readonly api = inject(AccountApi);

  readonly consents = signal<ConsentStateDto[]>([]);
  readonly permission = signal<BrowserPermissionState>(
    currentBrowserPermission(),
  );
  readonly loading = signal(true);
  readonly saving = signal(false);
  readonly error = signal<string | null>(null);
  readonly status = signal("");

  ngOnInit(): void {
    this.load();
  }

  load(): void {
    this.loading.set(true);
    this.api.consents().subscribe({
      next: ({ consents }) => {
        this.consents.set(consents);
        this.loading.set(false);
        this.error.set(null);
      },
      error: () => {
        this.loading.set(false);
        this.error.set("온라인 상태에서 다시 시도해 주세요.");
      },
    });
  }

  generalConsents(): ConsentStateDto[] {
    return this.consents().filter(
      (consent) =>
        consent.scope !== "MARKETING_STORE" &&
        !["TERMS_OF_SERVICE", "PRIVACY_POLICY"].includes(consent.scope),
    );
  }

  storeConsents(): ConsentStateDto[] {
    return this.consents().filter(
      (consent) => consent.scope === "MARKETING_STORE",
    );
  }

  toggle(consent: ConsentStateDto, granted: boolean): void {
    const before = this.consents();
    this.consents.set(
      optimisticConsent(
        before,
        consent.scope,
        consent.store_id,
        granted,
        new Date().toISOString(),
      ),
    );
    this.saving.set(true);
    this.status.set(
      `${this.label(consent.scope)} 동의가 즉시 ${granted ? "켜졌" : "꺼졌"}습니다.`,
    );
    this.api
      .updateConsents({
        consents: [
          {
            scope: consent.scope,
            store_id: consent.store_id,
            action: granted ? "GRANTED" : "REVOKED",
            document_version: consent.document_version,
            source: "ACCOUNT_NOTIFICATION_SETTINGS",
          },
        ],
      })
      .subscribe({
        next: ({ consents }) => {
          this.consents.set(consents);
          this.saving.set(false);
          this.status.set("동의 상태가 저장됐습니다.");
        },
        error: () => {
          this.consents.set(before);
          this.saving.set(false);
          this.status.set("저장에 실패해 이전 동의 상태로 되돌렸습니다.");
          this.load();
        },
      });
  }

  async requestPermission(): Promise<void> {
    if (typeof Notification === "undefined") {
      this.permission.set("unsupported");
      return;
    }
    const result = await Notification.requestPermission();
    this.permission.set(result);
    this.status.set(permissionCopy(result));
  }

  permissionDescription(): string {
    return permissionCopy(this.permission());
  }

  permissionLabel(): string {
    return {
      granted: "허용됨",
      denied: "차단됨",
      default: "미설정",
      unsupported: "미지원",
    }[this.permission()];
  }

  label(scope: ConsentScopeDto): string {
    return LABELS[scope];
  }

  channel(scope: ConsentScopeDto): string {
    return scope === "KAKAO_INFORMATIONAL"
      ? "카카오 알림톡"
      : scope.includes("MARKETING")
        ? "푸시·알림톡"
        : scope === "LOCATION_BASED_SEARCH"
          ? "앱 기능"
          : "Web Push";
  }

  maskedStore(storeId: string | null): string {
    return storeId ? `${storeId.slice(0, 6)}…` : "전체";
  }
}
