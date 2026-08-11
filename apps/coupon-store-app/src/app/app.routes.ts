import { Routes } from '@angular/router';
import { CouponAuthRouteComponent } from '@coupon/ui';
import { onboardingLeaveGuard, StoreOnboardingComponent } from './store-onboarding.component';
import { StoreFeatureStateComponent } from './store-feature-state.component';
import { StoreShellComponent } from './store-shell.component';

export const routes: Routes = [
  { path: 'login', component: CouponAuthRouteComponent, data: { mode: 'login' } },
  { path: 'signup', component: CouponAuthRouteComponent, data: { mode: 'signup' } },
  { path: 'auth/kakao/callback', component: CouponAuthRouteComponent, data: { mode: 'kakao' } },
  { path: 'verify-email', component: CouponAuthRouteComponent, data: { mode: 'verify' } },
  { path: 'terms', component: CouponAuthRouteComponent, data: { mode: 'terms' } },
  { path: 'account/security', component: CouponAuthRouteComponent, data: { mode: 'security' } },
  { path: 'account/notifications', component: CouponAuthRouteComponent, data: { mode: 'notifications' } },
  { path: 'account/withdraw', component: CouponAuthRouteComponent, data: { mode: 'withdraw' } },
  { path: 'onboarding/store', component: StoreOnboardingComponent, canDeactivate: [onboardingLeaveGuard] },
  {
    path: '',
    component: StoreShellComponent,
    children: [
      { path: '', pathMatch: 'full', redirectTo: 'dashboard' },
      { path: 'dashboard', component: StoreFeatureStateComponent, data: { title: '오늘', description: '적립·사용·취소와 운영 이상을 요약합니다.', emptyTitle: '아직 집계할 거래가 없어요', emptyDescription: '집계가 진행 중이면 0이 아닌 ‘집계 중’으로 표시됩니다.' } },
      { path: 'scan', component: StoreFeatureStateComponent, data: { title: '스캔', description: 'READY에서 SUCCESS·FAILURE까지의 안전한 스캔 상태 머신 영역입니다.', emptyTitle: '카메라를 준비해 주세요', emptyDescription: 'HTTPS와 카메라 권한을 확인한 뒤 고객 QR을 스캔합니다.' } },
      { path: 'loyalty', component: StoreFeatureStateComponent, data: { title: '도장 정책', description: '현재, 예약, 과거 버전을 구분합니다.' } },
      { path: 'campaigns', component: StoreFeatureStateComponent, data: { title: '할인 캠페인', description: '혜택부터 검토까지의 작성 흐름 영역입니다.' } },
      { path: 'customers', component: StoreFeatureStateComponent, data: { title: '고객', description: '이 상점의 가명 고객 정보만 표시합니다.' } },
      { path: 'analytics', component: StoreFeatureStateComponent, data: { title: '통계', description: '실시간 잠정치와 일 배치 확정치를 구분합니다.' } },
      { path: 'settings', component: StoreFeatureStateComponent, data: { title: '상점 설정', description: '상점 정보와 검수 상태를 관리합니다.' } },
    ],
  },
  { path: '**', redirectTo: 'dashboard' },
];
