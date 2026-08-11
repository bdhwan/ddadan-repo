import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormBuilder, ReactiveFormsModule, Validators } from "@angular/forms";
import type {
  CatalogItemDto,
  LoyaltyPolicyDto,
  SaveLoyaltyPolicyRequestDto,
} from "@coupon/contracts";
import { formatKoreaDateTime, formatWon } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { CatalogApi } from "./catalog.api";
import { LoyaltyApi } from "./loyalty.api";

@Component({
  selector: "coupon-store-loyalty",
  imports: [
    ReactiveFormsModule,
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="도장 정책"
      description="정책 변경은 새 버전으로 만들어 기존 고객의 조건을 보호합니다."
      eyebrow="버전형 정책"
      ><coupon-button (click)="newVersion()"
        >새 버전 만들기</coupon-button
      ></coupon-page-header
    >
    @if (loading()) {
      <coupon-card
        ><coupon-skeleton [lines]="8" label="정책 버전을 불러오는 중입니다."
      /></coupon-card>
    } @else if (loadError()) {
      <coupon-error-state
        title="정책을 불러오지 못했어요"
        [description]="loadError()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else {
      <section class="version-grid" aria-label="정책 버전">
        <article class="version active">
          <div>
            <span aria-hidden="true">●</span><strong>현재 활성 버전</strong>
          </div>
          @if (active(); as p) {
            <h2>v{{ p.policy_version }} · {{ p.goal_stamps }}개 완성</h2>
            <p>
              주문당 {{ p.stamps_per_order }}개 · 도장
              {{ p.stamp_validity_days }}일 · 리워드
              {{ p.reward_validity_days }}일
            </p>
            <coupon-badge status="success" label="운영 중">운영 중</coupon-badge
            ><button type="button" (click)="copyActive()">
              이 정책으로 새 버전 만들기
            </button>
          } @else {
            <p>현재 활성 정책이 없습니다.</p>
          }
        </article>
        <article class="version scheduled">
          <div>
            <span aria-hidden="true">◷</span><strong>다음 예약 버전</strong>
          </div>
          @if (scheduled(); as p) {
            <h2>v{{ p.policy_version }} · {{ p.goal_stamps }}개 완성</h2>
            <p>{{ p.starts_at ? date(p.starts_at) : "전환 시각 확인 필요" }}</p>
            <coupon-badge status="warning" label="예약">예약</coupon-badge>
          } @else {
            <p>예약된 정책이 없습니다.</p>
          }
        </article>
        <article class="version history">
          <div><span aria-hidden="true">↶</span><strong>과거 버전</strong></div>
          <p>{{ past().length }}개 버전 · 조건 스냅샷 보존</p>
          @for (p of past().slice(0, 2); track p.id) {
            <small
              >v{{ p.policy_version }} · {{ p.status }} ·
              {{ p.goal_stamps }}개</small
            >
          }
        </article>
      </section>
      @if (active()) {
        <div class="immutable" role="note">
          <span aria-hidden="true">ⓘ</span>
          <p>
            <strong>활성 정책은 직접 수정할 수 없습니다.</strong>
            목표·만료·리워드 조건 변경은 새 버전을 만들고 미래 전환 시각부터
            적용하세요.
          </p>
        </div>
      }
      @if (editing()) {
        <section class="editor">
          <nav aria-label="정책 작성 단계">
            @for (label of stepLabels; track label; let i = $index) {
              <button
                type="button"
                [class.current]="step() === i"
                (click)="step.set(i)"
                [attr.aria-current]="step() === i ? 'step' : null"
              >
                <span>{{ i + 1 }}</span
                >{{ label }}
              </button>
            }
          </nav>
          <form [formGroup]="form" (ngSubmit)="nextOrSave()">
            <coupon-card>
              @switch (step()) {
                @case (0) {
                  <fieldset>
                    <legend>1. 목표와 적립 조건</legend>
                    <div class="fields">
                      <label
                        >목표 도장 수 <small>2~100</small
                        ><input
                          type="number"
                          formControlName="goal_stamps"
                          min="2"
                          max="100" /></label
                      ><label
                        >주문당 적립 <small>1~10</small
                        ><input
                          type="number"
                          formControlName="stamps_per_order"
                          min="1"
                          max="10" /></label
                      ><label
                        >최소 주문액 <small>0~100,000,000원</small
                        ><input
                          type="number"
                          formControlName="minimum_order_amount"
                          min="0"
                          max="100000000" /></label
                      ><label
                        >영업일당 횟수 <small>1~20</small
                        ><input
                          type="number"
                          formControlName="per_business_day_limit"
                          min="1"
                          max="20"
                          [disabled]="unlimited()"
                      /></label>
                    </div>
                    <label class="check"
                      ><input
                        type="checkbox"
                        [checked]="unlimited()"
                        (change)="unlimited.set(!unlimited())"
                      />영업일당 제한 없음</label
                    >
                  </fieldset>
                }
                @case (1) {
                  <fieldset>
                    <legend>2. 유효기간과 중복 제한</legend>
                    <div class="fields">
                      <label
                        >도장 유효기간 <small>1~730일</small
                        ><input
                          type="number"
                          formControlName="stamp_validity_days"
                          min="1"
                          max="730" /></label
                      ><label
                        >리워드 유효기간 <small>1~365일</small
                        ><input
                          type="number"
                          formControlName="reward_validity_days"
                          min="1"
                          max="365" /></label
                      ><label
                        >중복 경고 구간 <small>1~60분</small
                        ><input
                          type="number"
                          formControlName="duplicate_warning_minutes"
                          min="1"
                          max="60"
                      /></label>
                    </div>
                    <div class="expiry-example">
                      <span aria-hidden="true">◷</span>
                      <p>
                        오늘 적립 예시<br /><strong
                          >도장 만료 {{ stampExpiryExample() }}</strong
                        ><br />리워드 만료 {{ rewardExpiryExample() }}
                      </p>
                    </div>
                  </fieldset>
                }
                @case (2) {
                  <fieldset>
                    <legend>3. 리워드와 대상 품목</legend>
                    <label
                      >소비자에게 보일 리워드 내용<textarea
                        formControlName="reward_description"
                        rows="3"
                        maxlength="200"
                        placeholder="예: 아메리카노 1잔 무료"
                      ></textarea>
                    </label>
                    <div>
                      <strong>적립 대상 품목</strong>
                      <p class="muted">
                        선택하지 않으면 전체 품목입니다. 비활성 품목은 새
                        정책에서 선택할 수 없습니다.
                      </p>
                      <div class="item-choices">
                        @for (item of catalog(); track item.id) {
                          <label [class.inactive]="!item.active"
                            ><input
                              type="checkbox"
                              [checked]="eligible().has(item.id)"
                              [disabled]="!item.active"
                              (change)="toggleItem(item.id)"
                            /><span
                              >{{ item.name
                              }}<small>{{
                                item.active
                                  ? "선택 가능"
                                  : "비활성 · 기존 스냅샷만 유지"
                              }}</small></span
                            ></label
                          >
                        }
                      </div>
                    </div>
                  </fieldset>
                }
                @case (3) {
                  <fieldset>
                    <legend>4. 소비자 화면 미리보기와 게시</legend>
                    <div class="preview">
                      <div>
                        <span class="mark" aria-hidden="true">◆</span
                        ><strong>다단 상점</strong>
                      </div>
                      <p class="count">
                        {{ previewCurrent() }}/{{
                          form.controls.goal_stamps.value
                        }}
                      </p>
                      <div
                        class="stamp-board"
                        [attr.aria-label]="
                          '예시 도장판 ' +
                          previewCurrent() +
                          '/' +
                          form.controls.goal_stamps.value
                        "
                      >
                        @for (slot of previewSlots(); track $index) {
                          <span [class.filled]="slot" aria-hidden="true">{{
                            slot ? "✓" : "·"
                          }}</span>
                        }
                      </div>
                      <p>
                        <strong>리워드</strong>
                        {{
                          form.controls.reward_description.value ||
                            "리워드 내용을 입력해 주세요."
                        }}
                      </p>
                      <small
                        >가장 이른 도장 만료 {{ stampExpiryExample() }}</small
                      >
                    </div>
                    <div class="publish">
                      <label class="check"
                        ><input
                          type="radio"
                          name="publishMode"
                          [checked]="publishMode() === 'now'"
                          (change)="publishMode.set('now')"
                        />저장 후 즉시 게시</label
                      ><label class="check"
                        ><input
                          type="radio"
                          name="publishMode"
                          [checked]="publishMode() === 'scheduled'"
                          (change)="publishMode.set('scheduled')"
                        />다음 버전 예약</label
                      >
                      @if (publishMode() === "scheduled") {
                        <label
                          >전환 시각<input
                            type="datetime-local"
                            formControlName="publish_at"
                        /></label>
                      }
                    </div>
                  </fieldset>
                }
              }
            </coupon-card>
            @if (saveError()) {
              <p class="form-error" role="alert">{{ saveError() }}</p>
            }
            <div class="actions">
              <coupon-button
                variant="secondary"
                (click)="previous()"
                [disabled]="step() === 0"
                >이전</coupon-button
              >
              <div>
                <coupon-button
                  variant="secondary"
                  (click)="saveDraft()"
                  [disabled]="saving()"
                  >초안 저장</coupon-button
                ><coupon-button
                  type="submit"
                  [disabled]="saving() || form.invalid"
                  >{{ step() === 3 ? "검토 후 게시" : "다음" }}</coupon-button
                >
              </div>
            </div>
            <p class="status" aria-live="polite">{{ statusMessage() }}</p>
          </form>
        </section>
      }
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .version-grid {
      display: grid;
      gap: 0.75rem;
    }
    .version {
      position: relative;
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .version > div:first-child {
      display: flex;
      gap: 0.45rem;
      color: var(--coupon-color-text-muted);
    }
    .version.active {
      border-top: 4px solid var(--coupon-color-success);
    }
    .version.scheduled {
      border-top: 4px solid var(--coupon-color-warning);
    }
    .version.history {
      border-top: 4px solid var(--coupon-color-border);
    }
    .version h2 {
      margin: 0.7rem 0 0.25rem;
      font-size: 1.15rem;
    }
    .version p {
      color: var(--coupon-color-text-muted);
    }
    .version button {
      display: block;
      min-height: 44px;
      margin-top: 0.7rem;
      border: 0;
      background: transparent;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    .version small {
      display: block;
    }
    .immutable {
      display: grid;
      grid-template-columns: 2rem 1fr;
      gap: 0.5rem;
      margin: 1rem 0;
      padding: 0.75rem;
      border: 1px solid var(--coupon-color-primary);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-surface);
    }
    .immutable p {
      margin: 0;
    }
    .editor {
      display: grid;
      gap: 1rem;
      margin-top: 1.5rem;
    }
    .editor nav {
      display: grid;
      grid-template-columns: repeat(4, 1fr);
      overflow-x: auto;
    }
    .editor nav button {
      display: grid;
      justify-items: center;
      min-width: 7rem;
      min-height: 54px;
      border: 0;
      border-bottom: 3px solid var(--coupon-color-border);
      background: transparent;
      color: var(--coupon-color-text-muted);
      font-weight: 800;
    }
    .editor nav button.current {
      border-color: var(--coupon-color-primary);
      color: var(--coupon-color-primary);
    }
    .editor nav span {
      display: grid;
      place-items: center;
      width: 1.5rem;
      height: 1.5rem;
      border: 1px solid;
      border-radius: 50%;
    }
    form {
      display: grid;
      gap: 1rem;
    }
    fieldset {
      display: grid;
      gap: 1rem;
      margin: 0;
      padding: 0;
      border: 0;
    }
    legend {
      margin-bottom: 0.7rem;
      font-size: 1.2rem;
      font-weight: 900;
    }
    .fields {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0.75rem;
    }
    label {
      display: grid;
      gap: 0.3rem;
      font-weight: 800;
    }
    label small,
    .muted {
      color: var(--coupon-color-text-muted);
      font-weight: 400;
    }
    input,
    textarea {
      width: 100%;
      min-height: 44px;
      padding: 0.6rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .check {
      grid-template-columns: 32px 1fr;
      align-items: center;
      min-height: 44px;
    }
    .check input {
      width: 22px;
      height: 22px;
    }
    .expiry-example {
      display: grid;
      grid-template-columns: 3rem 1fr;
      align-items: center;
      padding: 0.75rem;
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-surface-muted);
    }
    .expiry-example > span {
      font-size: 1.7rem;
    }
    .expiry-example p {
      margin: 0;
    }
    .item-choices {
      display: grid;
      gap: 0.5rem;
    }
    .item-choices label {
      grid-template-columns: 32px 1fr;
      align-items: center;
      min-height: 54px;
      padding: 0.5rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
    }
    .item-choices input {
      width: 22px;
      height: 22px;
    }
    .item-choices span {
      display: grid;
    }
    .item-choices .inactive {
      opacity: 0.65;
    }
    .preview {
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-bg);
    }
    .preview > div:first-child {
      display: flex;
      gap: 0.5rem;
    }
    .mark {
      color: var(--coupon-color-primary);
    }
    .count {
      margin: 0.8rem 0 0.25rem;
      font-size: 1.8rem;
      font-weight: 900;
    }
    .stamp-board {
      display: flex;
      flex-wrap: wrap;
      gap: 0.35rem;
      margin-bottom: 1rem;
    }
    .stamp-board span {
      display: grid;
      place-items: center;
      width: 2rem;
      height: 2rem;
      border: 1px dashed var(--coupon-color-border);
      border-radius: 50%;
    }
    .stamp-board .filled {
      border-style: solid;
      background: var(--coupon-color-primary);
      color: var(--coupon-color-on-primary);
    }
    .publish {
      display: grid;
      gap: 0.5rem;
      padding-top: 1rem;
    }
    .actions {
      display: flex;
      justify-content: space-between;
      gap: 0.5rem;
    }
    .actions > div {
      display: flex;
      gap: 0.5rem;
    }
    .form-error {
      color: var(--coupon-color-danger);
    }
    .status {
      text-align: right;
      color: var(--coupon-color-text-muted);
    }
    @media (min-width: 768px) {
      .version-grid {
        grid-template-columns: repeat(3, 1fr);
      }
      .editor {
        grid-template-columns: 13rem minmax(0, 1fr);
        align-items: start;
      }
      .editor nav {
        position: sticky;
        top: 5rem;
        grid-template-columns: 1fr;
      }
      .editor nav button {
        grid-template-columns: 2rem 1fr;
        justify-items: start;
        align-items: center;
        border-bottom: 0;
        border-left: 3px solid var(--coupon-color-border);
      }
      .fields {
        grid-template-columns: repeat(3, 1fr);
      }
      .item-choices {
        grid-template-columns: repeat(2, 1fr);
      }
    }
  `,
})
export class LoyaltyComponent {
  private readonly api = inject(LoyaltyApi);
  private readonly catalogApi = inject(CatalogApi);
  private readonly fb = inject(FormBuilder);
  private readonly destroyRef = inject(DestroyRef);
  readonly stepLabels = ["적립 조건", "유효기간", "리워드·품목", "검토·게시"];
  readonly policies = signal<LoyaltyPolicyDto[]>([]);
  readonly catalog = signal<CatalogItemDto[]>([]);
  readonly loading = signal(true);
  readonly loadError = signal<string | null>(null);
  readonly editing = signal(false);
  readonly step = signal(0);
  readonly selected = signal<LoyaltyPolicyDto | null>(null);
  readonly eligible = signal<ReadonlySet<string>>(new Set());
  readonly unlimited = signal(false);
  readonly saving = signal(false);
  readonly saveError = signal<string | null>(null);
  readonly statusMessage = signal("");
  readonly publishMode = signal<"now" | "scheduled">("now");
  readonly active = computed(
    () => this.policies().find((p) => p.status === "ACTIVE") ?? null,
  );
  readonly scheduled = computed(
    () => this.policies().find((p) => p.status === "SCHEDULED") ?? null,
  );
  readonly past = computed(() =>
    this.policies().filter(
      (p) => p.status === "ENDED" || p.status === "PAUSED",
    ),
  );
  readonly form = this.fb.nonNullable.group({
    goal_stamps: [
      10,
      [Validators.required, Validators.min(2), Validators.max(100)],
    ],
    stamps_per_order: [
      1,
      [Validators.required, Validators.min(1), Validators.max(10)],
    ],
    minimum_order_amount: [
      0,
      [Validators.required, Validators.min(0), Validators.max(100_000_000)],
    ],
    per_business_day_limit: [
      1,
      [Validators.required, Validators.min(1), Validators.max(20)],
    ],
    stamp_validity_days: [
      180,
      [Validators.required, Validators.min(1), Validators.max(730)],
    ],
    reward_validity_days: [
      30,
      [Validators.required, Validators.min(1), Validators.max(365)],
    ],
    duplicate_warning_minutes: [
      5,
      [Validators.required, Validators.min(1), Validators.max(60)],
    ],
    reward_description: ["", [Validators.required, Validators.maxLength(200)]],
    publish_at: [""],
  });
  constructor() {
    this.load();
  }
  load(): void {
    this.loading.set(true);
    this.loadError.set(null);
    this.api
      .list()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (r) => {
          this.policies.set(r.items);
          this.loading.set(false);
        },
        error: () => {
          this.loadError.set("서버 연결을 확인해 주세요.");
          this.loading.set(false);
        },
      });
    this.catalogApi
      .list()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (r) => this.catalog.set(r.items),
        error: () => this.catalog.set([]),
      });
  }
  newVersion(): void {
    this.selected.set(null);
    this.form.reset({
      goal_stamps: 10,
      stamps_per_order: 1,
      minimum_order_amount: 0,
      per_business_day_limit: 1,
      stamp_validity_days: 180,
      reward_validity_days: 30,
      duplicate_warning_minutes: 5,
      reward_description: "",
      publish_at: "",
    });
    this.eligible.set(new Set());
    this.unlimited.set(false);
    this.step.set(0);
    this.editing.set(true);
  }
  copyActive(): void {
    const p = this.active();
    if (!p) {
      this.newVersion();
      return;
    }
    this.selected.set(null);
    this.applyPolicy(p);
    this.editing.set(true);
    this.step.set(0);
    this.statusMessage.set(
      "활성 정책을 복사했습니다. 저장하면 새 버전이 됩니다.",
    );
  }
  applyPolicy(p: LoyaltyPolicyDto): void {
    this.form.reset({
      goal_stamps: p.goal_stamps,
      stamps_per_order: p.stamps_per_order,
      minimum_order_amount: p.minimum_order_amount.amount,
      per_business_day_limit: p.per_business_day_limit ?? 1,
      stamp_validity_days: p.stamp_validity_days,
      reward_validity_days: p.reward_validity_days,
      duplicate_warning_minutes: p.duplicate_warning_minutes,
      reward_description: p.reward_description,
      publish_at: "",
    });
    this.unlimited.set(p.per_business_day_limit === null);
    this.eligible.set(new Set(p.eligible_catalog_item_ids));
  }
  toggleItem(id: string): void {
    this.eligible.update((current) => {
      const next = new Set(current);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }
  previous(): void {
    this.step.update((v) => Math.max(0, v - 1));
  }
  nextOrSave(): void {
    if (this.step() < 3) {
      this.step.update((v) => v + 1);
      return;
    }
    this.publish();
  }
  saveDraft(): void {
    if (this.form.invalid) {
      this.form.markAllAsTouched();
      this.saveError.set("허용 범위와 필수 항목을 확인해 주세요.");
      return;
    }
    this.saving.set(true);
    this.saveError.set(null);
    const draft = this.selected();
    const call =
      draft?.status === "DRAFT"
        ? this.api.update(draft.id, {
            ...this.payload(),
            version: draft.version,
          })
        : this.api.create(this.payload());
    call.pipe(takeUntilDestroyed(this.destroyRef)).subscribe({
      next: (p) => {
        this.upsert(p);
        this.selected.set(p);
        this.saving.set(false);
        this.statusMessage.set(`v${p.policy_version} 초안을 저장했습니다.`);
      },
      error: () => {
        this.saveError.set("초안을 저장하지 못했습니다.");
        this.saving.set(false);
      },
    });
  }
  publish(): void {
    if (!this.selected() || this.selected()!.status !== "DRAFT") {
      this.saveError.set("게시 전에 초안을 먼저 저장해 주세요.");
      return;
    }
    if (
      this.publishMode() === "scheduled" &&
      !this.form.controls.publish_at.value
    ) {
      this.saveError.set("예약 전환 시각을 입력해 주세요.");
      return;
    }
    this.saving.set(true);
    const raw = this.form.controls.publish_at.value;
    const publishAt =
      this.publishMode() === "now" ? null : new Date(raw).toISOString();
    this.api
      .publish(this.selected()!.id, {
        publish_at: publishAt,
        version: this.selected()!.version,
      })
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (p) => {
          this.upsert(p);
          this.selected.set(p);
          this.saving.set(false);
          this.editing.set(false);
          this.statusMessage.set(
            this.publishMode() === "now"
              ? "새 정책을 게시했습니다."
              : "새 정책을 예약했습니다.",
          );
        },
        error: () => {
          this.saveError.set("정책을 게시하지 못했습니다.");
          this.saving.set(false);
        },
      });
  }
  payload(): SaveLoyaltyPolicyRequestDto {
    const v = this.form.getRawValue();
    return {
      goal_stamps: v.goal_stamps,
      stamps_per_order: v.stamps_per_order,
      minimum_order_amount: { amount: v.minimum_order_amount, currency: "KRW" },
      per_business_day_limit: this.unlimited()
        ? null
        : v.per_business_day_limit,
      stamp_validity_days: v.stamp_validity_days,
      reward_validity_days: v.reward_validity_days,
      duplicate_warning_minutes: v.duplicate_warning_minutes,
      reward_description: v.reward_description,
      eligible_catalog_item_ids: [...this.eligible()],
    };
  }
  upsert(p: LoyaltyPolicyDto): void {
    this.policies.update((items) => [...items.filter((x) => x.id !== p.id), p]);
  }
  previewCurrent(): number {
    return Math.max(0, this.form.controls.goal_stamps.value - 2);
  }
  previewSlots(): boolean[] {
    const goal = this.form.controls.goal_stamps.value;
    const shown = Math.min(goal, 12);
    return Array.from(
      { length: shown },
      (_, i) => i < Math.min(this.previewCurrent(), shown),
    );
  }
  stampExpiryExample(): string {
    return this.example(this.form.controls.stamp_validity_days.value);
  }
  rewardExpiryExample(): string {
    return this.example(this.form.controls.reward_validity_days.value);
  }
  example(days: number): string {
    const d = new Date();
    d.setDate(d.getDate() + days);
    return formatKoreaDateTime(d);
  }
  date(v: string): string {
    return formatKoreaDateTime(v);
  }
  won(v: number): string {
    return formatWon(v);
  }
}
