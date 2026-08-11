import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { ActivatedRoute, RouterLink } from "@angular/router";
import type {
  PublicCampaignDto,
  PublicStoreDetailDto,
} from "@coupon/contracts";
import { CouponClientError } from "@coupon/client-core";
import { formatKoreaDateTime, formatWon } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponEmptyStateComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import {
  INITIAL_CLAIM_STATE,
  beginCampaignClaim,
  rejectCampaignClaim,
  resolveCampaignClaim,
  type CampaignClaimState,
} from "./store-detail-state";
import { StoreDetailApi } from "./store-detail.api";

@Component({
  selector: "coupon-store-detail",
  imports: [
    RouterLink,
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponEmptyStateComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (loading()) {
      <coupon-card
        ><coupon-skeleton
          [lines]="10"
          label="상점 정보와 캠페인을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="상점을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (store(); as current) {
      <coupon-page-header
        [title]="current.name"
        [description]="current.introduction"
        eyebrow="공개 상점"
      >
        <coupon-button
          variant="secondary"
          [disabled]="favoriteBusy()"
          (click)="toggleFavorite()"
          >{{
            current.is_favorite ? "★ 관심 해제" : "☆ 관심 등록"
          }}</coupon-button
        >
      </coupon-page-header>

      @if (favoriteMessage()) {
        <p class="status-message" role="status">{{ favoriteMessage() }}</p>
      }
      @if (current.status !== "ACTIVE") {
        <div class="store-alert" role="alert">
          <strong>{{
            current.status === "SUSPENDED" ? "운영 일시 중지" : "영업 종료"
          }}</strong>
          <p>
            상점 상태와 별개로 개별 쿠폰의 사용 조건과 기간을 확인해 주세요.
          </p>
        </div>
      }

      <div class="store-grid">
        <coupon-card
          ><h2>매장 안내</h2>
          <dl>
            <div>
              <dt>위치</dt>
              <dd>{{ current.address_summary }}</dd>
            </div>
            <div>
              <dt>영업시간</dt>
              <dd>{{ current.business_hours_summary }}</dd>
            </div>
            <div>
              <dt>현재 상태</dt>
              <dd>
                {{
                  current.currently_open ? "영업 중 (안내)" : "현재 휴무 (안내)"
                }}
              </dd>
            </div>
            <div>
              <dt>도장 정책</dt>
              <dd>{{ current.loyalty_policy_summary ?? "활성 정책 없음" }}</dd>
            </div>
          </dl>
          <p class="muted">
            현재 휴무 여부는 안내값입니다. 쿠폰 사용 가능성은 각 쿠폰 조건을
            우선합니다.
          </p></coupon-card
        >
        <section aria-labelledby="campaign-title">
          <h2 id="campaign-title">공개 선착순 캠페인</h2>
          @for (campaign of current.campaigns; track campaign.id) {
            <article
              class="campaign-card"
              [class.optimistic]="claimState(campaign.id).optimistic_claimed"
            >
              <div class="campaign-top">
                <coupon-badge status="success" label="선착순 받기 가능"
                  >선착순</coupon-badge
                ><span>{{
                  campaign.remaining_quantity === null
                    ? "운영 상한 내 제공"
                    : campaign.remaining_quantity + "장 남음"
                }}</span>
              </div>
              <p class="benefit">{{ campaign.benefit_label }}</p>
              <h3>{{ campaign.name }}</h3>
              <dl>
                <div>
                  <dt>최소 주문</dt>
                  <dd>{{ won(campaign.minimum_order_amount.amount) }}</dd>
                </div>
                <div>
                  <dt>품목 조건</dt>
                  <dd>{{ campaign.item_restriction_summary ?? "없음" }}</dd>
                </div>
                <div>
                  <dt>받기 종료</dt>
                  <dd>{{ date(campaign.issuance_ends_at) }} (미포함)</dd>
                </div>
                <div>
                  <dt>사용 종료</dt>
                  <dd>{{ date(campaign.usable_until) }} (미포함)</dd>
                </div>
              </dl>
              @if (claimState(campaign.id).message; as message) {
                <p
                  class="claim-message"
                  [class.error]="
                    ['sold_out', 'error'].includes(
                      claimState(campaign.id).status
                    )
                  "
                  role="status"
                >
                  <span aria-hidden="true">{{
                    ["claimed", "duplicate"].includes(
                      claimState(campaign.id).status
                    )
                      ? "✓"
                      : claimState(campaign.id).status === "claiming"
                        ? "…"
                        : "!"
                  }}</span
                  >{{ message }}
                </p>
              }
              @if (claimState(campaign.id).coupon_id) {
                <a class="wallet-link" routerLink="/wallet"
                  >기존 쿠폰을 지갑에서 보기</a
                >
              } @else {
                <coupon-button
                  [fullWidth]="true"
                  [disabled]="claimDisabled(campaign)"
                  (click)="claim(campaign)"
                  >{{
                    claimState(campaign.id).status === "claiming"
                      ? "받는 중…"
                      : claimState(campaign.id).status === "sold_out"
                        ? "소진"
                        : "받기"
                  }}</coupon-button
                >
              }
            </article>
          } @empty {
            <coupon-empty-state
              title="현재 공개 캠페인이 없어요"
              description="관심 등록하면 새 혜택을 쉽게 확인할 수 있어요."
            />
          }
        </section>
      </div>
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .store-grid {
      display: grid;
      gap: 1.25rem;
    }
    h2 {
      margin-top: 0;
    }
    dl {
      display: grid;
      gap: 0.45rem;
      margin: 0;
    }
    dl div {
      display: grid;
      grid-template-columns: 7rem 1fr;
      gap: 0.5rem;
      padding: 0.35rem 0;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dt,
    .muted {
      color: var(--coupon-color-text-muted);
    }
    dd {
      margin: 0;
    }
    .status-message,
    .store-alert,
    .claim-message {
      display: flex;
      gap: 0.45rem;
      padding: 0.75rem;
      border-left: 4px solid var(--coupon-color-primary);
      background: var(--coupon-color-surface-muted);
    }
    .store-alert {
      display: block;
      border-color: var(--coupon-color-warning);
    }
    .campaign-card {
      margin-bottom: 1rem;
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
      transition: transform 0.15s ease;
    }
    .campaign-card.optimistic {
      outline: 2px solid var(--coupon-color-primary);
    }
    .campaign-top {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.5rem;
    }
    .campaign-top > span {
      color: var(--coupon-color-text-muted);
      font-size: 0.875rem;
      font-weight: 800;
    }
    .benefit {
      margin: 1rem 0 0.15rem;
      color: var(--coupon-color-primary);
      font-size: 1.45rem;
      font-weight: 900;
    }
    h3 {
      margin: 0 0 1rem;
    }
    .claim-message {
      align-items: center;
      margin: 1rem 0;
    }
    .claim-message.error {
      border-color: var(--coupon-color-danger);
      color: var(--coupon-color-danger);
    }
    .wallet-link {
      display: grid;
      place-items: center;
      min-height: 44px;
      margin-top: 1rem;
      border: 1px solid var(--coupon-color-primary);
      border-radius: var(--coupon-radius-sm);
      color: var(--coupon-color-primary);
      font-weight: 800;
      text-decoration: none;
    }
    @media (min-width: 768px) {
      .store-grid {
        grid-template-columns: minmax(18rem, 0.8fr) minmax(0, 1.2fr);
        align-items: start;
      }
    }
  `,
})
export class StoreDetailComponent implements OnInit {
  private readonly api = inject(StoreDetailApi);
  private readonly route = inject(ActivatedRoute);
  private readonly destroyRef = inject(DestroyRef);
  readonly store = signal<PublicStoreDetailDto | null>(null);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly favoriteBusy = signal(false);
  readonly favoriteMessage = signal<string | null>(null);
  readonly claims = signal<Record<string, CampaignClaimState>>({});

  ngOnInit(): void {
    this.load();
  }

  load(): void {
    this.loading.set(true);
    this.error.set(null);
    const slug = this.route.snapshot.paramMap.get("slug") ?? "";
    this.api
      .detail(slug)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (store) => {
          this.store.set(store);
          this.claims.set(
            Object.fromEntries(
              store.campaigns.map((campaign) => [
                campaign.id,
                campaign.claimed_coupon_id
                  ? {
                      ...INITIAL_CLAIM_STATE,
                      status: "duplicate",
                      coupon_id: campaign.claimed_coupon_id,
                      optimistic_claimed: true,
                      message:
                        "이미 받은 캠페인입니다. 기존 쿠폰으로 안내합니다.",
                    }
                  : INITIAL_CLAIM_STATE,
              ]),
            ),
          );
          this.loading.set(false);
        },
        error: () => {
          this.error.set("상점이 없거나 서버에 연결할 수 없습니다.");
          this.loading.set(false);
        },
      });
  }

  toggleFavorite(): void {
    const store = this.store();
    if (!store || this.favoriteBusy()) return;
    const previous = store.is_favorite;
    this.store.set({ ...store, is_favorite: !previous });
    this.favoriteBusy.set(true);
    this.favoriteMessage.set(
      !previous ? "관심 상점에 추가했습니다." : "관심 상점에서 제거했습니다.",
    );
    this.api
      .favorite(store.id, !previous, createUuid())
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => this.favoriteBusy.set(false),
        error: () => {
          this.store.update((current) =>
            current ? { ...current, is_favorite: previous } : current,
          );
          this.favoriteMessage.set(
            "관심 상태를 저장하지 못해 이전 상태로 되돌렸습니다.",
          );
          this.favoriteBusy.set(false);
        },
      });
  }

  claim(campaign: PublicCampaignDto): void {
    const current = this.claimState(campaign.id);
    const next = beginCampaignClaim(current, createUuid);
    if (next === current) return;
    this.setClaim(campaign.id, next);
    this.api
      .claim(campaign.id, next.idempotency_key!)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (response) =>
          this.setClaim(campaign.id, resolveCampaignClaim(next, response)),
        error: (error: unknown) => {
          const code =
            error instanceof CouponClientError ? error.code : "UNKNOWN";
          const message =
            error instanceof CouponClientError
              ? error.message
              : "쿠폰을 받지 못했습니다. 같은 요청으로 다시 시도해 주세요.";
          this.setClaim(campaign.id, rejectCampaignClaim(next, code, message));
        },
      });
  }

  claimState(campaignId: string): CampaignClaimState {
    return this.claims()[campaignId] ?? INITIAL_CLAIM_STATE;
  }
  claimDisabled(campaign: PublicCampaignDto): boolean {
    const status = this.claimState(campaign.id).status;
    return (
      status === "claiming" ||
      status === "sold_out" ||
      this.store()?.status !== "ACTIVE"
    );
  }
  won(amount: number): string {
    return formatWon(amount);
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }

  private setClaim(campaignId: string, state: CampaignClaimState): void {
    this.claims.update((claims) => ({ ...claims, [campaignId]: state }));
  }
}

function createUuid(): string {
  return typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (character) => {
        const random = Math.floor(Math.random() * 16);
        return (character === "x" ? random : (random & 3) | 8).toString(16);
      });
}
