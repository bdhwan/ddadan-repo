import {
  ChangeDetectionStrategy,
  Component,
  inject,
  signal,
} from "@angular/core";
import { ActivatedRoute } from "@angular/router";
import { CouponButtonComponent } from "./button.component";
import { CouponCardComponent } from "./card.component";
import { CouponEmptyStateComponent } from "./empty-state.component";
import { CouponErrorStateComponent } from "./error-state.component";
import { CouponPageHeaderComponent } from "./page-header.component";
import { CouponSkeletonComponent } from "./skeleton.component";

@Component({
  selector: "coupon-feature-state",
  imports: [
    CouponButtonComponent,
    CouponCardComponent,
    CouponEmptyStateComponent,
    CouponErrorStateComponent,
    CouponPageHeaderComponent,
    CouponSkeletonComponent,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <coupon-page-header [title]="title" [description]="description" />
    <div class="state-controls" role="group" aria-label="화면 상태 미리보기">
      <coupon-button variant="secondary" (click)="state.set('loading')"
        >로딩</coupon-button
      >
      <coupon-button variant="secondary" (click)="state.set('empty')"
        >빈 상태</coupon-button
      >
      <coupon-button variant="secondary" (click)="state.set('error')"
        >오류</coupon-button
      >
    </div>
    <coupon-card>
      @switch (state()) {
        @case ("loading") {
          <coupon-skeleton [lines]="5" />
        }
        @case ("error") {
          <coupon-error-state
            requestId="phase1-demo"
            (retry)="state.set('loading')"
          />
        }
        @default {
          <coupon-empty-state
            [title]="emptyTitle"
            [description]="emptyDescription"
          />
        }
      }
    </coupon-card>
  `,
  styles: `
    .state-controls {
      display: flex;
      flex-wrap: wrap;
      gap: 0.5rem;
      margin-bottom: 1rem;
    }
  `,
})
export class CouponFeatureStateComponent {
  private readonly route = inject(ActivatedRoute);
  readonly state = signal<"loading" | "empty" | "error">("empty");
  readonly title = String(this.route.snapshot.data["title"] ?? "준비 중");
  readonly description = String(
    this.route.snapshot.data["description"] ?? "Phase 1 화면 shell입니다.",
  );
  readonly emptyTitle = String(
    this.route.snapshot.data["emptyTitle"] ?? "아직 표시할 내용이 없어요",
  );
  readonly emptyDescription = String(
    this.route.snapshot.data["emptyDescription"] ??
      "데이터가 생기면 이곳에 보여드릴게요.",
  );
}
