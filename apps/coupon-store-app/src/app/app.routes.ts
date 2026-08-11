import { Routes } from "@angular/router";
import { CouponAuthRouteComponent } from "@coupon/ui";
import {
  onboardingLeaveGuard,
  StoreOnboardingComponent,
} from "./store-onboarding.component";
import { StoreFeatureStateComponent } from "./store-feature-state.component";
import { StoreShellComponent } from "./store-shell.component";
import { StoreDashboardComponent } from "./store-dashboard.component";
import { StoreScanComponent } from "./store-scan.component";
import { LoyaltyComponent } from "./loyalty.component";
import { CatalogComponent } from "./catalog.component";
import { CampaignProgressComponent } from "./campaign-progress.component";

export const routes: Routes = [
  {
    path: "login",
    component: CouponAuthRouteComponent,
    data: { mode: "login" },
  },
  {
    path: "signup",
    component: CouponAuthRouteComponent,
    data: { mode: "signup" },
  },
  {
    path: "auth/kakao/callback",
    component: CouponAuthRouteComponent,
    data: { mode: "kakao" },
  },
  {
    path: "verify-email",
    component: CouponAuthRouteComponent,
    data: { mode: "verify" },
  },
  {
    path: "terms",
    component: CouponAuthRouteComponent,
    data: { mode: "terms" },
  },
  {
    path: "account/security",
    component: CouponAuthRouteComponent,
    data: { mode: "security" },
  },
  {
    path: "account/notifications",
    component: CouponAuthRouteComponent,
    data: { mode: "notifications" },
  },
  {
    path: "account/withdraw",
    component: CouponAuthRouteComponent,
    data: { mode: "withdraw" },
  },
  {
    path: "onboarding/store",
    component: StoreOnboardingComponent,
    canDeactivate: [onboardingLeaveGuard],
  },
  {
    path: "",
    component: StoreShellComponent,
    children: [
      { path: "", pathMatch: "full", redirectTo: "dashboard" },
      { path: "dashboard", component: StoreDashboardComponent },
      { path: "scan", component: StoreScanComponent },
      { path: "loyalty", component: LoyaltyComponent },
      { path: "catalog", component: CatalogComponent },
      { path: "campaigns", component: CampaignProgressComponent },
      {
        path: "customers",
        component: StoreFeatureStateComponent,
        data: {
          title: "고객",
          description: "이 상점의 가명 고객 정보만 표시합니다.",
        },
      },
      {
        path: "analytics",
        component: StoreFeatureStateComponent,
        data: {
          title: "통계",
          description: "실시간 잠정치와 일 배치 확정치를 구분합니다.",
        },
      },
      {
        path: "settings",
        component: StoreFeatureStateComponent,
        data: {
          title: "상점 설정",
          description: "상점 정보와 검수 상태를 관리합니다.",
        },
      },
    ],
  },
  { path: "**", redirectTo: "dashboard" },
];
