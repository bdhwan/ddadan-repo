import {
  ChangeDetectionStrategy,
  Component,
  inject,
  signal,
} from "@angular/core";
import { AuthSessionService } from "@coupon/client-core";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
} from "@coupon/ui";

@Component({
  selector: "coupon-account-security",
  imports: [
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <coupon-page-header
        title="보안 설정"
        description="연결된 로그인 수단과 세션을 확인합니다."
        eyebrow="Account"
      />
      <coupon-card>
        <div class="heading">
          <div>
            <h2>연결 로그인 수단</h2>
            <p>계정에 실제로 연결된 Firebase provider만 표시합니다.</p>
          </div>
          <coupon-badge status="success" label="연결됨">연결됨</coupon-badge>
        </div>
        <ul>
          @for (provider of providers(); track provider) {
            <li>{{ providerLabel(provider) }}</li>
          } @empty {
            <li>현재 세션에서 로그인 수단을 확인할 수 없습니다.</li>
          }
        </ul>
      </coupon-card>
      <coupon-card>
        <h2>비밀번호 변경</h2>
        <p>
          로그인 이메일로 Firebase 비밀번호 재설정 링크를 보냅니다. 계정 존재
          여부는 안내에 노출하지 않습니다.
        </p>
        <coupon-button
          variant="secondary"
          [disabled]="busy()"
          (click)="resetPassword()"
          >비밀번호 변경 링크 보내기</coupon-button
        >
      </coupon-card>
      <coupon-card>
        <h2>이 기기에서 로그아웃</h2>
        <p>
          현재 브라우저의 Firebase 세션만 종료합니다. 다른 기기의 세션 폐기는
          소비자 API 계약에 없으므로 이 화면에서 요청하지 않습니다.
        </p>
        <coupon-button
          variant="secondary"
          [disabled]="busy()"
          (click)="signOutThisDevice()"
          >이 기기에서 로그아웃</coupon-button
        >
      </coupon-card>
      <p role="status" aria-live="polite">{{ status() }}</p>
    </main>
  `,
  styles: `
    main {
      display: grid;
      width: min(100% - 2rem, 48rem);
      gap: 1rem;
      margin: 0 auto;
      padding: 2rem 0;
    }
    h2 {
      margin-top: 0;
    }
    p {
      color: var(--coupon-color-text-muted);
    }
    .heading {
      display: flex;
      align-items: start;
      justify-content: space-between;
      gap: 1rem;
    }
  `,
})
export class AccountSecurityComponent {
  private readonly auth = inject(AuthSessionService);

  readonly busy = signal(false);
  readonly status = signal("");

  providers(): string[] {
    return (
      this.auth.currentUser?.providerData.map((item) => item.providerId) ?? []
    );
  }

  providerLabel(provider: string): string {
    return (
      {
        password: "이메일·비밀번호",
        "oidc.kakao": "카카오",
        "kakao.com": "카카오",
      }[provider] ?? provider
    );
  }

  async resetPassword(): Promise<void> {
    this.busy.set(true);
    try {
      await this.auth.sendPasswordReset();
      this.status.set(
        "계정 존재 여부와 관계없이 동일한 안내를 표시합니다. 재설정 메일을 확인해 주세요.",
      );
    } catch {
      this.status.set("계정 존재 여부와 관계없이 재설정 안내를 확인해 주세요.");
    } finally {
      this.busy.set(false);
    }
  }

  async signOutThisDevice(): Promise<void> {
    this.busy.set(true);
    try {
      await this.auth.signOut();
      this.status.set("이 기기에서 로그아웃했습니다.");
    } catch {
      this.status.set("로그아웃하지 못했습니다. 다시 시도해 주세요.");
    } finally {
      this.busy.set(false);
    }
  }
}
