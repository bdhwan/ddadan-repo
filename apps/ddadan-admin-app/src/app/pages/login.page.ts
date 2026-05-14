import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { AuthService } from '../auth.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <div class="wrap">
      <div class="panel" style="width:380px">
        <h2>DDADAN 로그인</h2>
        <div class="field">
          <label>이메일</label>
          <input type="email" [(ngModel)]="email" />
        </div>
        <div class="field">
          <label>비밀번호</label>
          <input type="password" [(ngModel)]="password" />
        </div>
        @if (error()) {
          <div class="error">{{ error() }}</div>
        }
        <button (click)="loginEmail()" [disabled]="busy()" style="width:100%; margin-top:8px">
          이메일로 로그인
        </button>
        <button class="secondary" (click)="loginGoogle()" [disabled]="busy()" style="width:100%; margin-top:8px">
          Google로 로그인
        </button>
        <p class="muted" style="margin-top:14px">
          계정이 없나요? <a routerLink="/signup">회원가입</a>
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
      h2 {
        margin: 0 0 16px;
      }
    `,
  ],
})
export class LoginPage {
  private readonly auth = inject(AuthService);
  private readonly router = inject(Router);
  email = '';
  password = '';
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);

  async loginEmail() {
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.auth.signInWithEmail(this.email, this.password);
      this.router.navigateByUrl('/devices');
    } catch (err) {
      this.error.set((err as Error).message);
    } finally {
      this.busy.set(false);
    }
  }

  async loginGoogle() {
    this.busy.set(true);
    this.error.set(null);
    try {
      await this.auth.signInWithGoogle();
      this.router.navigateByUrl('/devices');
    } catch (err) {
      this.error.set((err as Error).message);
    } finally {
      this.busy.set(false);
    }
  }
}
