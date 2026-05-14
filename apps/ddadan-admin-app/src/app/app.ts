import { Component } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';

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
            <a routerLink="/devices" routerLinkActive="active">디바이스</a>
            <a routerLink="/assets" routerLinkActive="active">에셋</a>
            <a routerLink="/screens" routerLinkActive="active">화면</a>
          </nav>
          <div class="footer">
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
  showNav(): boolean {
    return true;
  }
}
