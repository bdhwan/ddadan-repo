import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  inject,
  signal,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormBuilder, ReactiveFormsModule, Validators } from "@angular/forms";
import type { CatalogItemDto } from "@coupon/contracts";
import { formatWon } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponEmptyStateComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import { CatalogApi } from "./catalog.api";

@Component({
  selector: "coupon-store-catalog",
  imports: [
    ReactiveFormsModule,
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
    <coupon-page-header
      title="품목 관리"
      description="정책의 품목 조건에 사용할 상점 내부 품목을 관리합니다."
      eyebrow="상품 마스터"
      ><coupon-button (click)="newItem()"
        >품목 추가</coupon-button
      ></coupon-page-header
    >
    <div class="notice" role="note">
      <span aria-hidden="true">ⓘ</span>
      <p>
        <strong>가격은 참고값입니다.</strong> 실제 적립·할인 계산은 현장
        거래에서 입력한 금액을 사용합니다. 비활성 품목은 새 정책에서 선택할 수
        없습니다.
      </p>
    </div>
    @if (loading()) {
      <coupon-card
        ><coupon-skeleton [lines]="7" label="품목을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error()) {
      <coupon-error-state
        title="품목을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else {
      <div class="layout">
        <section>
          @if (items().length === 0) {
            <coupon-empty-state
              title="등록한 품목이 없어요"
              description="첫 품목을 추가하면 도장·쿠폰 정책에서 선택할 수 있습니다."
            />
          } @else {
            <div class="table-wrap">
              <table>
                <caption class="sr-only">
                  상점 품목 목록
                </caption>
                <thead>
                  <tr>
                    <th>품목명</th>
                    <th>SKU</th>
                    <th>카테고리</th>
                    <th>참고 가격</th>
                    <th>상태</th>
                    <th><span class="sr-only">작업</span></th>
                  </tr>
                </thead>
                <tbody>
                  @for (item of items(); track item.id) {
                    <tr>
                      <td>
                        <strong>{{ item.name }}</strong>
                      </td>
                      <td>{{ item.sku ?? "—" }}</td>
                      <td>{{ item.category }}</td>
                      <td>
                        {{
                          item.reference_price
                            ? won(item.reference_price.amount)
                            : "미설정"
                        }}
                      </td>
                      <td>
                        <coupon-badge
                          [status]="item.active ? 'success' : 'neutral'"
                          [label]="item.active ? '활성' : '비활성'"
                          >{{ item.active ? "활성" : "비활성" }}</coupon-badge
                        >
                      </td>
                      <td>
                        <button type="button" (click)="edit(item)">편집</button>
                      </td>
                    </tr>
                  }
                </tbody>
              </table>
            </div>
          }
        </section>
        @if (editing()) {
          <aside>
            <coupon-card
              ><form [formGroup]="form" (ngSubmit)="save()">
                <h2>{{ selected() ? "품목 편집" : "새 품목" }}</h2>
                <label
                  >품목명<input formControlName="name" maxlength="80" /></label
                ><label
                  >내부 SKU <small>(선택)</small
                  ><input
                    formControlName="sku"
                    maxlength="50"
                    autocomplete="off" /></label
                ><label
                  >카테고리<input
                    formControlName="category"
                    maxlength="50" /></label
                ><label
                  >참고 가격
                  <div class="won">
                    <input
                      type="number"
                      formControlName="reference_price"
                      min="0"
                      max="100000000"
                    /><span>원</span>
                  </div></label
                ><label class="check"
                  ><input type="checkbox" formControlName="active" /><span
                    >활성 품목</span
                  ></label
                >
                @if (saveError()) {
                  <p class="error" role="alert">{{ saveError() }}</p>
                }
                <div class="actions">
                  <coupon-button variant="secondary" (click)="cancel()"
                    >취소</coupon-button
                  ><coupon-button
                    type="submit"
                    [disabled]="form.invalid || saving()"
                    >{{ saving() ? "저장 중…" : "저장" }}</coupon-button
                  >
                </div>
              </form></coupon-card
            >
          </aside>
        }
      </div>
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .notice {
      display: grid;
      grid-template-columns: 2rem 1fr;
      gap: 0.5rem;
      margin-bottom: 1rem;
      padding: 0.75rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-surface);
    }
    .notice p {
      margin: 0;
    }
    .table-wrap {
      overflow-x: auto;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    table {
      width: 100%;
      min-width: 720px;
      border-collapse: collapse;
    }
    th,
    td {
      padding: 0.75rem;
      border-bottom: 1px solid var(--coupon-color-border);
      text-align: left;
    }
    th {
      background: var(--coupon-color-surface-muted);
    }
    td button {
      min-width: 44px;
      min-height: 44px;
      border: 0;
      background: transparent;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    .layout {
      display: grid;
      gap: 1rem;
    }
    form {
      display: grid;
      gap: 0.8rem;
    }
    form h2 {
      margin: 0;
    }
    label {
      display: grid;
      gap: 0.3rem;
      font-weight: 800;
    }
    input {
      min-height: 44px;
      padding: 0.6rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .won {
      display: grid;
      grid-template-columns: 1fr 3rem;
    }
    .won input {
      border-radius: 0.5rem 0 0 0.5rem;
    }
    .won span {
      display: grid;
      place-items: center;
      border: 1px solid var(--coupon-color-border);
      border-left: 0;
      border-radius: 0 0.5rem 0.5rem 0;
    }
    .check {
      grid-template-columns: 44px 1fr;
      align-items: center;
      min-height: 44px;
    }
    .check input {
      width: 22px;
      height: 22px;
    }
    .actions {
      display: flex;
      justify-content: flex-end;
      gap: 0.5rem;
    }
    .error {
      color: var(--coupon-color-danger);
    }
    .sr-only {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0, 0, 0, 0);
    }
    @media (min-width: 768px) {
      .layout:has(aside) {
        grid-template-columns: minmax(0, 2fr) minmax(19rem, 1fr);
        align-items: start;
      }
      aside {
        position: sticky;
        top: 5rem;
      }
    }
  `,
})
export class CatalogComponent {
  private readonly api = inject(CatalogApi);
  private readonly fb = inject(FormBuilder);
  private readonly destroyRef = inject(DestroyRef);
  readonly items = signal<CatalogItemDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly saving = signal(false);
  readonly saveError = signal<string | null>(null);
  readonly editing = signal(false);
  readonly selected = signal<CatalogItemDto | null>(null);
  readonly form = this.fb.nonNullable.group({
    name: ["", [Validators.required, Validators.maxLength(80)]],
    sku: ["", Validators.maxLength(50)],
    category: ["", [Validators.required, Validators.maxLength(50)]],
    reference_price: [0, [Validators.min(0), Validators.max(100_000_000)]],
    active: [true],
  });
  constructor() {
    this.load();
  }
  load(): void {
    this.loading.set(true);
    this.error.set(null);
    this.api
      .list()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (r) => {
          this.items.set(r.items);
          this.loading.set(false);
        },
        error: () => {
          this.error.set("서버 연결을 확인해 주세요.");
          this.loading.set(false);
        },
      });
  }
  newItem(): void {
    this.selected.set(null);
    this.form.reset({
      name: "",
      sku: "",
      category: "",
      reference_price: 0,
      active: true,
    });
    this.editing.set(true);
  }
  edit(item: CatalogItemDto): void {
    this.selected.set(item);
    this.form.reset({
      name: item.name,
      sku: item.sku ?? "",
      category: item.category,
      reference_price: item.reference_price?.amount ?? 0,
      active: item.active,
    });
    this.editing.set(true);
  }
  cancel(): void {
    this.editing.set(false);
    this.selected.set(null);
  }
  save(): void {
    if (this.form.invalid) return;
    this.saving.set(true);
    this.saveError.set(null);
    const v = this.form.getRawValue();
    const payload = {
      name: v.name,
      sku: v.sku || null,
      category: v.category,
      active: v.active,
      reference_price: { amount: v.reference_price, currency: "KRW" as const },
      ...(this.selected() ? { version: this.selected()!.version } : {}),
    };
    const call = this.selected()
      ? this.api.update(this.selected()!.id, payload)
      : this.api.create(payload);
    call.pipe(takeUntilDestroyed(this.destroyRef)).subscribe({
      next: (item) => {
        this.items.update((items) =>
          [...items.filter((x) => x.id !== item.id), item].sort((a, b) =>
            a.name.localeCompare(b.name, "ko"),
          ),
        );
        this.saving.set(false);
        this.cancel();
      },
      error: () => {
        this.saveError.set("품목을 저장하지 못했습니다.");
        this.saving.set(false);
      },
    });
  }
  won(value: number): string {
    return formatWon(value);
  }
}
