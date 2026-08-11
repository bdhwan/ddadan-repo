import { Routes } from "@angular/router";
import {
  CouponAuthRouteComponent,
  CouponFeatureStateComponent,
} from "@coupon/ui";
import { ConsumerShellComponent } from "./consumer-shell.component";
import { MyQrComponent } from "./my-qr.component";
import { WalletComponent } from "./wallet.component";
import { NotificationsComponent } from "./notifications.component";
import { StoreDetailComponent } from "./store-detail.component";

const authRoutes: Routes = [
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
];

export const routes: Routes = [
  ...authRoutes,
  {
    path: "",
    component: ConsumerShellComponent,
    children: [
      {
        path: "",
        component: CouponFeatureStateComponent,
        data: {
          title: "홈",
          description: "만료 임박 혜택과 도장판을 우선해 보여줄 영역입니다.",
          emptyTitle: "아직 혜택이 없어요",
          emptyDescription: "관심 상점이나 매장 QR로 첫 혜택을 만나보세요.",
        },
      },
      { path: "wallet", component: WalletComponent },
      { path: "my-qr", component: MyQrComponent },
      { path: "notifications", component: NotificationsComponent },
      {
        path: "account",
        component: CouponFeatureStateComponent,
        data: {
          title: "내 정보",
          description: "계정, 보안, 동의, 알림 설정을 관리합니다.",
        },
      },
      {
        path: "stores/:slug",
        component: StoreDetailComponent,
      },
    ],
  },
  { path: "**", redirectTo: "" },
];
