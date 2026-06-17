import { Component, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { filter, map, startWith } from 'rxjs';

@Component({
  selector: 'admin-root',
  standalone: true,
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  template: `
    <div class="layout" [class.preview-layout]="!showNav()">
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
      <main class="content" [class.preview-content]="!showNav()">
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
      .layout.preview-layout {
        min-height: 100dvh;
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
      .content.preview-content {
        padding: 0;
        max-width: none;
      }
    `,
  ],
})
export class App {
  private readonly router = inject(Router);
  private readonly url = toSignal(
    this.router.events.pipe(
      filter((e) => e instanceof NavigationEnd),
      map(() => this.router.url),
      startWith(this.router.url),
    ),
    { initialValue: this.router.url },
  );

  showNav(): boolean {
    return !this.url().startsWith('/preview/');
  }
}
