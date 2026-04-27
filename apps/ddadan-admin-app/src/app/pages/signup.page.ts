import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { ApiService } from '../api.service';
import { AuthService } from '../auth.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <div class="wrap">
      <div class="panel" style="width:420px">
        <h2>회원가입</h2>
        <div class="field">
          <label>이메일</label>
          <input type="email" [(ngModel)]="email" />
        </div>
        <div class="field">
          <label>비밀번호 (6자 이상)</label>
          <input type="password" [(ngModel)]="password" />
        </div>
        <label class="muted" style="display:flex; gap:6px; align-items:center">
          <input type="checkbox" [(ngModel)]="agreeAll" style="width:auto" />
          <a routerLink="/terms" target="_blank">이용약관</a> 및
          <a routerLink="/privacy" target="_blank">개인정보처리방침</a>에 모두 동의합니다.
        </label>
        @if (error()) {
          <div class="error">{{ error() }}</div>
        }
        <button (click)="signup()" [disabled]="busy() || !agreeAll" style="width:100%; margin-top:12px">
          이메일로 가입
        </button>
        <button class="secondary" (click)="signupGoogle()" [disabled]="busy() || !agreeAll" style="width:100%; margin-top:8px">
          Google로 가입
        </button>
        <p class="muted" style="margin-top:14px">
          이미 회원이세요? <a routerLink="/login">로그인</a>
        </p>
      </div>
    </div>
  `,
  styles: [
    `
      .wrap {
        min-height: 100vh;
        display: flex;
        align-items: center;
        justify-content: center;
      }
    `,
  ],
})
export class SignupPage {
  private readonly auth = inject(AuthService);
  private readonly api = inject(ApiService);
  private readonly router = inject(Router);
  email = '';
  password = '';
  agreeAll = false;
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);

  async signup() {
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.auth.signUpWithEmail(this.email, this.password);
      await this.acceptCurrentPolicies();
      this.router.navigateByUrl('/stores');
    } catch (err) {
      this.error.set((err as Error).message);
    } finally {
      this.busy.set(false);
    }
  }

  async signupGoogle() {
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.auth.signInWithGoogle();
      await this.acceptCurrentPolicies();
      this.router.navigateByUrl('/stores');
    } catch (err) {
      this.error.set((err as Error).message);
    } finally {
      this.busy.set(false);
    }
  }

  private async acceptCurrentPolicies(): Promise<void> {
    return new Promise((resolve) => {
      this.api.currentPolicies().subscribe({
        next: (res) => {
          const ids = [res.terms?.id, res.privacy?.id].filter(
            (v): v is number => typeof v === 'number',
          );
          if (ids.length === 0) return resolve();
          this.api.acceptPolicies(ids).subscribe({ next: () => resolve(), error: () => resolve() });
        },
        error: () => resolve(),
      });
    });
  }
}
