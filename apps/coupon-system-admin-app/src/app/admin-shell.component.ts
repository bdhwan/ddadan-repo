import { ChangeDetectionStrategy, Component } from "@angular/core";
import { RouterLink, RouterLinkActive, RouterOutlet } from "@angular/router";

@Component({
  selector: "coupon-admin-shell",
  imports: [RouterLink, RouterLinkActive, RouterOutlet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="mobile-warning" role="alert">
      <span aria-hidden="true">⚠</span><strong>모바일 읽기 전용</strong
      ><span>변경 작업은 1024px 이상 화면에서만 진행하세요.</span>
    </div>
    <div class="layout">
      <aside>
        <a class="brand" routerLink="/store-reviews"
          ><span aria-hidden="true">◆</span> 쿠폰 운영</a
        >
        <nav aria-label="운영 관리자 주요 메뉴">
          <a routerLink="/operations" routerLinkActive="active">운영 현황</a>
          <a routerLink="/store-reviews" routerLinkActive="active">상점 검수</a>
          <a routerLink="/members" routerLinkActive="active">회원·상점</a>
          <a routerLink="/transactions" routerLinkActive="active">거래 탐색</a>
          <a routerLink="/campaigns" routerLinkActive="active">캠페인</a>
          <a routerLink="/jobs" routerLinkActive="active">작업 큐</a>
          <a routerLink="/audit" routerLinkActive="active">감사</a>
        </nav>
      </aside>
      <main id="main-content" tabindex="-1"><router-outlet /></main>
    </div>
  `,
  styles: `
    .mobile-warning {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.5rem;
      padding: 0.75rem 1rem;
      border-bottom: 1px solid var(--coupon-color-warning);
      background: var(--coupon-color-surface);
      color: var(--coupon-color-warning);
    }
    .layout {
      min-height: calc(100dvh - 70px);
    }
    aside {
      padding: 0.75rem 1rem;
      border-bottom: 1px solid var(--coupon-color-border);
      background: var(--coupon-color-surface);
    }
    .brand {
      display: inline-flex;
      align-items: center;
      gap: 0.5rem;
      min-height: 44px;
      text-decoration: none;
      font-weight: 900;
    }
    .brand span {
      color: var(--coupon-color-primary);
    }
    nav {
      display: flex;
      gap: 0.25rem;
      overflow-x: auto;
    }
    nav a {
      display: inline-flex;
      align-items: center;
      min-height: 44px;
      padding: 0.5rem 0.75rem;
      border-radius: var(--coupon-radius-sm);
      white-space: nowrap;
      text-decoration: none;
      color: var(--coupon-color-text-muted);
      font-weight: 700;
    }
    nav a.active {
      background: var(--coupon-color-surface-muted);
      color: var(--coupon-color-primary);
    }
    main {
      min-width: 0;
      padding: 1.5rem 1rem 3rem;
    }
    @media (max-width: 1023px) {
      main {
        pointer-events: none;
      }
      main a {
        pointer-events: auto;
      }
    }
    @media (min-width: 1024px) {
      .mobile-warning {
        display: none;
      }
      .layout {
        display: grid;
        grid-template-columns: 16rem minmax(0, 1fr);
        min-height: 100dvh;
      }
      aside {
        position: sticky;
        top: 0;
        align-self: start;
        height: 100dvh;
        border-right: 1px solid var(--coupon-color-border);
        border-bottom: 0;
        padding: 1rem;
      }
      nav {
        flex-direction: column;
        margin-top: 1.5rem;
        overflow: visible;
      }
      main {
        padding: 2.5rem 3rem;
      }
    }
  `,
})
export class AdminShellComponent {}
