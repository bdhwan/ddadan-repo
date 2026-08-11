import { ChangeDetectionStrategy, Component } from "@angular/core";
import { RouterLink, RouterLinkActive, RouterOutlet } from "@angular/router";

@Component({
  selector: "coupon-consumer-shell",
  imports: [RouterLink, RouterLinkActive, RouterOutlet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <header>
      <a routerLink="/" aria-label="다단 쿠폰 홈"
        ><span aria-hidden="true">◆</span> 다단 쿠폰</a
      >
    </header>
    <main id="main-content" tabindex="-1"><router-outlet /></main>
    <nav aria-label="소비자 주요 메뉴">
      <a
        routerLink="/"
        [routerLinkActiveOptions]="{ exact: true }"
        routerLinkActive="active"
        ><span aria-hidden="true">⌂</span><span>홈</span></a
      >
      <a routerLink="/wallet" routerLinkActive="active"
        ><span aria-hidden="true">▣</span><span>지갑</span></a
      >
      <a routerLink="/my-qr" routerLinkActive="active"
        ><span aria-hidden="true">▦</span><span>내 QR</span></a
      >
      <a routerLink="/notifications" routerLinkActive="active"
        ><span aria-hidden="true">○</span><span>알림</span></a
      >
      <a routerLink="/account" routerLinkActive="active"
        ><span aria-hidden="true">◉</span><span>내 정보</span></a
      >
    </nav>
  `,
  styles: `
    :host {
      display: block;
      min-height: 100dvh;
      padding-bottom: 82px;
    }
    header {
      position: sticky;
      top: 0;
      z-index: 5;
      border-bottom: 1px solid var(--coupon-color-border);
      background: color-mix(in srgb, var(--coupon-color-bg) 92%, transparent);
      backdrop-filter: blur(12px);
    }
    header a {
      display: flex;
      align-items: center;
      gap: 0.5rem;
      width: min(100% - 2rem, 70rem);
      min-height: 56px;
      margin: auto;
      text-decoration: none;
      font-weight: 900;
    }
    header span {
      color: var(--coupon-color-primary);
    }
    main {
      width: min(100% - 2rem, 62rem);
      margin: 0 auto;
      padding: 1.5rem 0 2rem;
    }
    nav {
      position: fixed;
      inset: auto 0 0;
      z-index: 10;
      display: grid;
      grid-template-columns: repeat(5, 1fr);
      border-top: 1px solid var(--coupon-color-border);
      background: var(--coupon-color-surface);
      padding-bottom: env(safe-area-inset-bottom);
    }
    nav a {
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: 0.15rem;
      min-height: 64px;
      padding: 0.35rem 0.15rem;
      color: var(--coupon-color-text-muted);
      text-decoration: none;
      font-size: 0.75rem;
      font-weight: 700;
    }
    nav a > span:first-child {
      font-size: 1.15rem;
    }
    nav a.active {
      color: var(--coupon-color-primary);
    }
    @media (min-width: 768px) {
      :host {
        padding-bottom: 0;
      }
      nav {
        position: sticky;
        top: 56px;
        display: flex;
        justify-content: center;
        gap: 0.5rem;
        border-top: 0;
        border-bottom: 1px solid var(--coupon-color-border);
        padding: 0.35rem;
      }
      nav a {
        flex-direction: row;
        min-width: 7rem;
        min-height: 44px;
        border-radius: var(--coupon-radius-sm);
        font-size: 0.875rem;
      }
      nav a.active {
        background: var(--coupon-color-surface-muted);
      }
    }
  `,
})
export class ConsumerShellComponent {}
