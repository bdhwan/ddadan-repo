import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  HostListener,
  OnInit,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormsModule } from "@angular/forms";
import type { CampaignStatus, OwnerCampaignDto } from "@coupon/contracts";
import { AuthSessionService, visibilityAwarePoll } from "@coupon/client-core";
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
  CAMPAIGN_WIZARD_STEPS,
  IMMUTABLE_AFTER_ISSUANCE,
  createCampaignDraft,
  estimatedNotificationCount,
  maximumCampaignExposure,
  previewCampaignDiscount,
  toSaveCampaignRequest,
  validateCampaignStep,
  type CampaignDraft,
  type CampaignWizardStep,
} from "./campaign-wizard";
import { CampaignsApi } from "./campaigns.api";

type CampaignAction = "pause" | "cancel" | "revoke";

@Component({
  selector: "coupon-store-campaign-progress",
  imports: [
    FormsModule,
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
      title="할인 캠페인"
      description="발급 기간과 사용 기간, 대상 스냅샷과 처리 수를 각각 확인합니다."
      eyebrow="Campaigns"
    >
      <coupon-button (click)="startWizard()">새 캠페인</coupon-button>
    </coupon-page-header>

    @if (mode() === "wizard") {
      <section class="wizard" aria-labelledby="wizard-title">
        <div class="wizard-head">
          <div>
            <p class="eyebrow">새 캠페인 작성</p>
            <h2 #wizardHeading id="wizard-title" tabindex="-1">
              {{ stepLabel(step()) }}
            </h2>
          </div>
          <button type="button" class="plain-button" (click)="closeWizard()">
            목록으로
          </button>
        </div>
        <ol class="wizard-steps" aria-label="캠페인 작성 단계">
          @for (candidate of steps; track candidate; let index = $index) {
            <li [class.current]="candidate === step()">
              <span>{{ index + 1 }}</span
              >{{ stepLabel(candidate) }}
            </li>
          }
        </ol>

        <coupon-card>
          @switch (step()) {
            @case ("benefit") {
              <fieldset>
                <legend>1. 혜택</legend>
                <label
                  >캠페인 이름<input [(ngModel)]="draft.name" maxlength="80"
                /></label>
                <label
                  >할인 유형
                  <select [(ngModel)]="draft.benefit_type">
                    <option value="FIXED">정액 할인</option>
                    <option value="PERCENTAGE">정률 할인</option>
                    <option value="FREE_ITEM">무료 품목</option>
                  </select>
                </label>
                @if (draft.benefit_type === "FIXED") {
                  <label
                    >할인액<input
                      type="number"
                      min="1"
                      step="1"
                      [(ngModel)]="draft.fixed_discount_amount"
                  /></label>
                } @else if (draft.benefit_type === "PERCENTAGE") {
                  <div class="two-cols">
                    <label
                      >할인율 (%)<input
                        type="number"
                        min="1"
                        max="100"
                        step="1"
                        [(ngModel)]="draft.percentage"
                    /></label>
                    <label
                      >최대 할인액<input
                        type="number"
                        min="1"
                        step="1"
                        [(ngModel)]="draft.maximum_discount_amount"
                    /></label>
                  </div>
                  <p class="note">
                    예시: 9,999원 주문 결과 {{ won(sampleDiscount()) }}. 1원
                    미만은 버리고 최대 할인액을 적용합니다.
                  </p>
                } @else {
                  <label
                    >무료 품목 ID (쉼표로 구분)<input
                      [ngModel]="draft.free_item_ids.join(', ')"
                      (ngModelChange)="draft.free_item_ids = splitIds($event)"
                  /></label>
                  <p class="note">
                    주문에 대상 품목이 여러 개면 가장 낮은 실제 단가 1개를 무료
                    처리합니다.
                  </p>
                }
              </fieldset>
            }
            @case ("conditions") {
              <fieldset>
                <legend>2. 사용 조건</legend>
                <label
                  >최소 주문액<input
                    type="number"
                    min="0"
                    step="1"
                    [(ngModel)]="draft.minimum_order_amount"
                /></label>
                <label
                  >대상 품목 ID<input
                    [ngModel]="draft.eligible_item_ids.join(', ')"
                    (ngModelChange)="
                      draft.eligible_item_ids = splitIds($event)
                    "
                /></label>
                <label
                  >제외 품목 ID<input
                    [ngModel]="draft.excluded_item_ids.join(', ')"
                    (ngModelChange)="
                      draft.excluded_item_ids = splitIds($event)
                    "
                /></label>
                <p class="note">
                  품목 정보가 없는 주문은 품목 제한 쿠폰을 승인할 수 없습니다.
                  대상과 제외가 겹치면 제외이 우선입니다.
                </p>
              </fieldset>
            }
            @case ("audience") {
              <fieldset>
                <legend>3. 대상</legend>
                <label
                  >대상 집합<select [(ngModel)]="draft.audience_type">
                    <option value="ALL_FAVORITES">관심 상점 전체</option>
                    <option value="SEGMENT">세그먼트</option>
                    <option value="SPECIFIC_CUSTOMERS" disabled>
                      특정 고객 · 고객 ID 입력 기능 준비 중
                    </option>
                  </select></label
                >
                <label
                  >발급 방식<select [(ngModel)]="draft.issuance_method">
                    <option value="FIRST_COME">공개 선착순</option>
                    <option value="TARGETED">대상 일괄 지급</option>
                    <option value="DIRECT">특정 고객 직접 지급</option>
                  </select></label
                >
                <label
                  >예상 대상 수<input
                    type="number"
                    min="0"
                    step="1"
                    [(ngModel)]="draft.estimated_audience"
                /></label>
              </fieldset>
            }
            @case ("quantity") {
              <fieldset>
                <legend>4. 수량</legend>
                <label class="check"
                  ><input
                    type="checkbox"
                    [checked]="draft.total_quantity === null"
                    (change)="toggleUnlimited($event)"
                  />운영 상한 내 무제한</label
                >
                @if (draft.total_quantity !== null) {
                  <label
                    >총 발급 수량<input
                      type="number"
                      min="1"
                      step="1"
                      [(ngModel)]="draft.total_quantity"
                  /></label>
                }
                <div class="two-cols">
                  <label
                    >회원별 누적 수량<input
                      type="number"
                      min="1"
                      step="1"
                      [(ngModel)]="draft.per_user_quantity" /></label
                  ><label
                    >영업일별 수량 (0은 제한 없음)<input
                      type="number"
                      min="0"
                      step="1"
                      [ngModel]="draft.per_business_day_quantity ?? 0"
                      (ngModelChange)="
                        draft.per_business_day_quantity = $event || null
                      "
                  /></label>
                </div>
                <label class="check"
                  ><input
                    type="checkbox"
                    [(ngModel)]="draft.restore_quantity_on_revoke"
                  />회수 시 발급 수량 복원</label
                >
              </fieldset>
            }
            @case ("schedule") {
              <fieldset>
                <legend>5. 일정</legend>
                <div class="two-cols">
                  <label
                    >발급 시작<input
                      type="datetime-local"
                      [(ngModel)]="draft.issuance_starts_at" /></label
                  ><label
                    >발급 종료 (미포함)<input
                      type="datetime-local"
                      [(ngModel)]="draft.issuance_ends_at"
                  /></label>
                </div>
                <div class="two-cols">
                  <label
                    >사용 시작<input
                      type="datetime-local"
                      [(ngModel)]="draft.usable_from" /></label
                  ><label
                    >사용 종료 (미포함)<input
                      type="datetime-local"
                      [(ngModel)]="draft.usable_until"
                  /></label>
                </div>
                <p class="note" role="note">
                  모든 기간은 [시작, 종료)입니다. 사용 종료 시각 정각부터는
                  사용할 수 없습니다.
                </p>
              </fieldset>
            }
            @case ("notification") {
              <fieldset>
                <legend>6. 알림</legend>
                <label class="check"
                  ><input type="checkbox" [(ngModel)]="draft.notify_in_app" />앱
                  내 알림</label
                ><label class="check"
                  ><input
                    type="checkbox"
                    [(ngModel)]="draft.notify_push"
                  />마케팅 동의 고객에게 푸시 알림</label
                >
                <p class="note">알림을 끄더라도 쿠폰 발급은 정상 진행됩니다.</p>
              </fieldset>
            }
            @case ("review") {
              <fieldset>
                <legend>7. 검토</legend>
                <div class="review-grid">
                  <div>
                    <span>최대 할인 노출액</span
                    ><strong>{{
                      exposure() === null
                        ? "실제 품목 단가·운영 상한에 따라 확정"
                        : won(exposure()!)
                    }}</strong>
                  </div>
                  <div>
                    <span>예상 대상</span
                    ><strong>{{ draft.estimated_audience }}명</strong>
                  </div>
                  <div>
                    <span>예상 알림량</span
                    ><strong>{{ notificationCount() }}건</strong>
                  </div>
                  <div>
                    <span>9,999원 주문 예상 할인</span
                    ><strong>{{
                      draft.benefit_type === "FREE_ITEM"
                        ? "실제 품목 단가"
                        : won(sampleDiscount())
                    }}</strong>
                  </div>
                </div>
                <section class="immutable" aria-labelledby="immutable-title">
                  <h3 id="immutable-title">최초 발급 후 변경할 수 없는 항목</h3>
                  <ul>
                    @for (item of immutableFields; track item) {
                      <li>{{ item }}</li>
                    }
                  </ul>
                  <p>이미 발급된 쿠폰은 조건 스냅샷을 계속 사용합니다.</p>
                </section>
              </fieldset>
            }
          }

          @if (validationErrors().length) {
            <div #validationError class="errors" role="alert" tabindex="-1">
              <strong>확인해 주세요</strong>
              <ul>
                @for (message of validationErrors(); track message) {
                  <li>{{ message }}</li>
                }
              </ul>
            </div>
          }
          <div class="wizard-actions">
            <coupon-button
              variant="secondary"
              [disabled]="stepIndex() === 0"
              (click)="previousStep()"
              >이전</coupon-button
            >
            @if (step() === "review") {
              <coupon-button [disabled]="saving()" (click)="saveCampaign()">{{
                saving() ? "저장 중…" : "초안 저장"
              }}</coupon-button>
            } @else {
              <coupon-button (click)="nextStep()">다음</coupon-button>
            }
          </div>
        </coupon-card>
      </section>
    } @else if (loading() && !version()) {
      <coupon-card
        ><coupon-skeleton [lines]="8" label="캠페인 목록을 불러오는 중입니다."
      /></coupon-card>
    } @else if (error() && !version()) {
      <coupon-error-state
        title="캠페인을 불러오지 못했어요"
        [description]="error()!"
        [retryable]="true"
        (retry)="load()"
      />
    } @else if (items().length === 0) {
      <coupon-empty-state
        title="캠페인이 없어요"
        description="7단계 마법사로 첫 할인 캠페인을 만들어 보세요."
      />
    } @else {
      <div class="campaign-list">
        @for (campaign of items(); track campaign.id) {
          <article>
            <div class="campaign-title">
              <div>
                <coupon-badge
                  [status]="badge(campaign.status)"
                  [label]="statusLabel(campaign.status)"
                  >{{ statusLabel(campaign.status) }}</coupon-badge
                >
                <h2>{{ campaign.name }}</h2>
                <p>
                  {{ issuanceLabel(campaign.issuance_method) }} ·
                  {{ campaign.benefit_label }}
                </p>
              </div>
              <time>{{ date(campaign.updated_at) }}</time>
            </div>
            <dl class="campaign-facts">
              <div>
                <dt>발급 기간</dt>
                <dd>
                  {{ date(campaign.issuance_starts_at) }} ~
                  {{ date(campaign.issuance_ends_at) }}
                  <strong>(종료 미포함)</strong>
                </dd>
              </div>
              <div>
                <dt>사용 기간</dt>
                <dd>
                  {{ date(campaign.usable_from) }} ~
                  {{ date(campaign.usable_until) }}
                  <strong>(종료 미포함)</strong>
                </dd>
              </div>
              <div>
                <dt>발급</dt>
                <dd>
                  {{ campaign.issued_count }} /
                  {{ campaign.total_quantity ?? "운영 상한 내 무제한" }}
                </dd>
              </div>
              <div>
                <dt>사용</dt>
                <dd>{{ campaign.used_count }}건</dd>
              </div>
            </dl>
            @if (campaign.status === "ISSUING") {
              <section class="progress-section" aria-label="발급 진행 상세">
                <div>
                  <span>대상 스냅샷 확정</span
                  ><strong>{{
                    campaign.snapshot_target_count === null
                      ? "확정 중"
                      : campaign.snapshot_target_count + "명"
                  }}</strong>
                </div>
                <div>
                  <span>처리 완료</span
                  ><strong>{{ campaign.processed_count }}명</strong>
                </div>
                @if (campaign.snapshot_target_count !== null) {
                  <div
                    class="progress"
                    role="progressbar"
                    [attr.aria-valuenow]="progress(campaign)"
                    aria-valuemin="0"
                    aria-valuemax="100"
                  >
                    <span [style.width.%]="progress(campaign)"></span>
                  </div>
                }
              </section>
            }
            <div class="campaign-actions">
              @if (campaign.status === "DRAFT") {
                <coupon-button (click)="beginPublish(campaign)"
                  >게시 검토·재인증</coupon-button
                >
              }
              @if (
                ["ISSUING", "ACTIVE", "SCHEDULED"].includes(campaign.status)
              ) {
                <coupon-button
                  variant="secondary"
                  (click)="beginAction(campaign, 'pause')"
                  >일시 중지</coupon-button
                >
              }
              @if (campaign.status === "PAUSED") {
                <coupon-button
                  variant="secondary"
                  [disabled]="saving()"
                  (click)="resume(campaign)"
                  >같은 작업 키·체크포인트로 재개</coupon-button
                >
              }
              @if (!["ENDED", "CANCELLED"].includes(campaign.status)) {
                <coupon-button
                  variant="secondary"
                  (click)="beginQuantityEdit(campaign)"
                  >수량 수정</coupon-button
                ><coupon-button
                  variant="secondary"
                  (click)="beginAction(campaign, 'cancel')"
                  >캠페인 취소</coupon-button
                ><coupon-button
                  variant="secondary"
                  (click)="beginAction(campaign, 'revoke')"
                  >발급분 회수</coupon-button
                >
              }
            </div>
          </article>
        }
      </div>
    }

    @if (actionTarget(); as target) {
      <section
        class="action-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="action-title"
      >
        <h2 #actionHeading id="action-title" tabindex="-1">
          {{ actionTitle() }}
        </h2>
        <p class="danger-note">
          <strong>영향 요약</strong> 신규 발급이 중지되며,
          {{ target.issued_count }}건의 발급분과 {{ target.used_count }}건의
          사용 기록은 유지됩니다.
          @if (action() === "revoke") {
            미사용 발급분을 회수하며 이 작업은 자동으로 되돌릴 수 없습니다.
          }
        </p>
        <label
          >사유<textarea [(ngModel)]="actionReason" rows="3"></textarea>
        </label>
        <label
          >확인하려면 <strong>{{ confirmationPhrase() }}</strong
          >을 입력하세요<input [(ngModel)]="confirmation" autocomplete="off"
        /></label>
        @if (actionError()) {
          <p class="errors" role="alert">{{ actionError() }}</p>
        }
        <div class="wizard-actions">
          <coupon-button variant="secondary" (click)="cancelAction()"
            >돌아가기</coupon-button
          ><coupon-button
            [disabled]="
              confirmation !== confirmationPhrase() ||
              !actionReason.trim() ||
              saving()
            "
            (click)="confirmAction()"
            >영향을 이해했으며 진행</coupon-button
          >
        </div>
      </section>
    }

    @if (quantityTarget(); as target) {
      <section
        class="action-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="quantity-title"
      >
        <h2 #quantityHeading id="quantity-title" tabindex="-1">
          발급 수량 수정
        </h2>
        <p>
          <strong>현재 발급 {{ target.issued_count }}건.</strong>
          발급 시작 후 증량은 가능하지만, 감소할 때는 이미 발급·예약된 수량
          미만으로 낮출 수 없습니다. 개인 한도 축소는 기존 쿠폰을 회수하지 않고
          신규 발급에만 적용됩니다.
        </p>
        <label
          >총 발급 수량<input
            type="number"
            min="1"
            step="1"
            [(ngModel)]="quantityTotal"
        /></label>
        <label
          >회원별 누적 수량<input
            type="number"
            min="1"
            step="1"
            [(ngModel)]="quantityPerUser"
        /></label>
        @if (quantityError()) {
          <p class="errors" role="alert">{{ quantityError() }}</p>
        }
        <div class="wizard-actions">
          <coupon-button variant="secondary" (click)="cancelQuantity()"
            >취소</coupon-button
          ><coupon-button [disabled]="saving()" (click)="saveQuantity()"
            >수량 저장</coupon-button
          >
        </div>
      </section>
    }

    @if (publishTarget(); as target) {
      <section
        class="action-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="publish-title"
      >
        <h2 #publishHeading id="publish-title" tabindex="-1">
          캠페인 게시 검토
        </h2>
        <p class="danger-note">
          <strong>{{ target.name }}</strong
          >을 게시하면 즉시 또는 예약 시각에 대상 스냅샷·발급 작업을 단 한 번
          등록합니다. 최초 발급 후 혜택·조건·기간 스냅샷은 소급 변경할 수
          없습니다.
        </p>
        <label
          >확인하려면 <strong>캠페인 게시</strong>를 입력하세요<input
            [(ngModel)]="publishConfirmation"
            autocomplete="off"
        /></label>
        <label
          >현재 비밀번호<input
            type="password"
            [(ngModel)]="publishReauthentication"
            autocomplete="current-password"
        /></label>
        @if (publishError()) {
          <p class="errors" role="alert">{{ publishError() }}</p>
        }
        <div class="wizard-actions">
          <coupon-button variant="secondary" (click)="cancelPublish()"
            >취소</coupon-button
          ><coupon-button
            [disabled]="
              publishConfirmation !== '캠페인 게시' ||
              !publishReauthentication ||
              saving()
            "
            (click)="confirmPublish()"
            >재인증하고 게시</coupon-button
          >
        </div>
      </section>
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .wizard,
    .campaign-list {
      display: grid;
      gap: 1rem;
    }
    .wizard-head,
    .campaign-title,
    .wizard-actions,
    .campaign-actions {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.75rem;
    }
    .eyebrow {
      margin: 0;
      color: var(--coupon-color-primary);
      font-weight: 900;
    }
    h2 {
      margin: 0.25rem 0;
    }
    .plain-button {
      min-height: 44px;
      border: 0;
      background: transparent;
      color: var(--coupon-color-primary);
      font-weight: 800;
    }
    .wizard-steps {
      display: grid;
      grid-template-columns: repeat(7, minmax(5rem, 1fr));
      gap: 0.35rem;
      margin: 0;
      padding: 0 0 0.5rem;
      overflow-x: auto;
      list-style: none;
    }
    .wizard-steps li {
      display: grid;
      justify-items: center;
      gap: 0.25rem;
      color: var(--coupon-color-text-muted);
      font-size: 0.78rem;
    }
    .wizard-steps span {
      display: grid;
      place-items: center;
      width: 2rem;
      height: 2rem;
      border: 1px solid currentColor;
      border-radius: 50%;
    }
    .wizard-steps .current {
      color: var(--coupon-color-primary);
      font-weight: 900;
    }
    fieldset {
      display: grid;
      gap: 1rem;
      margin: 0;
      padding: 0;
      border: 0;
    }
    legend {
      margin-bottom: 1rem;
      font-size: 1.25rem;
      font-weight: 900;
    }
    label {
      display: grid;
      gap: 0.35rem;
      font-weight: 800;
    }
    input,
    select,
    textarea {
      width: 100%;
      min-height: 44px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .check {
      grid-template-columns: 24px 1fr;
      align-items: center;
      min-height: 44px;
    }
    .check input {
      width: 22px;
      height: 22px;
    }
    .two-cols,
    .review-grid,
    .campaign-facts {
      display: grid;
      gap: 0.75rem;
    }
    .note,
    time,
    .campaign-title p {
      color: var(--coupon-color-text-muted);
    }
    .errors,
    .danger-note {
      padding: 0.75rem;
      border-left: 4px solid var(--coupon-color-danger);
      background: var(--coupon-color-surface-muted);
      color: var(--coupon-color-danger);
    }
    .immutable {
      padding: 1rem;
      border: 1px solid var(--coupon-color-warning);
      border-radius: var(--coupon-radius-sm);
    }
    .review-grid > div {
      display: grid;
      gap: 0.25rem;
      padding: 0.8rem;
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-surface-muted);
    }
    .review-grid strong {
      font-size: 1.15rem;
    }
    .campaign-list > article,
    .action-panel {
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-md);
      background: var(--coupon-color-surface);
    }
    .campaign-title {
      align-items: flex-start;
    }
    .campaign-facts {
      margin: 1rem 0;
    }
    .campaign-facts div,
    .progress-section > div {
      padding: 0.55rem;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dt,
    .progress-section span {
      color: var(--coupon-color-text-muted);
    }
    dd {
      margin: 0.2rem 0 0;
    }
    .progress-section {
      margin: 1rem 0;
      padding: 0.7rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
    }
    .progress-section > div {
      display: flex;
      justify-content: space-between;
    }
    .progress {
      height: 0.75rem;
      overflow: hidden;
      border: 0 !important;
      border-radius: 2rem;
      background: var(--coupon-color-surface-muted);
    }
    .progress span {
      display: block;
      height: 100%;
      background: var(--coupon-color-primary);
    }
    .campaign-actions {
      justify-content: flex-end;
      flex-wrap: wrap;
    }
    .action-panel {
      position: fixed;
      inset: 50% auto auto 50%;
      z-index: 30;
      display: grid;
      gap: 0.85rem;
      width: min(calc(100% - 2rem), 38rem);
      max-height: 90dvh;
      overflow: auto;
      transform: translate(-50%, -50%);
      box-shadow: 0 0 0 100vmax #0008;
    }
    @media (min-width: 768px) {
      .two-cols,
      .review-grid,
      .campaign-facts {
        grid-template-columns: repeat(2, 1fr);
      }
      .campaign-list {
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }
    }
  `,
})
export class CampaignProgressComponent implements OnInit {
  readonly wizardHeading = viewChild<ElementRef<HTMLElement>>("wizardHeading");
  readonly validationError =
    viewChild<ElementRef<HTMLElement>>("validationError");
  readonly actionHeading = viewChild<ElementRef<HTMLElement>>("actionHeading");
  readonly quantityHeading =
    viewChild<ElementRef<HTMLElement>>("quantityHeading");
  readonly publishHeading =
    viewChild<ElementRef<HTMLElement>>("publishHeading");
  private readonly api = inject(CampaignsApi);
  private readonly auth = inject(AuthSessionService);
  private readonly destroyRef = inject(DestroyRef);
  private inFlight = false;
  private returnFocus: HTMLElement | null = null;
  readonly steps = CAMPAIGN_WIZARD_STEPS;
  readonly immutableFields = IMMUTABLE_AFTER_ISSUANCE;
  readonly items = signal<OwnerCampaignDto[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly version = signal<number | null>(null);
  readonly updatedAt = signal<string | null>(null);
  readonly mode = signal<"list" | "wizard">("list");
  readonly step = signal<CampaignWizardStep>("benefit");
  readonly validationErrors = signal<string[]>([]);
  readonly saving = signal(false);
  readonly actionTarget = signal<OwnerCampaignDto | null>(null);
  readonly action = signal<CampaignAction | null>(null);
  readonly actionError = signal<string | null>(null);
  readonly quantityTarget = signal<OwnerCampaignDto | null>(null);
  readonly quantityError = signal<string | null>(null);
  readonly publishTarget = signal<OwnerCampaignDto | null>(null);
  readonly publishError = signal<string | null>(null);
  confirmation = "";
  actionReason = "";
  quantityTotal = 1;
  quantityPerUser = 1;
  publishConfirmation = "";
  publishReauthentication = "";
  draft: CampaignDraft = createCampaignDraft();

  ngOnInit(): void {
    visibilityAwarePoll(5_000)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.load());
  }

  load(): void {
    if (this.inFlight) return;
    this.inFlight = true;
    this.api
      .list(this.version() ?? undefined, this.updatedAt() ?? undefined)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (response) => {
          this.items.set(response.items);
          this.version.set(response.version);
          this.updatedAt.set(response.updated_at);
          this.loading.set(false);
          this.error.set(null);
          this.inFlight = false;
        },
        error: () => {
          this.error.set("서버 연결을 확인해 주세요.");
          this.loading.set(false);
          this.inFlight = false;
        },
      });
  }

  startWizard(): void {
    this.captureFocus();
    this.draft = createCampaignDraft();
    this.step.set("benefit");
    this.validationErrors.set([]);
    this.mode.set("wizard");
    this.focusAfterRender(this.wizardHeading);
  }

  closeWizard(): void {
    this.mode.set("list");
    this.restoreFocus();
  }

  nextStep(): void {
    const errors = validateCampaignStep(this.draft, this.step());
    this.validationErrors.set(errors);
    if (errors.length) {
      this.focusAfterRender(this.validationError);
      return;
    }
    this.step.set(this.steps[this.stepIndex() + 1]);
    this.focusAfterRender(this.wizardHeading);
  }

  previousStep(): void {
    if (this.stepIndex() === 0) return;
    this.validationErrors.set([]);
    this.step.set(this.steps[this.stepIndex() - 1]);
    this.focusAfterRender(this.wizardHeading);
  }

  saveCampaign(): void {
    const errors = validateCampaignStep(this.draft, "review");
    this.validationErrors.set(errors);
    if (errors.length || this.saving()) return;
    this.saving.set(true);
    this.api
      .create(toSaveCampaignRequest(this.draft), createUuid())
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (campaign) => {
          this.items.update((items) => [campaign, ...items]);
          this.saving.set(false);
          this.mode.set("list");
          this.restoreFocus();
        },
        error: () => {
          this.validationErrors.set([
            "초안을 저장하지 못했습니다. 입력값과 연결을 확인해 주세요.",
          ]);
          this.saving.set(false);
          this.focusAfterRender(this.validationError);
        },
      });
  }

  beginAction(campaign: OwnerCampaignDto, action: CampaignAction): void {
    this.captureFocus();
    this.actionTarget.set(campaign);
    this.action.set(action);
    this.confirmation = "";
    this.actionReason = "";
    this.actionError.set(null);
    this.focusAfterRender(this.actionHeading);
  }

  cancelAction(): void {
    this.actionTarget.set(null);
    this.action.set(null);
    this.restoreFocus();
  }

  resume(campaign: OwnerCampaignDto): void {
    if (this.saving()) return;
    this.saving.set(true);
    this.api
      .action(
        campaign.id,
        "resume",
        {
          confirmation_phrase: "안전 재개",
          reason: "일시 중지 원인 해소 후 체크포인트 재개",
          revoke_issued_coupons: false,
        },
        createUuid(),
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (updated) => {
          this.replaceCampaign(updated);
          this.saving.set(false);
        },
        error: () => {
          this.error.set(
            "일시 중지 원인과 작업 체크포인트를 확인한 뒤 다시 재개하세요.",
          );
          this.saving.set(false);
        },
      });
  }

  beginQuantityEdit(campaign: OwnerCampaignDto): void {
    this.captureFocus();
    this.quantityTarget.set(campaign);
    this.quantityTotal =
      campaign.total_quantity ?? Math.max(campaign.issued_count, 1);
    this.quantityPerUser = campaign.per_user_quantity;
    this.quantityError.set(null);
    this.focusAfterRender(this.quantityHeading);
  }

  cancelQuantity(): void {
    this.quantityTarget.set(null);
    this.restoreFocus();
  }

  beginPublish(campaign: OwnerCampaignDto): void {
    this.captureFocus();
    this.publishTarget.set(campaign);
    this.publishConfirmation = "";
    this.publishReauthentication = "";
    this.publishError.set(null);
    this.focusAfterRender(this.publishHeading);
  }

  cancelPublish(): void {
    this.publishTarget.set(null);
    this.publishReauthentication = "";
    this.restoreFocus();
  }

  async confirmPublish(): Promise<void> {
    const target = this.publishTarget();
    if (
      !target ||
      this.publishConfirmation !== "캠페인 게시" ||
      !this.publishReauthentication ||
      this.saving()
    )
      return;
    this.saving.set(true);
    try {
      await this.auth.reauthenticateWithPassword(this.publishReauthentication);
    } catch {
      this.publishReauthentication = "";
      this.publishError.set("현재 비밀번호로 재인증하지 못했습니다.");
      this.saving.set(false);
      return;
    }
    this.api
      .publish(target.id, createUuid())
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (published) => {
          this.replaceCampaign(published);
          this.saving.set(false);
          this.cancelPublish();
        },
        error: () => {
          this.publishReauthentication = "";
          this.publishError.set(
            "재인증이 만료되었거나 게시 전 검증을 통과하지 못했습니다.",
          );
          this.saving.set(false);
        },
      });
  }

  saveQuantity(): void {
    const target = this.quantityTarget();
    if (!target || this.saving()) return;
    if (
      !Number.isSafeInteger(this.quantityTotal) ||
      this.quantityTotal < Math.max(1, target.issued_count)
    ) {
      this.quantityError.set(
        `총 수량은 이미 발급된 ${target.issued_count}건 이상이어야 합니다.`,
      );
      return;
    }
    if (
      !Number.isSafeInteger(this.quantityPerUser) ||
      this.quantityPerUser < 1
    ) {
      this.quantityError.set("회원별 수량은 1 이상이어야 합니다.");
      return;
    }
    this.saving.set(true);
    this.api
      .updateQuantity(
        target,
        this.quantityTotal,
        this.quantityPerUser,
        createUuid(),
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (updated) => {
          this.replaceCampaign(updated);
          this.quantityTarget.set(null);
          this.saving.set(false);
          this.restoreFocus();
        },
        error: () => {
          this.quantityError.set(
            "캠페인 버전 또는 발급·예약 수량이 변경되었습니다. 목록을 새로고침하세요.",
          );
          this.saving.set(false);
        },
      });
  }

  confirmAction(): void {
    const target = this.actionTarget();
    const action = this.action();
    if (
      !target ||
      !action ||
      this.confirmation !== this.confirmationPhrase() ||
      !this.actionReason.trim()
    )
      return;
    this.saving.set(true);
    this.api
      .action(
        target.id,
        action === "pause" ? "pause" : "cancel",
        {
          confirmation_phrase: this.confirmation,
          reason: this.actionReason.trim(),
          revoke_issued_coupons: action === "revoke",
        },
        createUuid(),
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (campaign) => {
          this.replaceCampaign(campaign);
          this.saving.set(false);
          this.cancelAction();
        },
        error: () => {
          this.actionError.set(
            "작업을 완료하지 못했습니다. 상태를 새로고침한 뒤 다시 확인해 주세요.",
          );
          this.saving.set(false);
        },
      });
  }

  stepIndex(): number {
    return this.steps.indexOf(this.step());
  }
  stepLabel(step: CampaignWizardStep): string {
    return {
      benefit: "혜택",
      conditions: "사용 조건",
      audience: "대상",
      quantity: "수량",
      schedule: "일정",
      notification: "알림",
      review: "검토",
    }[step];
  }
  splitIds(value: string): string[] {
    return value
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
  }
  toggleUnlimited(event: Event): void {
    this.draft.total_quantity = (event.target as HTMLInputElement).checked
      ? null
      : 100;
  }
  sampleDiscount(): number {
    return this.draft.benefit_type === "FREE_ITEM"
      ? 0
      : previewCampaignDiscount(this.draft, 9_999);
  }
  exposure(): number | null {
    return maximumCampaignExposure(this.draft);
  }
  notificationCount(): number {
    return estimatedNotificationCount(this.draft);
  }
  won(value: number): string {
    return formatWon(value);
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }
  progress(campaign: OwnerCampaignDto): number {
    return campaign.snapshot_target_count
      ? Math.min(
          100,
          Math.round(
            (campaign.processed_count / campaign.snapshot_target_count) * 100,
          ),
        )
      : 0;
  }
  statusLabel(status: CampaignStatus): string {
    return {
      DRAFT: "초안",
      SCHEDULED: "예약",
      ISSUING: "발급 중",
      ACTIVE: "진행 중",
      PAUSED: "일시 중지",
      ENDED: "종료",
      CANCELLED: "취소",
    }[status];
  }
  badge(status: CampaignStatus): "success" | "warning" | "danger" | "neutral" {
    return status === "ACTIVE" || status === "ISSUING"
      ? "success"
      : status === "SCHEDULED" || status === "PAUSED"
        ? "warning"
        : status === "CANCELLED"
          ? "danger"
          : "neutral";
  }
  issuanceLabel(method: OwnerCampaignDto["issuance_method"]): string {
    return {
      FIRST_COME: "공개 선착순",
      TARGETED: "대상 일괄 지급",
      DIRECT: "특정 고객 직접 지급",
    }[method];
  }
  confirmationPhrase(): string {
    return this.action() === "pause"
      ? "일시 중지"
      : this.action() === "revoke"
        ? "발급분 회수"
        : "캠페인 취소";
  }
  actionTitle(): string {
    return `${this.confirmationPhrase()} 영향 확인`;
  }

  @HostListener("document:keydown.escape")
  closeActiveLayer(): void {
    if (this.actionTarget()) this.cancelAction();
    else if (this.quantityTarget()) this.cancelQuantity();
    else if (this.publishTarget()) this.cancelPublish();
    else if (this.mode() === "wizard") this.closeWizard();
  }

  private captureFocus(): void {
    this.returnFocus =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
  }

  private restoreFocus(): void {
    const target = this.returnFocus;
    this.returnFocus = null;
    setTimeout(() => target?.focus());
  }

  private focusAfterRender(
    ref: () => ElementRef<HTMLElement> | undefined,
  ): void {
    setTimeout(() => ref()?.nativeElement.focus());
  }

  private replaceCampaign(campaign: OwnerCampaignDto): void {
    this.items.update((items) =>
      items.map((item) => (item.id === campaign.id ? campaign : item)),
    );
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
