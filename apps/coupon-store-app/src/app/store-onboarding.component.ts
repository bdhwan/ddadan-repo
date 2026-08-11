import { HttpErrorResponse } from '@angular/common/http';
import { ChangeDetectionStrategy, Component, HostListener, OnInit, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { CanDeactivateFn, RouterLink } from '@angular/router';
import type { OwnerStoreDto, SaveOwnerStoreRequestDto } from '@coupon/contracts';
import {
  CouponBadgeComponent,
  CouponButtonComponent,
  CouponCardComponent,
  CouponErrorStateComponent,
  CouponPageHeaderComponent,
  CouponSkeletonComponent,
} from '@coupon/ui';
import { finalize } from 'rxjs';
import { StoreOnboardingApi } from './store-onboarding.api';

const DRAFT_KEY = 'coupon-store-onboarding-draft-v1';
const STEP_LABELS = ['기본 정보', '사업자 정보', '영업 설정', '약관', '검수 제출'] as const;

@Component({
  selector: 'coupon-store-onboarding',
  imports: [
    ReactiveFormsModule,
    RouterLink,
    CouponBadgeComponent,
    CouponButtonComponent,
    CouponCardComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <header class="topbar">
      <a routerLink="/dashboard" aria-label="다단 상점 대시보드로 돌아가기"><span aria-hidden="true">←</span> 대시보드</a>
      <coupon-badge [status]="reviewBadgeStatus()" [label]="reviewLabel()">{{ reviewLabel() }}</coupon-badge>
    </header>
    <main id="main-content">
      <coupon-page-header
        title="상점 등록"
        description="각 단계는 로컬에 즉시 임시저장되며, ‘저장 후 다음’을 누르면 서버 초안에 반영됩니다."
        eyebrow="5-step onboarding"
      />

      <div class="onboarding-layout">
        <nav aria-label="상점 등록 단계">
          <ol>
            @for (label of stepLabels; track label; let index = $index) {
              <li [class.current]="step() === index" [class.complete]="step() > index">
                <button type="button" (click)="goToStep(index)" [attr.aria-current]="step() === index ? 'step' : null">
                  <span aria-hidden="true">{{ step() > index ? '✓' : index + 1 }}</span>
                  {{ label }}
                </button>
              </li>
            }
          </ol>
        </nav>

        <section>
          @if (loading()) {
            <coupon-card><coupon-skeleton [lines]="6" label="상점 초안을 불러오는 중입니다." /></coupon-card>
          } @else {
            @if (errorMessage()) {
              <coupon-error-state
                title="임시저장을 완료하지 못했어요"
                [description]="errorMessage()!"
                [requestId]="requestId()"
                [retryable]="true"
                (retry)="saveStep(false)"
              />
            }

            <form [formGroup]="form" (ngSubmit)="saveStep(true)">
              <coupon-card>
                @switch (step()) {
                  @case (0) {
                    <fieldset>
                      <legend>1. 기본 정보</legend>
                      <p>고객에게 보일 상점 정보를 입력하세요.</p>
                      <label>상점명 <span aria-hidden="true">*</span><input formControlName="name" autocomplete="organization" /></label>
                      <label>상점 소개<textarea formControlName="description" rows="4"></textarea></label>
                      <label>주소<input formControlName="address" autocomplete="street-address" /></label>
                    </fieldset>
                  }
                  @case (1) {
                    <fieldset>
                      <legend>2. 사업자 정보</legend>
                      <p>저장 후 민감정보는 마스킹된 값만 표시합니다.</p>
                      <label>사업자등록번호 <span aria-hidden="true">*</span><input formControlName="business_registration_number" inputmode="numeric" autocomplete="off" /></label>
                      <label>대표자명 <span aria-hidden="true">*</span><input formControlName="representative_name" autocomplete="off" /></label>
                      @if (store()) {
                        <div class="masked" role="status"><strong>저장된 값</strong><span>{{ store()?.business_registration_number_masked ?? maskRegistration(form.controls.business_registration_number.value) }}</span><span>{{ store()?.representative_name_masked ?? maskName(form.controls.representative_name.value) }}</span></div>
                      }
                    </fieldset>
                  }
                  @case (2) {
                    <fieldset>
                      <legend>3. 영업 설정</legend>
                      <p>안내용 영업시간이며, 쿠폰 사용 조건은 개별 정책을 우선합니다.</p>
                      <div class="hours">
                        <label>영업 시작<input type="time" formControlName="opens_at" /></label>
                        <label>영업 종료<input type="time" formControlName="closes_at" /></label>
                      </div>
                      <p class="timezone">타임존 <strong>Asia/Seoul</strong></p>
                    </fieldset>
                  }
                  @case (3) {
                    <fieldset>
                      <legend>4. 약관</legend>
                      <p>필수와 선택 동의를 구분합니다.</p>
                      <label class="check"><input type="checkbox" formControlName="accepted_required_terms" /><span><strong>[필수] 상점 이용약관과 개인정보 처리방침</strong><small>버전 phase1-2026-08 · 전문 링크는 배포 시 연결</small></span></label>
                      <label class="check"><input type="checkbox" formControlName="accepted_marketing" /><span><strong>[선택] 운영·마케팅 알림</strong><small>동의하지 않아도 상점 서비스를 이용할 수 있습니다.</small></span></label>
                    </fieldset>
                  }
                  @case (4) {
                    <fieldset>
                      <legend>5. 검수 제출</legend>
                      <p>제출 전에 저장된 정보와 검수 흐름을 확인하세요.</p>
                      <dl>
                        <div><dt>상점명</dt><dd>{{ form.controls.name.value || '미입력' }}</dd></div>
                        <div><dt>주소</dt><dd>{{ form.controls.address.value || '미입력' }}</dd></div>
                        <div><dt>사업자번호</dt><dd>{{ store()?.business_registration_number_masked ?? maskRegistration(form.controls.business_registration_number.value) }}</dd></div>
                        <div><dt>대표자</dt><dd>{{ store()?.representative_name_masked ?? maskName(form.controls.representative_name.value) }}</dd></div>
                        <div><dt>영업시간</dt><dd>{{ form.controls.opens_at.value }}–{{ form.controls.closes_at.value }}</dd></div>
                      </dl>
                      <ol class="timeline" aria-label="검수 상태">
                        <li class="done"><span aria-hidden="true">✓</span><div><strong>초안 작성</strong><p>현재 정보를 안전하게 저장합니다.</p></div></li>
                        <li [class.done]="reviewSubmitted()"><span aria-hidden="true">{{ reviewSubmitted() ? '✓' : '2' }}</span><div><strong>검수 제출</strong><p>제출 후 보완 요청이 오면 이 화면에 사유가 표시됩니다.</p></div></li>
                        <li><span aria-hidden="true">3</span><div><strong>승인·보완·거절</strong><p>운영 결정과 사유를 안내합니다.</p></div></li>
                      </ol>
                    </fieldset>
                  }
                }
              </coupon-card>

              <div class="actions">
                <coupon-button variant="secondary" [disabled]="step() === 0 || saving()" (click)="previousStep()">이전</coupon-button>
                @if (step() < 4) {
                  <coupon-button type="submit" [disabled]="saving()">{{ saving() ? '저장 중…' : '저장 후 다음' }}</coupon-button>
                } @else {
                  <coupon-button [disabled]="saving() || reviewSubmitted()" (click)="submitReview()">{{ saving() ? '제출 중…' : reviewSubmitted() ? '검수 제출 완료' : '검수 제출' }}</coupon-button>
                }
              </div>
              <p class="save-status" aria-live="polite">{{ statusMessage() }}</p>
            </form>
          }
        </section>
      </div>
    </main>
  `,
  styles: `
    :host { display: block; min-height: 100dvh; }
    .topbar { display: flex; align-items: center; justify-content: space-between; min-height: 58px; padding: 0 1rem; border-bottom: 1px solid var(--coupon-color-border); background: var(--coupon-color-surface); }
    .topbar a { display: inline-flex; align-items: center; gap: .4rem; min-height: 44px; text-decoration: none; font-weight: 800; }
    main { width: min(100% - 2rem, 76rem); margin: 0 auto; padding: 1.5rem 0 4rem; }
    .onboarding-layout { display: grid; gap: 1rem; }
    nav ol { display: grid; grid-template-columns: repeat(5, minmax(7rem, 1fr)); gap: .35rem; margin: 0; padding: 0 0 .5rem; overflow-x: auto; list-style: none; }
    nav button { display: flex; align-items: center; gap: .4rem; width: 100%; min-height: 48px; padding: .45rem; border: 0; border-bottom: 3px solid var(--coupon-color-border); background: transparent; color: var(--coupon-color-text-muted); text-align: left; font-weight: 700; cursor: pointer; }
    nav button > span { display: inline-grid; place-items: center; flex: 0 0 1.75rem; height: 1.75rem; border: 1px solid currentColor; border-radius: 50%; }
    nav li.current button { border-color: var(--coupon-color-primary); color: var(--coupon-color-primary); }
    nav li.complete button { color: var(--coupon-color-success); }
    form { display: grid; gap: 1rem; }
    fieldset { display: grid; gap: 1rem; margin: 0; padding: 0; border: 0; }
    legend { margin-bottom: .35rem; padding: 0; font-size: var(--coupon-font-size-lg); font-weight: 900; }
    fieldset > p { margin: 0; color: var(--coupon-color-text-muted); }
    label { display: grid; gap: .4rem; font-weight: 700; }
    input, textarea { width: 100%; min-height: 44px; padding: .65rem .75rem; border: 1px solid var(--coupon-color-border); border-radius: var(--coupon-radius-sm); background: var(--coupon-color-bg); color: var(--coupon-color-text); }
    textarea { resize: vertical; }
    .hours { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
    .timezone { padding: .75rem; border-radius: var(--coupon-radius-sm); background: var(--coupon-color-surface-muted); }
    .check { grid-template-columns: 44px 1fr; align-items: start; padding: .75rem; border: 1px solid var(--coupon-color-border); border-radius: var(--coupon-radius-sm); }
    .check input { width: 24px; height: 24px; margin: 10px; }
    .check span { display: grid; gap: .25rem; }
    .check small { color: var(--coupon-color-text-muted); font-weight: 400; }
    .masked { display: flex; flex-wrap: wrap; gap: .75rem; padding: .75rem; border-radius: var(--coupon-radius-sm); background: var(--coupon-color-surface-muted); }
    dl { display: grid; margin: 0; border: 1px solid var(--coupon-color-border); border-radius: var(--coupon-radius-sm); overflow: hidden; }
    dl div { display: grid; grid-template-columns: 8rem 1fr; border-bottom: 1px solid var(--coupon-color-border); }
    dl div:last-child { border-bottom: 0; }
    dt, dd { margin: 0; padding: .7rem; }
    dt { background: var(--coupon-color-surface-muted); font-weight: 800; }
    .timeline { display: grid; gap: 1rem; margin: 0; padding: 0; list-style: none; }
    .timeline li { display: grid; grid-template-columns: 2rem 1fr; gap: .75rem; color: var(--coupon-color-text-muted); }
    .timeline li > span { display: grid; place-items: center; width: 2rem; height: 2rem; border: 1px solid currentColor; border-radius: 50%; font-weight: 800; }
    .timeline li.done { color: var(--coupon-color-success); }
    .timeline p { margin: .2rem 0 0; color: var(--coupon-color-text-muted); }
    .actions { display: flex; justify-content: space-between; gap: .75rem; }
    .save-status { min-height: 1.5rem; margin: 0; color: var(--coupon-color-text-muted); text-align: right; }
    @media (min-width: 768px) {
      main { padding-top: 2rem; }
      .onboarding-layout { grid-template-columns: 14rem minmax(0, 1fr); align-items: start; gap: 2rem; }
      nav { position: sticky; top: 1rem; }
      nav ol { grid-template-columns: 1fr; overflow: visible; }
      nav button { border-bottom: 0; border-left: 3px solid var(--coupon-color-border); padding: .65rem; }
    }
  `,
})
export class StoreOnboardingComponent implements OnInit {
  private readonly api = inject(StoreOnboardingApi);
  private readonly formBuilder = inject(FormBuilder);

  readonly stepLabels = STEP_LABELS;
  readonly step = signal(0);
  readonly loading = signal(true);
  readonly saving = signal(false);
  readonly store = signal<OwnerStoreDto | null>(null);
  readonly errorMessage = signal<string | null>(null);
  readonly requestId = signal<string | null>(null);
  readonly statusMessage = signal('');
  readonly isDirty = signal(false);
  readonly reviewSubmitted = signal(false);

  readonly form = this.formBuilder.nonNullable.group({
    name: ['', [Validators.required, Validators.maxLength(80)]],
    description: ['', Validators.maxLength(500)],
    address: ['', Validators.maxLength(200)],
    business_registration_number: ['', [Validators.required, Validators.pattern(/^\d{10}$/)]],
    representative_name: ['', [Validators.required, Validators.maxLength(80)]],
    opens_at: ['09:00', Validators.required],
    closes_at: ['18:00', Validators.required],
    accepted_required_terms: [false, Validators.requiredTrue],
    accepted_marketing: [false],
  });

  constructor() {
    this.form.valueChanges.pipe(takeUntilDestroyed()).subscribe(() => {
      this.isDirty.set(true);
      this.saveLocally();
      this.statusMessage.set('이 기기에 임시저장됨');
    });
  }

  ngOnInit(): void {
    this.restoreLocalDraft();
    this.api
      .load()
      .pipe(finalize(() => this.loading.set(false)))
      .subscribe({
        next: (response) => {
          this.applyStore(response.store);
          this.requestId.set(response.request_id);
          this.statusMessage.set('서버 초안을 불러왔습니다.');
        },
        error: (error: unknown) => {
          const status = error instanceof HttpErrorResponse
            ? error.status
            : typeof error === 'object' && error !== null && 'status' in error
              ? (error as { status?: unknown }).status
              : null;
          if (status === 404) {
            this.statusMessage.set('새 상점 초안입니다.');
            return;
          }
          // Client-core may already have converted this to a safe message.
          this.statusMessage.set('로컬 임시저장본으로 계속합니다.');
        },
      });
  }

  @HostListener('window:beforeunload', ['$event'])
  warnBeforeUnload(event: BeforeUnloadEvent): void {
    if (this.isDirty()) {
      event.preventDefault();
    }
  }

  canLeave(): boolean {
    return !this.isDirty() || window.confirm('저장하지 않은 변경 내용이 있습니다. 화면을 나갈까요?');
  }

  goToStep(index: number): void {
    if (index >= 0 && index < STEP_LABELS.length) {
      this.step.set(index);
      window.scrollTo({ top: 0, behavior: 'smooth' });
    }
  }

  previousStep(): void {
    this.step.update((value) => Math.max(0, value - 1));
  }

  saveStep(advance: boolean): void {
    this.errorMessage.set(null);
    if (!this.isCurrentStepValid()) {
      this.statusMessage.set('필수 항목을 확인해 주세요.');
      return;
    }

    this.saving.set(true);
    const request = this.store()
      ? this.api.update(this.payload())
      : this.api.create(this.payload());
    request.pipe(finalize(() => this.saving.set(false))).subscribe({
      next: (response) => {
        this.store.set(response.store);
        this.requestId.set(response.request_id);
        this.isDirty.set(false);
        this.statusMessage.set('서버에 임시저장했습니다.');
        if (advance) {
          this.step.update((value) => Math.min(STEP_LABELS.length - 1, value + 1));
        }
      },
      error: (error: unknown) => this.captureError(error),
    });
  }

  submitReview(): void {
    const store = this.store();
    if (!store) {
      this.errorMessage.set('검수 제출 전에 초안을 한 번 저장해 주세요.');
      return;
    }
    if (!this.form.controls.accepted_required_terms.valid) {
      this.errorMessage.set('필수 약관 동의가 필요합니다.');
      return;
    }

    this.saving.set(true);
    this.errorMessage.set(null);
    this.api
      .submitReview({ version: store.version })
      .pipe(finalize(() => this.saving.set(false)))
      .subscribe({
        next: (response) => {
          this.store.set(response.store);
          this.requestId.set(response.request_id);
          this.reviewSubmitted.set(true);
          this.isDirty.set(false);
          localStorage.removeItem(DRAFT_KEY);
          this.statusMessage.set('검수를 제출했습니다. 상태 변경을 이 화면에서 안내합니다.');
        },
        error: (error: unknown) => this.captureError(error),
      });
  }

  reviewLabel(): string {
    const labels: Record<string, string> = {
      DRAFT: '초안',
      IN_REVIEW: '검수 중',
      CHANGES_REQUESTED: '보완 필요',
      APPROVED: '승인',
      REJECTED: '거절',
      SUSPENDED: '정지',
    };
    return labels[this.store()?.review_status ?? 'DRAFT'] ?? '초안';
  }

  reviewBadgeStatus(): 'success' | 'warning' | 'danger' | 'neutral' {
    const status = this.store()?.review_status;
    if (status === 'APPROVED') return 'success';
    if (status === 'CHANGES_REQUESTED' || status === 'IN_REVIEW') return 'warning';
    if (status === 'REJECTED' || status === 'SUSPENDED') return 'danger';
    return 'neutral';
  }

  maskRegistration(value: string): string {
    const digits = value.replace(/\D/g, '');
    return digits.length === 10 ? `${digits.slice(0, 3)}-••-${digits.slice(-5)}` : '미저장';
  }

  maskName(value: string): string {
    return value.length > 1 ? `${value[0]}•${value.at(-1)}` : value || '미저장';
  }

  private isCurrentStepValid(): boolean {
    const controlsByStep = [
      [this.form.controls.name],
      [this.form.controls.business_registration_number, this.form.controls.representative_name],
      [this.form.controls.opens_at, this.form.controls.closes_at],
      [this.form.controls.accepted_required_terms],
      [],
    ];
    const controls = controlsByStep[this.step()] ?? [];
    controls.forEach((control) => control.markAsTouched());
    return controls.every((control) => control.valid);
  }

  private payload(): SaveOwnerStoreRequestDto {
    const value = this.form.getRawValue();
    return {
      name: value.name,
      description: value.description || null,
      address: value.address || null,
      business_registration_number: value.business_registration_number,
      representative_name: value.representative_name,
      timezone: 'Asia/Seoul',
      business_hours: [1, 2, 3, 4, 5].map((day) => ({
        day_of_week: day as 1 | 2 | 3 | 4 | 5,
        opens_at: value.opens_at,
        closes_at: value.closes_at,
        closed: false,
      })),
      ...(value.accepted_required_terms ? { accepted_terms_version: 'phase1-2026-08' } : {}),
      ...(this.store() ? { version: this.store()!.version } : {}),
    };
  }

  private applyStore(store: OwnerStoreDto): void {
    this.store.set(store);
    this.reviewSubmitted.set(store.review_status === 'IN_REVIEW' || store.review_status === 'APPROVED');
    this.form.patchValue(
      {
        name: store.name,
        description: store.description ?? '',
        address: store.address ?? '',
        opens_at: store.business_hours[0]?.opens_at ?? '09:00',
        closes_at: store.business_hours[0]?.closes_at ?? '18:00',
      },
      { emitEvent: false },
    );
    this.isDirty.set(false);
  }

  private saveLocally(): void {
    localStorage.setItem(DRAFT_KEY, JSON.stringify(this.form.getRawValue()));
  }

  private restoreLocalDraft(): void {
    const raw = localStorage.getItem(DRAFT_KEY);
    if (!raw) return;
    try {
      this.form.patchValue(JSON.parse(raw) as Partial<ReturnType<typeof this.form.getRawValue>>, { emitEvent: false });
      this.statusMessage.set('이 기기의 임시저장본을 복원했습니다.');
    } catch {
      localStorage.removeItem(DRAFT_KEY);
    }
  }

  private captureError(error: unknown): void {
    const safe = error instanceof Error ? error.message : '서버에 저장하지 못했습니다.';
    this.errorMessage.set(safe);
    if (typeof error === 'object' && error !== null && 'request_id' in error) {
      const requestId = (error as { request_id?: unknown }).request_id;
      this.requestId.set(typeof requestId === 'string' ? requestId : null);
    }
    this.statusMessage.set('로컬 임시저장은 유지됩니다.');
  }
}

export const onboardingLeaveGuard: CanDeactivateFn<StoreOnboardingComponent> = (component) => component.canLeave();
