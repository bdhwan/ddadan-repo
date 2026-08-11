import { Routes } from '@angular/router';
import { CouponAuthRouteComponent, CouponFeatureStateComponent } from '@coupon/ui';
import { ConsumerShellComponent } from './consumer-shell.component';

const authRoutes: Routes = [
  { path: 'login', component: CouponAuthRouteComponent, data: { mode: 'login' } },
  { path: 'signup', component: CouponAuthRouteComponent, data: { mode: 'signup' } },
  { path: 'auth/kakao/callback', component: CouponAuthRouteComponent, data: { mode: 'kakao' } },
  { path: 'verify-email', component: CouponAuthRouteComponent, data: { mode: 'verify' } },
  { path: 'terms', component: CouponAuthRouteComponent, data: { mode: 'terms' } },
  { path: 'account/security', component: CouponAuthRouteComponent, data: { mode: 'security' } },
  { path: 'account/notifications', component: CouponAuthRouteComponent, data: { mode: 'notifications' } },
  { path: 'account/withdraw', component: CouponAuthRouteComponent, data: { mode: 'withdraw' } },
];

export const routes: Routes = [
  ...authRoutes,
  {
    path: '',
    component: ConsumerShellComponent,
    children: [
      { path: '', component: CouponFeatureStateComponent, data: { title: '홈', description: '만료 임박 혜택과 도장판을 우선해 보여줄 영역입니다.', emptyTitle: '아직 혜택이 없어요', emptyDescription: '관심 상점이나 매장 QR로 첫 혜택을 만나보세요.' } },
      { path: 'wallet', component: CouponFeatureStateComponent, data: { title: '지갑', description: '사용 가능, 도장, 사용·만료 내역을 확인합니다.', emptyTitle: '지갑이 비어 있어요', emptyDescription: '쿠폰을 받거나 도장을 적립하면 이곳에 표시됩니다.' } },
      { path: 'my-qr', component: CouponFeatureStateComponent, data: { title: '내 QR', description: '60초 회전형 QR과 8자리 보조 코드 영역입니다.', emptyTitle: 'QR을 준비할 수 없어요', emptyDescription: '온라인 상태와 약관 동의를 확인해 주세요. 결제 QR이 아닙니다.' } },
      { path: 'notifications', component: CouponFeatureStateComponent, data: { title: '알림', description: '거래, 혜택, 보안, 운영 공지를 구분해 보여줍니다.', emptyTitle: '새 알림이 없어요', emptyDescription: '알림을 삭제해도 거래와 쿠폰은 유지됩니다.' } },
      { path: 'account', component: CouponFeatureStateComponent, data: { title: '내 정보', description: '계정, 보안, 동의, 알림 설정을 관리합니다.' } },
      { path: 'stores/:slug', component: CouponFeatureStateComponent, data: { title: '상점 상세', description: '공개 정책과 캠페인 영역입니다.' } },
    ],
  },
  { path: '**', redirectTo: '' },
];
