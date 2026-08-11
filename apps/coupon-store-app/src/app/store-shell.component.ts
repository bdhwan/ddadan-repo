import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';

@Component({
  selector: 'coupon-store-shell',
  imports: [RouterLink, RouterLinkActive, RouterOutlet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <header><a routerLink="/dashboard"><span aria-hidden="true">◆</span> 다단 상점</a><a class="onboarding" routerLink="/onboarding/store">상점 등록</a></header>
    <div class="layout">
      <nav aria-label="상점 주요 메뉴">
        <a routerLink="/dashboard" routerLinkActive="active">오늘</a>
        <a routerLink="/scan" routerLinkActive="active">스캔</a>
        <a routerLink="/loyalty" routerLinkActive="active">도장 정책</a>
        <a routerLink="/catalog" routerLinkActive="active">품목</a>
        <a routerLink="/campaigns" routerLinkActive="active">할인 캠페인</a>
        <a routerLink="/customers" routerLinkActive="active">고객</a>
        <a routerLink="/analytics" routerLinkActive="active">통계</a>
        <a routerLink="/settings" routerLinkActive="active">상점 설정</a>
      </nav>
      <main id="main-content" tabindex="-1"><router-outlet /></main>
    </div>
  `,
  styles: `
    :host { display: block; min-height: 100dvh; }
    header { position: sticky; top: 0; z-index: 5; display: flex; align-items: center; justify-content: space-between; min-height: 58px; padding: 0 1rem; border-bottom: 1px solid var(--coupon-color-border); background: var(--coupon-color-surface); }
    header a { display: inline-flex; align-items: center; gap: .5rem; min-height: 44px; text-decoration: none; font-weight: 900; }
    header span { color: var(--coupon-color-primary); }
    header .onboarding { color: var(--coupon-color-primary); font-size: .875rem; }
    .layout { width: min(100%, 90rem); margin: 0 auto; }
    nav { display: flex; gap: .25rem; overflow-x: auto; padding: .5rem 1rem; border-bottom: 1px solid var(--coupon-color-border); background: var(--coupon-color-surface); }
    nav a { display: inline-flex; align-items: center; min-height: 44px; padding: .5rem .75rem; border-radius: var(--coupon-radius-sm); white-space: nowrap; text-decoration: none; font-weight: 700; color: var(--coupon-color-text-muted); }
    nav a.active { background: var(--coupon-color-surface-muted); color: var(--coupon-color-primary); }
    main { min-width: 0; padding: 1.5rem 1rem 3rem; }
    @media (min-width: 768px) {
      .layout { display: grid; grid-template-columns: 14rem minmax(0, 1fr); }
      nav { position: sticky; top: 58px; align-self: start; flex-direction: column; height: calc(100dvh - 58px); border-right: 1px solid var(--coupon-color-border); border-bottom: 0; padding: 1rem; }
      main { padding: 2rem; }
    }
    @media (min-width: 1280px) { main { padding: 2.5rem 3rem; } }
  `,
})
export class StoreShellComponent {}
