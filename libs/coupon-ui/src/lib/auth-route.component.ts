import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ActivatedRoute, RouterLink } from "@angular/router";
import { CouponButtonComponent } from "./button.component";
import { CouponCardComponent } from "./card.component";
import { CouponPageHeaderComponent } from "./page-header.component";

const AUTH_COPY: Record<string, { title: string; description: string }> = {
  login: {
    title: "로그인",
    description:
      "이메일 또는 비밀번호가 맞지 않습니다라는 하나의 안전한 안내를 사용합니다.",
  },
  signup: {
    title: "이메일 가입",
    description: "필수 약관과 선택 동의를 나누어 확인합니다.",
  },
  kakao: {
    title: "카카오 로그인 확인",
    description: "보안 검증을 완료하는 중입니다.",
  },
  verify: {
    title: "이메일 인증",
    description: "인증 메일 재발송 제한과 완료 상태를 안내합니다.",
  },
  terms: {
    title: "약관 동의",
    description: "필수·선택 항목과 버전을 확인하세요.",
  },
  security: {
    title: "보안 설정",
    description: "로그인 수단, 세션, 비밀번호를 관리합니다.",
  },
  notifications: {
    title: "알림 설정",
    description: "목적·상점·채널별 동의를 관리합니다.",
  },
  withdraw: {
    title: "계정 탈퇴",
    description: "영향, 재인증, 보존 기간을 먼저 확인합니다.",
  },
};

@Component({
  selector: "coupon-auth-route",
  imports: [
    RouterLink,
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main>
      <a class="skip-home" routerLink="/">홈으로 돌아가기</a>
      <coupon-page-header
        [title]="copy.title"
        [description]="copy.description"
        eyebrow="Account"
      />
      <coupon-card>
        @if (mode === "login" || mode === "signup") {
          <form (submit)="$event.preventDefault()">
            <label
              >이메일<input
                type="email"
                autocomplete="email"
                placeholder="name@example.com"
            /></label>
            <label
              >비밀번호<input
                type="password"
                [attr.autocomplete]="
                  mode === 'signup' ? 'new-password' : 'current-password'
                "
            /></label>
            @if (mode === "signup") {
              <label>이름<input autocomplete="name" /></label>
            }
            <coupon-button type="submit" [fullWidth]="true">{{
              mode === "signup" ? "가입 안내 받기" : "로그인"
            }}</coupon-button>
            <coupon-button variant="secondary" [fullWidth]="true"
              >카카오로 계속하기</coupon-button
            >
          </form>
        } @else {
          <div class="placeholder" role="status">
            <span aria-hidden="true">◇</span>
            <h2>Phase 1 안전 흐름 shell</h2>
            <p>
              서버 연동 전에 키보드와 스크린리더로 흐름을 검증할 수 있는
              상태입니다.
            </p>
          </div>
        }
      </coupon-card>
    </main>
  `,
  styles: `
    main {
      width: min(100% - 2rem, 36rem);
      margin: 0 auto;
      padding: 2rem 0;
    }
    .skip-home {
      display: inline-flex;
      min-height: 44px;
      align-items: center;
      margin-bottom: 1rem;
      color: var(--coupon-color-primary);
      font-weight: 700;
    }
    form {
      display: grid;
      gap: 1rem;
    }
    label {
      display: grid;
      gap: 0.4rem;
      font-weight: 700;
    }
    input {
      min-height: 44px;
      padding: 0.65rem 0.75rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
      font: inherit;
    }
    .placeholder {
      text-align: center;
    }
    .placeholder > span {
      font-size: 2rem;
      color: var(--coupon-color-primary);
    }
    .placeholder p {
      color: var(--coupon-color-text-muted);
    }
  `,
})
export class CouponAuthRouteComponent {
  private readonly route = inject(ActivatedRoute);
  readonly mode = String(this.route.snapshot.data["mode"] ?? "login");
  readonly copy = AUTH_COPY[this.mode] ?? AUTH_COPY["login"]!;
}
