import { Component, inject } from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { AuthService } from './auth.service';

@Component({
  selector: 'admin-root',
  standalone: true,
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  template: `
    <div class="layout">
      @if (showNav()) {
        <aside class="nav">
          <div class="brand">DDADAN</div>
          <nav>
            <a routerLink="/stores" routerLinkActive="active">매장</a>
            <a routerLink="/assets" routerLinkActive="active">에셋</a>
            <a routerLink="/screens" routerLinkActive="active">화면</a>
            <a routerLink="/account" routerLinkActive="active">계정</a>
          </nav>
          <div class="footer">
            <button class="secondary" type="button" (click)="logout()">로그아웃</button>
            <div class="muted small">
              <a routerLink="/terms">이용약관</a> · <a routerLink="/privacy">개인정보</a>
            </div>
          </div>
        </aside>
      }
      <main class="content">
        <router-outlet></router-outlet>
      </main>
    </div>
  `,
  styles: [
    `
      .layout {
        display: flex;
        min-height: 100vh;
      }
      .nav {
        width: 200px;
        background: #fff;
        border-right: 1px solid var(--border);
        padding: 18px 14px;
        display: flex;
        flex-direction: column;
      }
      .brand {
        font-weight: 700;
        font-size: 20px;
        margin-bottom: 18px;
      }
      nav {
        display: flex;
        flex-direction: column;
        gap: 4px;
      }
      nav a {
        padding: 8px 10px;
        border-radius: 6px;
        color: var(--text);
      }
      nav a:hover {
        background: #f0f3f9;
        text-decoration: none;
      }
      nav a.active {
        background: #e9efff;
        color: var(--accent);
        font-weight: 600;
      }
      .footer {
        margin-top: auto;
        display: flex;
        flex-direction: column;
        gap: 10px;
      }
      .small {
        font-size: 11px;
      }
      .content {
        flex: 1;
        padding: 22px 28px;
        max-width: 1200px;
      }
    `,
  ],
})
export class App {
  private readonly auth = inject(AuthService);
  private readonly router = inject(Router);

  showNav(): boolean {
    const url = this.router.url;
    if (url.startsWith('/login') || url.startsWith('/signup') || url.startsWith('/register')) {
      return false;
    }
    return this.auth.isLoggedIn();
  }

  async logout() {
    await this.auth.signOut();
    this.router.navigateByUrl('/login');
  }
}
