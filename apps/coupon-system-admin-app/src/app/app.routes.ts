import { Routes } from '@angular/router';
import { CouponFeatureStateComponent } from '@coupon/ui';
import { AdminLoginComponent } from './admin-login.component';
import { AdminReviewQueueComponent } from './admin-review-queue.component';
import { AdminShellComponent } from './admin-shell.component';
import { AdminTransactionExplorerComponent } from './admin-transaction-explorer.component';

export const routes: Routes = [
  { path: 'login', component: AdminLoginComponent },
  {
    path: '',
    component: AdminShellComponent,
    children: [
      { path: '', pathMatch: 'full', redirectTo: 'store-reviews' },
      { path: 'operations', component: CouponFeatureStateComponent, data: { title: '운영 현황', description: 'API·DB·Redis·worker·알림 상태 shell입니다.' } },
      { path: 'store-reviews', component: AdminReviewQueueComponent },
      { path: 'members', component: CouponFeatureStateComponent, data: { title: '회원·상점', description: '상태와 역할, 제재를 확인합니다.' } },
      { path: 'transactions', component: AdminTransactionExplorerComponent },
      { path: 'jobs', component: CouponFeatureStateComponent, data: { title: '작업 큐', description: '작업 키, 시도, 체크포인트, 오류를 표시합니다.' } },
      { path: 'audit', component: CouponFeatureStateComponent, data: { title: '감사', description: '관리자 조회·변경 로그를 확인합니다.' } },
    ],
  },
  { path: '**', redirectTo: 'store-reviews' },
];
