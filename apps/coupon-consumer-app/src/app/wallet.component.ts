import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormsModule } from '@angular/forms';
import type { WalletCouponDto } from '@coupon/contracts';
import { formatExpiryDday, formatKoreaDateTime, formatStampBoard, formatWon } from '@coupon/domain';
import { visibilityAwarePoll } from '@coupon/client-core';
import { CouponBadgeComponent, CouponButtonComponent, CouponCardComponent, CouponEmptyStateComponent, CouponErrorStateComponent, CouponPageHeaderComponent, CouponSkeletonComponent } from '@coupon/ui';
import { finalize } from 'rxjs';
import { WalletApi } from './wallet.api';
import { initialWalletState, reduceWalletState, type WalletSnapshot, type WalletViewState } from './wallet-state';

type WalletTab = 'available' | 'stamps' | 'history';
type WalletSort = 'expiry' | 'recent' | 'store';
const CACHE_KEY = 'coupon-wallet-last-snapshot-v2';

@Component({
  selector: 'coupon-wallet',
  imports: [FormsModule, CouponBadgeComponent, CouponButtonComponent, CouponCardComponent, CouponEmptyStateComponent, CouponErrorStateComponent, CouponPageHeaderComponent, CouponSkeletonComponent],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header title="내 지갑" description="여러 상점의 쿠폰과 도장을 한곳에서 확인하세요." eyebrow="30초마다 자동 동기화">
      <coupon-button variant="secondary" (click)="load(true)" [disabled]="state().status === 'loading'">새로고침</coupon-button>
    </coupon-page-header>

    @if (state().status === 'offline' || state().status === 'stale') {
      <div class="sync-banner" role="status"><span aria-hidden="true">⚠</span><div><strong>{{ state().status === 'offline' ? '오프라인 읽기 전용' : '최신 상태 확인 필요' }}</strong><p>{{ state().message }} @if (state().synced_at) { 최종 동기화 {{ date(state().synced_at!) }} }</p></div></div>
    }

    <div class="tabs" role="tablist" aria-label="지갑 분류">
      <button role="tab" [attr.aria-selected]="tab() === 'available'" (click)="selectTab('available')">사용 가능 <span>{{ availableCoupons().length }}</span></button>
      <button role="tab" [attr.aria-selected]="tab() === 'stamps'" (click)="selectTab('stamps')">도장 <span>{{ state().stamps.length }}</span></button>
      <button role="tab" [attr.aria-selected]="tab() === 'history'" (click)="selectTab('history')">사용·만료 내역 <span>{{ historyCoupons().length }}</span></button>
    </div>

    @if (tab() !== 'stamps') {
      <section class="filters" aria-label="지갑 필터와 정렬">
        <label>상점<select [(ngModel)]="storeFilter"><option value="">전체 상점</option>@for (store of stores(); track store) { <option [value]="store">{{ store }}</option> }</select></label>
        <label>혜택 유형<select [(ngModel)]="benefitFilter"><option value="">전체 혜택</option><option value="FIXED">정액 할인</option><option value="PERCENTAGE">정률 할인</option><option value="FREE_ITEM">무료 품목</option><option value="STAMP_REWARD">도장 리워드</option></select></label>
        <label class="check"><input type="checkbox" [(ngModel)]="expiresSoon" />7일 이내 만료</label>
        <label>정렬<select [(ngModel)]="sort"><option value="expiry">만료 임박</option><option value="recent">최근 발급</option><option value="store">상점명</option></select></label>
      </section>
    }

    @if (state().status === 'loading' && !state().synced_at) {
      <coupon-card><coupon-skeleton [lines]="8" label="지갑을 불러오는 중입니다." /></coupon-card>
    } @else if (state().status === 'error') {
      <coupon-error-state title="지갑을 불러오지 못했어요" [description]="state().message ?? '연결 상태를 확인해 주세요.'" [retryable]="true" (retry)="load(true)" />
    } @else {
      <section role="tabpanel" tabindex="0">
        @switch (tab()) {
          @case ('stamps') {
            @if (state().stamps.length === 0) {
              <coupon-empty-state title="아직 모은 도장이 없어요" description="매장에서 내 QR을 보여주고 첫 도장을 적립해 보세요." />
            } @else {
              <div class="card-grid">
                @for (board of state().stamps; track board.store_id) {
                  <article class="stamp-card">
                    <div><span class="store-mark" aria-hidden="true">◆</span><strong>{{ board.store_name }}</strong><coupon-badge [status]="board.policy_status === 'ACTIVE' ? 'success' : 'neutral'" [label]="board.policy_status === 'ACTIVE' ? '적립 가능' : '적립 종료'">{{ board.policy_status === 'ACTIVE' ? '적립 가능' : '적립 종료' }}</coupon-badge></div>
                    <p class="stamp-count" [attr.aria-label]="'도장 ' + board.available_stamps + '개, 목표 ' + board.goal_stamps + '개'">{{ stampBoard(board.available_stamps, board.goal_stamps) }}</p>
                    <div class="stamp-track"><span [style.width.%]="stampProgress(board.available_stamps, board.goal_stamps)"></span></div>
                    <p><strong>리워드</strong> {{ board.reward_description }}</p>
                    <p class="muted">가장 이른 도장 만료 {{ board.earliest_stamp_expires_at ? date(board.earliest_stamp_expires_at) : '만료 예정 없음' }}</p>
                  </article>
                }
              </div>
            }
          }
          @default {
            @if (filteredCoupons().length === 0) {
              <coupon-empty-state [title]="tab() === 'available' ? '사용 가능한 쿠폰이 없어요' : '사용·만료 내역이 없어요'" description="필터를 바꾸거나 관심 상점의 혜택을 확인해 보세요." />
            } @else {
              <div class="card-grid">
                @for (coupon of filteredCoupons(); track coupon.id) {
                  <article class="coupon-card">
                    <div class="card-top"><coupon-badge [status]="badgeStatus(coupon.status)" [label]="statusLabel(coupon.status)">{{ statusLabel(coupon.status) }}</coupon-badge><span>{{ expiry(coupon.expires_at) }}</span></div>
                    <p class="benefit">{{ coupon.benefit_label }}</p><h2>{{ coupon.store_name }}</h2>
                    <dl><div><dt>최소 주문</dt><dd>{{ won(coupon.minimum_order_amount.amount) }}</dd></div><div><dt>품목 제한</dt><dd>{{ coupon.item_restriction_summary ?? '없음' }}</dd></div><div><dt>만료</dt><dd>{{ date(coupon.expires_at) }}</dd></div></dl>
                    <button #detailTrigger type="button" class="detail" (click)="openDetail(coupon, detailTrigger)">전체 조건과 식별번호 보기</button>
                  </article>
                }
              </div>
            }
          }
        }
      </section>
    }

    @if (selected(); as coupon) {
      <div class="backdrop" (click)="closeDetail()"></div>
      <section #detailPanel class="detail-panel" role="dialog" aria-modal="true" aria-labelledby="coupon-detail-title" tabindex="-1" (keydown.escape)="closeDetail()">
        <div class="panel-head"><div><coupon-badge [status]="badgeStatus(coupon.status)" [label]="statusLabel(coupon.status)">{{ statusLabel(coupon.status) }}</coupon-badge><h2 id="coupon-detail-title">{{ coupon.benefit_label }}</h2><p>{{ coupon.store_name }}</p></div><button type="button" aria-label="상세 닫기" (click)="closeDetail()">×</button></div>
        <h3>전체 사용 조건</h3><ul>@for (condition of coupon.conditions; track condition) { <li>{{ condition }}</li> }</ul>
        <dl class="detail-dl"><div><dt>발급 사유</dt><dd>{{ coupon.issued_reason }}</dd></div><div><dt>발급 시각</dt><dd>{{ date(coupon.issued_at) }}</dd></div><div><dt>사용 시각</dt><dd>{{ coupon.used_at ? date(coupon.used_at) : '해당 없음' }}</dd></div><div><dt>만료 시각</dt><dd>{{ coupon.expired_at ? date(coupon.expired_at) : date(coupon.expires_at) }}</dd></div>@if (coupon.terminal_reason) { <div><dt>종료 사유</dt><dd>{{ coupon.terminal_reason }}</dd></div> }<div><dt>문의용 식별번호</dt><dd><code>{{ coupon.inquiry_reference }}</code></dd></div></dl>
        <coupon-button [fullWidth]="true" (click)="closeDetail()">확인</coupon-button>
      </section>
    }
  `,
  styles: `
    :host{display:block}.sync-banner{display:grid;grid-template-columns:2rem 1fr;gap:.6rem;margin-bottom:1rem;padding:.8rem;border:1px solid var(--coupon-color-warning);border-radius:var(--coupon-radius-sm);color:var(--coupon-color-warning);background:var(--coupon-color-surface)}.sync-banner p{margin:.15rem 0 0;color:var(--coupon-color-text-muted)}.tabs{display:grid;grid-template-columns:repeat(3,1fr);margin-bottom:1rem;border-bottom:1px solid var(--coupon-color-border)}.tabs button{min-height:48px;border:0;border-bottom:3px solid transparent;background:transparent;color:var(--coupon-color-text-muted);font-weight:800}.tabs button[aria-selected=true]{border-color:var(--coupon-color-primary);color:var(--coupon-color-primary)}.tabs span{display:inline-grid;place-items:center;min-width:1.4rem;padding:.05rem .35rem;border-radius:1rem;background:var(--coupon-color-surface-muted);font-size:.75rem}.filters{display:grid;grid-template-columns:1fr 1fr;gap:.7rem;margin-bottom:1rem}.filters label{display:grid;gap:.25rem;font-size:.875rem;font-weight:700}.filters select{min-height:44px;padding:.5rem;border:1px solid var(--coupon-color-border);border-radius:var(--coupon-radius-sm);background:var(--coupon-color-surface);color:var(--coupon-color-text)}.filters .check{display:flex;align-items:center;min-height:44px}.check input{width:22px;height:22px;margin-right:.5rem}.card-grid{display:grid;gap:1rem}.coupon-card,.stamp-card{padding:1rem;border:1px solid var(--coupon-color-border);border-radius:var(--coupon-radius-md);background:var(--coupon-color-surface);box-shadow:var(--coupon-shadow)}.card-top,.stamp-card>div:first-child{display:flex;align-items:center;justify-content:space-between;gap:.5rem}.card-top>span{color:var(--coupon-color-danger);font-weight:800}.benefit{margin:1rem 0 .15rem;color:var(--coupon-color-primary);font-size:1.4rem;font-weight:900}.coupon-card h2{margin:0 0 1rem;font-size:1rem}.coupon-card dl,.detail-dl{display:grid;gap:.4rem;margin:0}.coupon-card dl div,.detail-dl div{display:grid;grid-template-columns:6.5rem 1fr;gap:.5rem}.coupon-card dt,.detail-dl dt{color:var(--coupon-color-text-muted)}dd{margin:0}.detail{width:100%;min-height:44px;margin-top:1rem;border:1px solid var(--coupon-color-primary);border-radius:var(--coupon-radius-sm);background:transparent;color:var(--coupon-color-primary);font-weight:800}.stamp-count{margin:1.25rem 0 .25rem;font-size:2rem;font-weight:900}.stamp-track{height:.65rem;overflow:hidden;border-radius:1rem;background:var(--coupon-color-surface-muted)}.stamp-track span{display:block;height:100%;background:var(--coupon-color-primary)}.store-mark{color:var(--coupon-color-primary)}.muted{color:var(--coupon-color-text-muted)}.backdrop{position:fixed;inset:0;z-index:20;background:#0008}.detail-panel{position:fixed;inset:auto 0 0;z-index:21;max-height:88dvh;overflow:auto;padding:1.25rem;border-radius:1.25rem 1.25rem 0 0;background:var(--coupon-color-surface)}.panel-head{display:flex;justify-content:space-between;gap:1rem}.panel-head h2{margin:.5rem 0 0}.panel-head p{margin:.2rem 0}.panel-head>button{width:44px;height:44px;border:0;background:transparent;color:var(--coupon-color-text);font-size:2rem}.detail-panel h3{margin-top:1.5rem}.detail-dl{margin:1.5rem 0}.detail-dl div{padding:.5rem 0;border-bottom:1px solid var(--coupon-color-border)}code{overflow-wrap:anywhere}@media(min-width:700px){.filters{grid-template-columns:repeat(4,1fr)}.card-grid{grid-template-columns:repeat(2,minmax(0,1fr))}.detail-panel{inset:50% auto auto 50%;width:min(90vw,36rem);max-height:85vh;transform:translate(-50%,-50%);border-radius:var(--coupon-radius-lg)}}
  `,
})
export class WalletComponent implements OnInit {
  private readonly api = inject(WalletApi);
  private readonly destroyRef = inject(DestroyRef);
  private previousFocus: HTMLElement | null = null;
  readonly detailPanel = viewChild<ElementRef<HTMLElement>>('detailPanel');
  readonly state = signal<WalletViewState>(initialWalletState());
  readonly tab = signal<WalletTab>('available');
  readonly selected = signal<WalletCouponDto | null>(null);
  readonly storeFilter = signal('');
  readonly benefitFilter = signal('');
  readonly expiresSoon = signal(false);
  readonly sort = signal<WalletSort>('expiry');
  private allCoupons = signal<WalletCouponDto[]>([]);
  readonly availableCoupons = computed(() => this.allCoupons().filter((coupon) => coupon.status === 'AVAILABLE' || coupon.status === 'RESERVED'));
  readonly historyCoupons = computed(() => this.allCoupons().filter((coupon) => !['AVAILABLE', 'RESERVED'].includes(coupon.status)));
  readonly stores = computed(() => [...new Set(this.allCoupons().map((coupon) => coupon.store_name))].sort((a, b) => a.localeCompare(b, 'ko')));
  readonly filteredCoupons = computed(() => {
    const source = this.tab() === 'available' ? this.availableCoupons() : this.historyCoupons();
    return source.filter((coupon) => !this.storeFilter() || coupon.store_name === this.storeFilter())
      .filter((coupon) => !this.benefitFilter() || coupon.benefit_type === this.benefitFilter())
      .filter((coupon) => !this.expiresSoon() || Date.parse(coupon.expires_at) - Date.now() <= 7 * 86_400_000)
      .sort((a, b) => this.compare(a, b));
  });

  ngOnInit(): void {
    const updateOnline = () => navigator.onLine ? this.load(true) : this.state.update((state) => reduceWalletState(state, { type: 'OFFLINE', cached: this.cached() }));
    window.addEventListener('online', updateOnline);
    window.addEventListener('offline', updateOnline);
    this.destroyRef.onDestroy(() => { window.removeEventListener('online', updateOnline); window.removeEventListener('offline', updateOnline); });
    visibilityAwarePoll(30_000).pipe(takeUntilDestroyed(this.destroyRef)).subscribe(() => this.load(false));
  }

  load(announce: boolean): void {
    if (!navigator.onLine) { this.state.update((state) => reduceWalletState(state, { type: 'OFFLINE', cached: this.cached() })); return; }
    if (announce || !this.state().synced_at) this.state.update((state) => reduceWalletState(state, { type: 'LOAD' }));
    const current = this.state();
    this.api.load(current.version === null ? null : { version: current.version, updated_at: current.updated_at ?? undefined }).pipe(
      finalize(() => undefined), takeUntilDestroyed(this.destroyRef),
    ).subscribe({
      next: ({ available, history, stamps }) => {
        const coupons = this.mergeCoupons([...available.items, ...history.items]);
        const snapshot: WalletSnapshot = { coupons, stamps: stamps.items, synced_at: new Date().toISOString(), version: Math.max(available.version, history.version, stamps.version), updated_at: [available.updated_at, history.updated_at, stamps.updated_at].sort().at(-1) ?? null };
        localStorage.setItem(CACHE_KEY, JSON.stringify(snapshot)); this.allCoupons.set(coupons); this.state.update((state) => reduceWalletState(state, { type: 'SUCCESS', snapshot }));
      },
      error: () => { const cached = this.cached(); if (cached) this.allCoupons.set(cached.coupons); this.state.update((state) => reduceWalletState(state, { type: 'FAILURE', message: '지갑을 불러오지 못했습니다.', cached, online: navigator.onLine })); },
    });
  }

  selectTab(tab: WalletTab): void { this.tab.set(tab); }
  openDetail(coupon: WalletCouponDto, trigger: HTMLElement): void { this.previousFocus = trigger; this.selected.set(coupon); setTimeout(() => this.detailPanel()?.nativeElement.focus()); }
  closeDetail(): void { this.selected.set(null); setTimeout(() => this.previousFocus?.focus()); }
  won(value: number): string { return formatWon(value); }
  date(value: string): string { return formatKoreaDateTime(value); }
  expiry(value: string): string { return formatExpiryDday(value); }
  stampBoard(current: number, goal: number): string { return formatStampBoard(current, goal); }
  stampProgress(current: number, goal: number): number { return Math.min(100, Math.round(current / goal * 100)); }
  statusLabel(status: WalletCouponDto['status']): string { return ({ PENDING: '발급 중', AVAILABLE: '사용 가능', RESERVED: '사용 확인 중', USED: '사용 완료', EXPIRED: '기간 만료', REVOKED: '운영 회수', VOIDED: '사용 취소' })[status]; }
  badgeStatus(status: WalletCouponDto['status']): 'success' | 'warning' | 'danger' | 'neutral' { return status === 'AVAILABLE' ? 'success' : status === 'RESERVED' || status === 'PENDING' ? 'warning' : status === 'REVOKED' ? 'danger' : 'neutral'; }

  private compare(a: WalletCouponDto, b: WalletCouponDto): number { if (this.sort() === 'recent') return Date.parse(b.issued_at) - Date.parse(a.issued_at); if (this.sort() === 'store') return a.store_name.localeCompare(b.store_name, 'ko'); return Date.parse(a.expires_at) - Date.parse(b.expires_at); }
  private mergeCoupons(items: WalletCouponDto[]): WalletCouponDto[] { return [...new Map(items.map((item) => [item.id, item])).values()]; }
  private cached(): WalletSnapshot | null { try { const raw = localStorage.getItem(CACHE_KEY); return raw ? JSON.parse(raw) as WalletSnapshot : null; } catch { return null; } }
}
