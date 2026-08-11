import { HttpErrorResponse } from "@angular/common/http";
import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { takeUntilDestroyed } from "@angular/core/rxjs-interop";
import { FormBuilder, ReactiveFormsModule, Validators } from "@angular/forms";
import type {
  CatalogItemDto,
  CreateStampTransactionRequestDto,
  ResolvedCustomerDto,
  StampPreviewResponseDto,
  StampTransactionResponseDto,
  ConfirmRedemptionRequestDto,
  RedemptionPreviewResponseDto,
  RedemptionResponseDto,
} from "@coupon/contracts";
import { CouponClientError } from "@coupon/client-core";
import { formatKoreaDateTime, formatWon } from "@coupon/domain";
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from "@coupon/ui";
import {
  ScanStateMachine,
  type CameraState,
  type ScanState,
} from "./scan-state-machine";
import { StoreOperationsApi } from "./store-operations.api";
import {
  redemptionConditionMessage,
  redemptionReservationView,
} from "./redemption-state";

interface BarcodeDetectorLike {
  detect(source: CanvasImageSource): Promise<Array<{ rawValue: string }>>;
}
interface BarcodeDetectorConstructor {
  new (options: { formats: string[] }): BarcodeDetectorLike;
}

@Component({
  selector: "coupon-store-scan",
  imports: [
    ReactiveFormsModule,
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header
      title="고객 QR 스캔"
      description="QR 확인부터 최종 승인까지 한 단계씩 안전하게 처리합니다."
      eyebrow="현장 거래"
    >
      <coupon-badge
        [status]="
          state() === 'SUCCESS'
            ? 'success'
            : state() === 'FAILURE'
              ? 'danger'
              : 'neutral'
        "
        [label]="'현재 단계 ' + state()"
        >{{ stepNumber() }}/8 · {{ state() }}</coupon-badge
      >
    </coupon-page-header>
    <ol class="steps" aria-label="스캔 처리 단계">
      @for (step of steps; track step; let index = $index) {
        <li
          [class.current]="state() === step"
          [class.done]="stepIndex() > index"
        >
          <span>{{ index + 1 }}</span
          ><small>{{ stepLabel(step) }}</small>
        </li>
      }
    </ol>

    @switch (state()) {
      @case ("READY") {
        <section class="scan-layout">
          <div class="camera-card">
            <video
              #video
              playsinline
              muted
              aria-label="고객 QR을 비추는 후면 카메라 미리보기"
            ></video>
            <div class="camera-placeholder">
              <span aria-hidden="true">▦</span
              ><strong>{{ cameraTitle() }}</strong>
              <p>{{ cameraDescription() }}</p>
            </div>
          </div>
          <div class="side-card">
            @if (camera() === "checking") {
              <coupon-skeleton
                [lines]="4"
                label="카메라 권한을 확인하는 중입니다."
              />
            } @else {
              <h2>카메라를 사용할 수 없나요?</h2>
              <p>
                고객 화면의 8자리 일회용 보조 코드를 입력할 수 있습니다.
                이메일·전화번호는 받지 않습니다.
              </p>
              <form [formGroup]="manualForm" (ngSubmit)="resolveManual()">
                <label
                  >8자리 보조 코드<input
                    formControlName="code"
                    inputmode="numeric"
                    maxlength="8"
                    autocomplete="one-time-code"
                    placeholder="12345678" /></label
                ><coupon-button
                  type="submit"
                  [fullWidth]="true"
                  [disabled]="manualForm.invalid || resolving()"
                  >{{
                    resolving() ? "확인 중…" : "보조 코드 확인"
                  }}</coupon-button
                >
              </form>
              @if (camera() !== "unchecked" && camera() !== "checking") {
                <coupon-button
                  variant="secondary"
                  [fullWidth]="true"
                  (click)="startCamera()"
                  >카메라 권한 다시 확인</coupon-button
                >
              }
              @if (resolveError()) {
                <p class="inline-error" role="alert">
                  <span aria-hidden="true">!</span>{{ resolveError() }}
                </p>
              }
            }
          </div>
        </section>
      }
      @case ("SCANNING") {
        <section class="scan-layout">
          <div class="camera-card active">
            <video
              #video
              playsinline
              muted
              aria-label="고객 QR을 비추는 후면 카메라 미리보기"
            ></video>
            <div class="finder" aria-hidden="true"></div>
            <p>
              {{
                resolving()
                  ? "QR을 확인하고 있습니다. 중복 프레임은 처리하지 않습니다."
                  : detectorAvailable()
                    ? "QR을 네모 안에 맞춰 주세요."
                    : "이 브라우저는 자동 인식을 지원하지 않습니다. 보조 코드를 입력해 주세요."
              }}
            </p>
          </div>
          <div class="side-card">
            <h2>8자리 보조 코드</h2>
            <p>QR과 같은 60초 nonce를 사용합니다.</p>
            <form [formGroup]="manualForm" (ngSubmit)="resolveManual()">
              <label
                >보조 코드<input
                  formControlName="code"
                  inputmode="numeric"
                  maxlength="8"
                  autocomplete="one-time-code" /></label
              ><coupon-button
                type="submit"
                [fullWidth]="true"
                [disabled]="manualForm.invalid || resolving()"
                >코드 확인</coupon-button
              >
            </form>
            @if (resolveError()) {
              <p class="inline-error" role="alert">{{ resolveError() }}</p>
            }
          </div>
        </section>
      }
      @case ("CUSTOMER_RESOLVED") {
        <coupon-card
          ><div class="resolved">
            <span aria-hidden="true">✓</span>
            <div>
              <h2>고객을 안전하게 확인했습니다</h2>
              <p>
                {{ customer()?.display_name_masked }} ·
                {{ customer()?.customer_reference_masked }}
              </p>
              <p>
                현재 가용 도장 {{ customer()?.available_stamp_count }}개 · QR
                만료 {{ date(customer()!.qr_expires_at) }}
              </p>
            </div>
          </div>
          <coupon-button [fullWidth]="true" (click)="beginInput()"
            >거래 입력 계속</coupon-button
          ></coupon-card
        >
      }
      @case ("INPUT") {
        <form
          class="transaction-form"
          [formGroup]="orderForm"
          (ngSubmit)="preview()"
        >
          <coupon-card
            ><fieldset>
              <legend>거래 종류</legend>
              <div class="transaction-types">
                <label
                  ><input
                    type="radio"
                    formControlName="type"
                    value="stamp"
                  /><span
                    ><strong>도장 적립</strong
                    ><small>Phase 2에서 사용 가능</small></span
                  ></label
                ><label
                  ><input
                    type="radio"
                    formControlName="type"
                    value="redeem"
                  /><span
                    ><strong>쿠폰 사용</strong
                    ><small>2분 예약 후 최종 승인</small></span
                  ></label
                >
              </div>
            </fieldset></coupon-card
          >
          <coupon-card
            ><fieldset>
              <legend>주문 정보</legend>
              <label
                >주문 금액 <span aria-hidden="true">*</span>
                <div class="won-input">
                  <input
                    type="number"
                    formControlName="gross_amount"
                    min="0"
                    max="100000000"
                    step="1"
                    inputmode="numeric"
                  /><span>원</span>
                </div></label
              ><label
                >외부 주문번호 <small>(선택)</small
                ><input
                  formControlName="external_order_ref"
                  maxlength="80"
                  autocomplete="off"
              /></label>
              <div class="item-row">
                <label
                  >품목<select formControlName="catalog_item_id">
                    <option value="">직접 입력</option>
                    @for (item of activeCatalog(); track item.id) {
                      <option [value]="item.id">
                        {{ item.name }}{{ item.sku ? " · " + item.sku : "" }}
                      </option>
                    }
                  </select></label
                ><label
                  >품목명<input
                    formControlName="item_name"
                    maxlength="80" /></label
                ><label
                  >수량<input
                    type="number"
                    formControlName="quantity"
                    min="1"
                    max="100" /></label
                ><label
                  >실제 단가<input
                    type="number"
                    formControlName="unit_price"
                    min="0"
                    max="100000000"
                /></label>
              </div>
              <p class="hint">
                상품 마스터 가격은 참고값입니다. 정책 계산에는 이 거래에서
                입력한 실제 금액을 사용합니다.
              </p>
            </fieldset></coupon-card
          >
          @if (previewError()) {
            <p class="inline-error" role="alert">
              <span aria-hidden="true">!</span>{{ previewError() }}
            </p>
          }
          <coupon-button
            type="submit"
            [fullWidth]="true"
            [disabled]="orderForm.invalid || previewing()"
            >{{
              previewing() ? "조건 확인 중…" : "승인 전 검토"
            }}</coupon-button
          >
        </form>
      }
      @case ("REVIEW") {
        @if (transactionType() === "stamp" && previewResult(); as review) {
          <section class="review-grid">
            <coupon-card
              ><h2>고객·예상 결과</h2>
              <dl>
                <div>
                  <dt>고객</dt>
                  <dd>
                    {{ review.display_name_masked }} ·
                    {{ review.customer_reference_masked }}
                  </dd>
                </div>
                <div>
                  <dt>주문 금액</dt>
                  <dd>{{ won(orderForm.controls.gross_amount.value) }}</dd>
                </div>
                <div>
                  <dt>예상 적립</dt>
                  <dd>
                    <strong>{{ review.expected_stamp_count }}개</strong> · 적립
                    후 {{ review.balance_after }}개
                  </dd>
                </div>
                <div>
                  <dt>도장 만료</dt>
                  <dd>{{ date(review.stamp_expires_at) }}</dd>
                </div>
                <div>
                  <dt>리워드</dt>
                  <dd>{{ review.reward_description }}</dd>
                </div>
              </dl></coupon-card
            ><coupon-card
              ><h2>만료와 제한 재확인</h2>
              <ul>
                @for (limit of review.limits; track limit) {
                  <li>{{ limit }}</li>
                }
              </ul>
              @if (review.duplicate_warning) {
                <p class="warning" role="alert">
                  <span aria-hidden="true">⚠</span
                  >{{ review.duplicate_warning }}
                </p>
              }
            </coupon-card>
          </section>
          <div class="review-actions">
            <coupon-button variant="secondary" (click)="editInput()"
              >입력 수정</coupon-button
            ><coupon-button (click)="submit()">도장 적립 승인</coupon-button>
          </div>
        } @else if (redemptionPreview(); as review) {
          <section class="review-grid">
            <coupon-card>
              <div
                class="reservation-clock"
                [class.expired]="reservationView().expired"
                role="timer"
                aria-live="polite"
              >
                <span aria-hidden="true">{{
                  reservationView().expired ? "!" : "◷"
                }}</span>
                <strong>{{ reservationView().message }}</strong>
              </div>
              <h2>예상 할인·결제 금액</h2>
              <dl>
                <div>
                  <dt>고객</dt>
                  <dd>
                    {{ review.display_name_masked }} ·
                    {{ review.customer_reference_masked }}
                  </dd>
                </div>
                <div>
                  <dt>혜택</dt>
                  <dd>{{ review.benefit_label }}</dd>
                </div>
                <div>
                  <dt>주문 금액</dt>
                  <dd>{{ won(orderForm.controls.gross_amount.value) }}</dd>
                </div>
                <div>
                  <dt>예상 할인</dt>
                  <dd>
                    <strong>{{ won(review.expected_discount_amount) }}</strong>
                  </dd>
                </div>
                <div>
                  <dt>예상 결제</dt>
                  <dd>
                    <strong>{{ won(review.payable_amount) }}</strong>
                  </dd>
                </div>
                <div>
                  <dt>문의 식별</dt>
                  <dd>
                    <code>{{ review.coupon_inquiry_reference }}</code>
                  </dd>
                </div>
              </dl>
            </coupon-card>
            <coupon-card>
              <h2>사용 조건 재확인</h2>
              <ul>
                @for (condition of review.conditions; track condition) {
                  <li>{{ condition }}</li>
                }
              </ul>
              <p class="note">
                승인 시점에 금액·품목·[시작, 종료) 기간을 서버가 다시
                확인합니다.
              </p>
            </coupon-card>
          </section>
          <div class="review-actions">
            <coupon-button variant="secondary" (click)="editInput()"
              >입력 수정</coupon-button
            >
            @if (reservationView().expired) {
              <coupon-button [disabled]="previewing()" (click)="reReserve()">{{
                previewing() ? "재예약 중…" : "다시 예약"
              }}</coupon-button>
            } @else {
              <coupon-button (click)="submit()">쿠폰 사용 승인</coupon-button>
            }
          </div>
        }
      }
      @case ("SUBMITTING") {
        <section class="processing" aria-live="polite">
          <span class="spinner" aria-hidden="true"></span>
          <h2>
            {{
              transactionType() === "redeem"
                ? "쿠폰 사용을 반영하고 있습니다"
                : "도장을 반영하고 있습니다"
            }}
          </h2>
          <p>창을 닫거나 승인 버튼을 다시 누르지 마세요.</p>
        </section>
      }
      @case ("SUCCESS") {
        @if (transactionType() === "stamp" && result(); as success) {
          <section class="result success" role="status">
            <span class="result-icon" aria-hidden="true">✓</span>
            <h2>도장 적립 완료</h2>
            <p>
              {{ success.stamp_count }}개가 반영되어 현재
              {{ success.balance_after }}개입니다.
              @if (success.reward_issued) {
                리워드도 함께 발급되었습니다.
              }
            </p>
            <dl>
              <div>
                <dt>거래 ID</dt>
                <dd>
                  <code>{{ success.transaction_id }}</code>
                </dd>
              </div>
              <div>
                <dt>처리 시각</dt>
                <dd>{{ date(success.processed_at) }}</dd>
              </div>
            </dl>
            <p>이 화면은 자동으로 초기화되지 않습니다.</p>
            <coupon-button [fullWidth]="true" (click)="nextCustomer()"
              >다음 고객</coupon-button
            >
          </section>
        } @else if (redemptionResult(); as success) {
          <section class="result success" role="status">
            <span class="result-icon" aria-hidden="true">✓</span>
            <h2>
              {{
                success.status === "CANCELLED"
                  ? "쿠폰 사용 취소 완료"
                  : "쿠폰 사용 완료"
              }}
            </h2>
            <p>
              {{
                success.status === "CANCELLED"
                  ? "쿠폰 사용을 취소했습니다. 쿠폰 상태는 서버 결과를 따릅니다."
                  : won(success.discount_amount) + " 할인을 반영했습니다."
              }}
            </p>
            <dl>
              <div>
                <dt>최종 결제</dt>
                <dd>{{ won(success.payable_amount) }}</dd>
              </div>
              <div>
                <dt>거래 ID</dt>
                <dd>
                  <code>{{ success.transaction_id }}</code>
                </dd>
              </div>
              <div>
                <dt>처리 시각</dt>
                <dd>{{ date(success.processed_at) }}</dd>
              </div>
              @if (success.cancellable_until) {
                <div>
                  <dt>점주 취소 한도</dt>
                  <dd>{{ date(success.cancellable_until) }} (최대 10분)</dd>
                </div>
              }
            </dl>
            @if (
              success.status === "USED" &&
              canCancelRedemption() &&
              !showCancelForm()
            ) {
              <coupon-button
                variant="secondary"
                [fullWidth]="true"
                (click)="showCancelForm.set(true)"
                >10분 이내 사용 취소</coupon-button
              >
            }
            @if (success.status === "USED" && showCancelForm()) {
              <form
                class="cancel-form"
                [formGroup]="cancelForm"
                (ngSubmit)="cancelRedemption()"
              >
                <h3>사용 취소 사유</h3>
                <label
                  >외부 주문 취소 사유<textarea
                    formControlName="reason"
                    rows="3"
                    maxlength="200"
                  ></textarea>
                </label>
                <label class="restore-check"
                  ><input
                    type="checkbox"
                    formControlName="restore_if_eligible"
                  />원 캠페인과 만료 조건이 유효하면 쿠폰을 사용 가능으로
                  복원</label
                >
                <div class="review-actions">
                  <coupon-button
                    variant="secondary"
                    (click)="showCancelForm.set(false)"
                    >돌아가기</coupon-button
                  ><coupon-button
                    type="submit"
                    [disabled]="cancelForm.invalid || cancelling()"
                    >{{
                      cancelling() ? "취소 중…" : "사용 취소 확정"
                    }}</coupon-button
                  >
                </div>
              </form>
            }
            <p>이 화면은 자동으로 초기화되지 않습니다.</p>
            <coupon-button [fullWidth]="true" (click)="nextCustomer()"
              >다음 고객</coupon-button
            >
          </section>
        }
      }
      @case ("FAILURE") {
        <section class="result failure" role="alert">
          <span class="result-icon" aria-hidden="true">×</span>
          <h2>
            {{
              uncertain()
                ? "처리 결과를 확인해야 합니다"
                : transactionType() === "redeem"
                  ? "쿠폰 사용 실패"
                  : "도장 적립 실패"
            }}
          </h2>
          <p>{{ failureMessage() }}</p>
          <dl>
            <div>
              <dt>거래 ID</dt>
              <dd>
                {{
                  result()?.transaction_id ??
                    redemptionResult()?.transaction_id ??
                    "아직 확인되지 않음"
                }}
              </dd>
            </div>
            <div>
              <dt>요청 식별</dt>
              <dd>
                <code>{{ idempotencyKey() }}</code>
              </dd>
            </div>
          </dl>
          @if (uncertain()) {
            <p class="warning">
              <strong>새 요청을 만들지 않습니다.</strong> 아래 버튼은 같은
              멱등키로 원래 요청의 결과를 확인합니다.
            </p>
            <coupon-button [fullWidth]="true" (click)="checkResult()"
              >처리 결과 확인</coupon-button
            >
          } @else {
            <coupon-button [fullWidth]="true" (click)="nextCustomer()"
              >다음 고객</coupon-button
            >
          }
        </section>
      }
    }
  `,
  styles: `
    :host {
      display: block;
    }
    .steps {
      display: grid;
      grid-template-columns: repeat(8, minmax(3.2rem, 1fr));
      gap: 0.25rem;
      margin: 0 0 1rem;
      padding: 0;
      overflow-x: auto;
      list-style: none;
    }
    .steps li {
      display: grid;
      justify-items: center;
      gap: 0.2rem;
      min-width: 3.2rem;
      color: var(--coupon-color-text-muted);
    }
    .steps span {
      display: grid;
      place-items: center;
      width: 1.8rem;
      height: 1.8rem;
      border: 1px solid currentColor;
      border-radius: 50%;
      font-weight: 800;
    }
    .steps small {
      white-space: nowrap;
    }
    .steps .current {
      color: var(--coupon-color-primary);
      font-weight: 900;
    }
    .steps .done {
      color: var(--coupon-color-success);
    }
    .scan-layout {
      display: grid;
      gap: 1rem;
    }
    .camera-card,
    .side-card,
    .processing,
    .result {
      padding: 1rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-lg);
      background: var(--coupon-color-surface);
    }
    .camera-card {
      position: relative;
      display: grid;
      place-items: center;
      min-height: 22rem;
      overflow: hidden;
      background: #07130f;
      color: #fff;
    }
    .camera-card video {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
      object-fit: cover;
    }
    .camera-placeholder {
      position: relative;
      z-index: 1;
      display: grid;
      justify-items: center;
      max-width: 24rem;
      padding: 1rem;
      text-align: center;
    }
    .camera-placeholder > span {
      font-size: 4rem;
    }
    .camera-card.active p {
      position: absolute;
      inset: auto 0.75rem 0.75rem;
      z-index: 2;
      margin: 0;
      padding: 0.5rem;
      border-radius: 0.4rem;
      background: #07130fcc;
      text-align: center;
    }
    .finder {
      position: relative;
      z-index: 2;
      width: min(60vw, 16rem);
      aspect-ratio: 1;
      border: 4px solid #fff;
      border-radius: 1rem;
      box-shadow: 0 0 0 999px #0005;
    }
    .side-card h2 {
      margin-top: 0;
    }
    .side-card form {
      display: grid;
      gap: 0.75rem;
    }
    .side-card > coupon-button {
      display: block;
      margin-top: 0.75rem;
    }
    label {
      display: grid;
      gap: 0.35rem;
      font-weight: 800;
    }
    input,
    select {
      width: 100%;
      min-height: 44px;
      padding: 0.6rem 0.7rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .inline-error {
      display: flex;
      gap: 0.4rem;
      padding: 0.7rem;
      border-left: 4px solid var(--coupon-color-danger);
      background: var(--coupon-color-surface-muted);
      color: var(--coupon-color-danger);
      font-weight: 700;
    }
    .resolved {
      display: grid;
      grid-template-columns: 3rem 1fr;
      gap: 0.75rem;
      margin-bottom: 1rem;
    }
    .resolved > span {
      display: grid;
      place-items: center;
      width: 3rem;
      height: 3rem;
      border-radius: 50%;
      background: var(--coupon-color-success);
      color: var(--coupon-color-bg);
      font-size: 1.5rem;
    }
    .resolved h2 {
      margin: 0;
    }
    .resolved p {
      margin: 0.25rem 0;
      color: var(--coupon-color-text-muted);
    }
    .transaction-form {
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
      margin-bottom: 0.75rem;
      font-size: 1.2rem;
      font-weight: 900;
    }
    .transaction-types {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0.6rem;
    }
    .transaction-types label {
      grid-template-columns: 24px 1fr;
      align-items: center;
      min-height: 64px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
    }
    .transaction-types input {
      width: 20px;
      height: 20px;
    }
    .transaction-types span {
      display: grid;
    }
    .transaction-types small,
    .hint {
      color: var(--coupon-color-text-muted);
    }
    .transaction-types .disabled {
      opacity: 0.65;
    }
    .won-input {
      display: grid;
      grid-template-columns: 1fr 3rem;
      align-items: center;
    }
    .won-input input {
      border-radius: 0.5rem 0 0 0.5rem;
    }
    .won-input span {
      display: grid;
      place-items: center;
      height: 44px;
      border: 1px solid var(--coupon-color-border);
      border-left: 0;
      border-radius: 0 0.5rem 0.5rem 0;
    }
    .item-row {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0.75rem;
    }
    .review-grid {
      display: grid;
      gap: 1rem;
    }
    .review-grid h2 {
      margin-top: 0;
    }
    .review-grid dl,
    .result dl {
      display: grid;
      margin: 0;
    }
    .review-grid dl div,
    .result dl div {
      display: grid;
      grid-template-columns: 7rem 1fr;
      gap: 0.5rem;
      padding: 0.6rem 0;
      border-bottom: 1px solid var(--coupon-color-border);
    }
    dt {
      color: var(--coupon-color-text-muted);
    }
    dd {
      margin: 0;
    }
    .warning {
      padding: 0.7rem;
      border: 1px solid var(--coupon-color-warning);
      border-radius: var(--coupon-radius-sm);
      color: var(--coupon-color-warning);
    }
    .reservation-clock {
      display: flex;
      align-items: center;
      gap: 0.5rem;
      margin-bottom: 1rem;
      padding: 0.75rem;
      border: 1px solid var(--coupon-color-primary);
      border-radius: var(--coupon-radius-sm);
      color: var(--coupon-color-primary);
    }
    .reservation-clock.expired {
      border-color: var(--coupon-color-danger);
      color: var(--coupon-color-danger);
    }
    .note {
      color: var(--coupon-color-text-muted);
    }
    .cancel-form {
      display: grid;
      gap: 0.75rem;
      width: min(100%, 36rem);
      margin: 1rem 0;
      padding: 1rem;
      border: 1px solid var(--coupon-color-warning);
      border-radius: var(--coupon-radius-sm);
      text-align: left;
    }
    .cancel-form textarea {
      min-height: 88px;
      padding: 0.65rem;
      border: 1px solid var(--coupon-color-border);
      border-radius: var(--coupon-radius-sm);
      background: var(--coupon-color-bg);
      color: var(--coupon-color-text);
    }
    .restore-check {
      grid-template-columns: 24px 1fr;
      align-items: center;
      min-height: 44px;
    }
    .restore-check input {
      width: 22px;
      height: 22px;
    }
    .review-actions {
      display: flex;
      justify-content: space-between;
      gap: 0.75rem;
      margin-top: 1rem;
    }
    .processing,
    .result {
      display: grid;
      justify-items: center;
      text-align: center;
    }
    .spinner {
      width: 3rem;
      height: 3rem;
      border: 5px solid var(--coupon-color-border);
      border-top-color: var(--coupon-color-primary);
      border-radius: 50%;
      animation: spin 0.8s linear infinite;
    }
    .result-icon {
      display: grid;
      place-items: center;
      width: 4rem;
      height: 4rem;
      border: 3px solid currentColor;
      border-radius: 50%;
      font-size: 2rem;
      font-weight: 900;
    }
    .result.success .result-icon {
      color: var(--coupon-color-success);
    }
    .result.failure .result-icon {
      color: var(--coupon-color-danger);
    }
    .result h2 {
      margin: 0.5rem 0;
    }
    .result dl {
      width: min(100%, 36rem);
      margin: 1rem 0;
      text-align: left;
    }
    .result code {
      overflow-wrap: anywhere;
    }
    .result coupon-button {
      width: min(100%, 30rem);
    }
    @keyframes spin {
      to {
        transform: rotate(360deg);
      }
    }
    @media (min-width: 768px) {
      .scan-layout,
      .review-grid {
        grid-template-columns: minmax(0, 1.6fr) minmax(18rem, 1fr);
      }
      .camera-card {
        min-height: 34rem;
      }
      .item-row {
        grid-template-columns: 2fr 2fr 0.7fr 1fr;
      }
    }
  `,
})
export class StoreScanComponent implements OnInit, AfterViewInit, OnDestroy {
  readonly video = viewChild<ElementRef<HTMLVideoElement>>("video");
  readonly steps: ScanState[] = [
    "READY",
    "SCANNING",
    "CUSTOMER_RESOLVED",
    "INPUT",
    "REVIEW",
    "SUBMITTING",
    "SUCCESS",
    "FAILURE",
  ];
  private readonly machine = new ScanStateMachine();
  private readonly api = inject(StoreOperationsApi);
  private readonly fb = inject(FormBuilder);
  private readonly destroyRef = inject(DestroyRef);
  private stream: MediaStream | null = null;
  private detector: BarcodeDetectorLike | null = null;
  private frameId: number | null = null;
  private credential: { qr_token?: string; auxiliary_code?: string } | null =
    null;
  private pendingPayload: CreateStampTransactionRequestDto | null = null;
  private pendingRedemptionPayload: ConfirmRedemptionRequestDto | null = null;
  private reservationTimer: ReturnType<typeof setInterval> | null = null;
  readonly state = signal<ScanState>("READY");
  readonly camera = signal<CameraState>("unchecked");
  readonly detectorAvailable = signal(false);
  readonly resolving = signal(false);
  readonly previewing = signal(false);
  readonly customer = signal<ResolvedCustomerDto | null>(null);
  readonly previewResult = signal<StampPreviewResponseDto | null>(null);
  readonly result = signal<StampTransactionResponseDto | null>(null);
  readonly redemptionPreview = signal<RedemptionPreviewResponseDto | null>(
    null,
  );
  readonly redemptionResult = signal<RedemptionResponseDto | null>(null);
  readonly now = signal(Date.now());
  readonly cancelling = signal(false);
  readonly showCancelForm = signal(false);
  readonly resolveError = signal<string | null>(null);
  readonly previewError = signal<string | null>(null);
  readonly failureMessage = signal("요청을 처리하지 못했습니다.");
  readonly uncertain = signal(false);
  readonly idempotencyKey = signal("");
  readonly catalog = signal<CatalogItemDto[]>([]);
  readonly activeCatalog = computed(() =>
    this.catalog().filter((item) => item.active),
  );
  readonly reservationView = computed(() => {
    const preview = this.redemptionPreview();
    return preview
      ? redemptionReservationView(preview, new Date(this.now()))
      : {
          remaining_seconds: 0,
          expired: true,
          message: "예약 정보가 없습니다.",
        };
  });
  readonly manualForm = this.fb.nonNullable.group({
    code: ["", [Validators.required, Validators.pattern(/^\d{8}$/)]],
  });
  readonly orderForm = this.fb.nonNullable.group({
    type: ["stamp" as "stamp" | "redeem", Validators.required],
    gross_amount: [
      0,
      [
        Validators.required,
        Validators.min(0),
        Validators.max(100_000_000),
        Validators.pattern(/^\d+$/),
      ],
    ],
    external_order_ref: ["", Validators.maxLength(80)],
    catalog_item_id: [""],
    item_name: ["", [Validators.required, Validators.maxLength(80)]],
    quantity: [
      1,
      [Validators.required, Validators.min(1), Validators.max(100)],
    ],
    unit_price: [
      0,
      [Validators.required, Validators.min(0), Validators.max(100_000_000)],
    ],
  });
  readonly cancelForm = this.fb.nonNullable.group({
    reason: [
      "",
      [Validators.required, Validators.minLength(3), Validators.maxLength(200)],
    ],
    restore_if_eligible: [true],
  });

  ngOnInit(): void {
    this.api
      .catalog()
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (response) => this.catalog.set(response.items),
        error: () => this.catalog.set([]),
      });
  }
  ngAfterViewInit(): void {
    void this.startCamera();
  }
  ngOnDestroy(): void {
    this.stopCamera();
    this.stopReservationClock();
  }

  async startCamera(): Promise<void> {
    this.stopCamera();
    this.resolveError.set(null);
    this.machine.checkingCamera();
    this.sync();
    if (!window.isSecureContext) {
      this.machine.insecureContext();
      this.sync();
      return;
    }
    if (!navigator.mediaDevices?.getUserMedia) {
      this.machine.cameraUnavailable();
      this.sync();
      return;
    }
    try {
      this.stream = await navigator.mediaDevices.getUserMedia({
        video: { facingMode: { ideal: "environment" } },
        audio: false,
      });
      this.machine.cameraReady();
      if (this.machine.state === "READY") this.machine.startScanning();
      this.sync();
      setTimeout(() => {
        const video = this.video()?.nativeElement;
        if (!video || !this.stream) return;
        video.srcObject = this.stream;
        void video.play();
        this.prepareDetector();
      });
    } catch (error) {
      if (
        error instanceof DOMException &&
        (error.name === "NotAllowedError" || error.name === "SecurityError")
      )
        this.machine.cameraDenied();
      else this.machine.cameraUnavailable();
      this.sync();
    }
  }

  resolveManual(): void {
    if (this.manualForm.invalid || this.resolving()) return;
    if (this.machine.state === "READY") this.machine.startScanning();
    if (!this.machine.lockDecodedFrame()) return;
    this.sync();
    this.resolve({ auxiliary_code: this.manualForm.controls.code.value });
  }
  beginInput(): void {
    this.machine.beginInput();
    this.sync();
  }
  editInput(): void {
    this.machine.editInput();
    this.sync();
  }
  preview(): void {
    if (this.orderForm.invalid || !this.customer()) {
      this.orderForm.markAllAsTouched();
      return;
    }
    this.previewing.set(true);
    this.previewError.set(null);
    if (this.transactionType() === "redeem") {
      this.requestRedemptionPreview(false);
      return;
    }
    this.api
      .preview({
        scan_session_id: this.customer()!.scan_session_id,
        order: this.orderPayload(),
      })
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (review) => {
          this.previewResult.set(review);
          this.previewing.set(false);
          this.machine.review();
          this.sync();
        },
        error: (error: unknown) => {
          this.previewing.set(false);
          this.previewError.set(
            messageOf(error, "적립 조건을 확인하지 못했습니다."),
          );
        },
      });
  }
  submit(): void {
    if (this.transactionType() === "redeem") {
      const preview = this.redemptionPreview();
      if (!preview || this.reservationView().expired) return;
      this.idempotencyKey.set(createUuid());
      this.pendingRedemptionPayload = { order: this.orderPayload() };
      this.machine.submit();
      this.sync();
      this.sendPending();
      return;
    }
    if (!this.previewResult() || !this.credential) return;
    this.idempotencyKey.set(createUuid());
    this.pendingPayload = {
      ...this.credential,
      preview_id: this.previewResult()!.preview_id,
      order: this.orderPayload(),
    };
    this.machine.submit();
    this.sync();
    this.sendPending();
  }
  checkResult(): void {
    if (!this.pendingPayload && !this.pendingRedemptionPayload) return;
    this.machine.retryUncertainSubmission();
    this.sync();
    this.sendPending();
  }
  nextCustomer(): void {
    this.machine.nextCustomer();
    this.customer.set(null);
    this.previewResult.set(null);
    this.result.set(null);
    this.redemptionPreview.set(null);
    this.redemptionResult.set(null);
    this.pendingPayload = null;
    this.pendingRedemptionPayload = null;
    this.credential = null;
    this.uncertain.set(false);
    this.cancelling.set(false);
    this.showCancelForm.set(false);
    this.cancelForm.reset({ reason: "", restore_if_eligible: true });
    this.stopReservationClock();
    this.manualForm.reset();
    this.orderForm.reset({
      type: "stamp",
      gross_amount: 0,
      external_order_ref: "",
      catalog_item_id: "",
      item_name: "",
      quantity: 1,
      unit_price: 0,
    });
    this.sync();
    setTimeout(() => void this.startCamera());
  }
  stepIndex(): number {
    return this.steps.indexOf(this.state());
  }
  stepNumber(): number {
    return this.stepIndex() + 1;
  }
  stepLabel(step: ScanState): string {
    return {
      READY: "준비",
      SCANNING: "스캔",
      CUSTOMER_RESOLVED: "고객 확인",
      INPUT: "입력",
      REVIEW: "검토",
      SUBMITTING: "승인",
      SUCCESS: "성공",
      FAILURE: "실패",
    }[step];
  }
  cameraTitle(): string {
    return {
      unchecked: "카메라 준비 전",
      checking: "카메라 권한 확인 중",
      ready: "후면 카메라 준비됨",
      denied: "카메라 권한이 거부됨",
      unavailable: "카메라를 사용할 수 없음",
      insecure: "HTTPS 연결이 필요함",
    }[this.camera()];
  }
  cameraDescription(): string {
    return {
      unchecked: "잠시만 기다려 주세요.",
      checking: "브라우저 권한 요청에 응답해 주세요.",
      ready: "고객 QR을 프레임 안에 맞춰 주세요.",
      denied: "브라우저 설정에서 권한을 허용하거나 보조 코드를 이용하세요.",
      unavailable:
        "지원 브라우저나 다른 카메라를 사용하거나 보조 코드를 입력하세요.",
      insecure: "안전한 HTTPS 주소에서 다시 열거나 보조 코드를 입력하세요.",
    }[this.camera()];
  }
  date(value: string): string {
    return formatKoreaDateTime(value);
  }
  won(value: number): string {
    return formatWon(value);
  }
  transactionType(): "stamp" | "redeem" {
    return this.orderForm.controls.type.value;
  }
  reReserve(): void {
    if (!this.reservationView().expired || this.previewing()) return;
    this.previewing.set(true);
    this.previewError.set(null);
    this.requestRedemptionPreview(true);
  }
  canCancelRedemption(): boolean {
    const deadline = this.redemptionResult()?.cancellable_until;
    return Boolean(deadline && Date.parse(deadline) > this.now());
  }
  cancelRedemption(): void {
    const result = this.redemptionResult();
    if (
      !result ||
      !this.canCancelRedemption() ||
      this.cancelling() ||
      this.cancelForm.invalid
    ) {
      this.cancelForm.markAllAsTouched();
      return;
    }
    this.cancelling.set(true);
    const cancellation = this.cancelForm.getRawValue();
    this.api
      .cancelRedemption(
        result.redemption_id,
        {
          reason: cancellation.reason.trim(),
          restore_if_eligible: cancellation.restore_if_eligible,
        },
        createUuid(),
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (cancelled) => {
          this.redemptionResult.set(cancelled);
          this.cancelling.set(false);
          this.showCancelForm.set(false);
        },
        error: (error: unknown) => {
          this.failureMessage.set(
            error instanceof CouponClientError
              ? error.message
              : "취소 한도와 거래 상태를 확인해 주세요.",
          );
          this.cancelling.set(false);
        },
      });
  }

  private resolve(credential: {
    qr_token?: string;
    auxiliary_code?: string;
  }): void {
    this.resolving.set(true);
    this.resolveError.set(null);
    this.credential = credential;
    this.stopDecodeLoop();
    this.api
      .resolve(credential)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (customer) => {
          this.customer.set(customer);
          this.resolving.set(false);
          this.stopCamera();
          this.machine.customerResolved();
          this.sync();
        },
        error: (error: unknown) => {
          this.resolving.set(false);
          this.resolveError.set(
            messageOf(error, "QR 또는 보조 코드를 확인하지 못했습니다."),
          );
          this.machine.rejectDecodedFrame();
          this.sync();
          if (this.detector) this.decodeFrame();
        },
      });
  }
  private requestRedemptionPreview(replacing: boolean): void {
    const customer = this.customer();
    if (!customer) {
      this.previewing.set(false);
      return;
    }
    this.api
      .previewRedemption(
        {
          scan_session_id: customer.scan_session_id,
          order: this.orderPayload(),
        },
        createUuid(),
      )
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (review) => {
          this.previewResult.set(null);
          this.redemptionPreview.set(review);
          this.previewing.set(false);
          this.startReservationClock();
          if (!replacing) {
            this.machine.review();
            this.sync();
          }
        },
        error: (error: unknown) => {
          this.previewing.set(false);
          const clientError = error instanceof CouponClientError ? error : null;
          this.previewError.set(
            redemptionConditionMessage(
              clientError?.code ?? "UNKNOWN",
              clientError?.field_errors ?? [],
              clientError?.message ?? "쿠폰 사용 조건을 확인하지 못했습니다.",
            ),
          );
        },
      });
  }
  private sendPending(): void {
    if (this.transactionType() === "redeem") {
      const preview = this.redemptionPreview();
      if (!preview || !this.pendingRedemptionPayload) return;
      this.api
        .confirmRedemption(
          preview.redemption_id,
          this.pendingRedemptionPayload,
          this.idempotencyKey(),
        )
        .pipe(takeUntilDestroyed(this.destroyRef))
        .subscribe({
          next: (result) => {
            this.redemptionResult.set(result);
            this.uncertain.set(false);
            this.machine.succeed();
            this.sync();
          },
          error: (error: unknown) => this.handleSubmissionFailure(error),
        });
      return;
    }
    this.api
      .submit(this.pendingPayload!, this.idempotencyKey())
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (result) => {
          this.result.set(result);
          this.uncertain.set(false);
          this.machine.succeed();
          this.sync();
        },
        error: (error: unknown) => this.handleSubmissionFailure(error),
      });
  }
  private handleSubmissionFailure(error: unknown): void {
    const status =
      error instanceof CouponClientError
        ? error.status
        : error instanceof HttpErrorResponse
          ? error.status
          : -1;
    this.uncertain.set(status === 0);
    const fallback =
      this.transactionType() === "redeem"
        ? "쿠폰 사용 승인 조건을 충족하지 못했습니다."
        : "도장 적립 승인 조건을 충족하지 못했습니다.";
    this.failureMessage.set(
      status === 0
        ? "승인 응답이 도착하지 않아 실제 반영 여부가 불확실합니다."
        : error instanceof CouponClientError
          ? redemptionConditionMessage(
              error.code,
              error.field_errors,
              error.message,
            )
          : fallback,
    );
    this.machine.fail();
    this.sync();
  }
  private startReservationClock(): void {
    this.stopReservationClock();
    this.now.set(Date.now());
    this.reservationTimer = setInterval(() => this.now.set(Date.now()), 1_000);
  }
  private stopReservationClock(): void {
    if (this.reservationTimer !== null) clearInterval(this.reservationTimer);
    this.reservationTimer = null;
  }
  private orderPayload() {
    const value = this.orderForm.getRawValue();
    return {
      external_order_ref: value.external_order_ref || null,
      gross_amount: value.gross_amount,
      currency: "KRW" as const,
      items: [
        {
          catalog_item_id: value.catalog_item_id || null,
          name_snapshot: value.item_name,
          quantity: value.quantity,
          unit_price: value.unit_price,
        },
      ],
    };
  }
  private prepareDetector(): void {
    const Constructor = (
      window as unknown as { BarcodeDetector?: BarcodeDetectorConstructor }
    ).BarcodeDetector;
    if (!Constructor) {
      this.detectorAvailable.set(false);
      return;
    }
    this.detector = new Constructor({ formats: ["qr_code"] });
    this.detectorAvailable.set(true);
    this.decodeFrame();
  }
  private decodeFrame(): void {
    if (
      !this.detector ||
      this.machine.state !== "SCANNING" ||
      this.machine.frameLocked
    )
      return;
    const video = this.video()?.nativeElement;
    if (!video || video.readyState < 2) {
      this.frameId = requestAnimationFrame(() => this.decodeFrame());
      return;
    }
    this.detector
      .detect(video)
      .then((codes) => {
        const token = codes[0]?.rawValue;
        if (token && this.machine.lockDecodedFrame()) {
          this.sync();
          this.resolve({ qr_token: token });
          return;
        }
        this.frameId = requestAnimationFrame(() => this.decodeFrame());
      })
      .catch(() => {
        this.frameId = requestAnimationFrame(() => this.decodeFrame());
      });
  }
  private stopDecodeLoop(): void {
    if (this.frameId !== null) cancelAnimationFrame(this.frameId);
    this.frameId = null;
  }
  private stopCamera(): void {
    this.stopDecodeLoop();
    this.stream?.getTracks().forEach((track) => track.stop());
    this.stream = null;
    const video = this.video()?.nativeElement;
    if (video) video.srcObject = null;
  }
  private sync(): void {
    this.state.set(this.machine.state);
    this.camera.set(this.machine.camera);
  }
}

function messageOf(error: unknown, fallback: string): string {
  return error instanceof CouponClientError ? error.message : fallback;
}
function createUuid(): string {
  return typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
        const r = (Math.random() * 16) | 0;
        return (c === "x" ? r : (r & 3) | 8).toString(16);
      });
}
