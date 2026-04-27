import { Routes } from '@angular/router';
import { authGuard } from './auth.guard';

export const routes: Routes = [
  {
    path: '',
    pathMatch: 'full',
    redirectTo: 'stores',
  },
  {
    path: 'login',
    loadComponent: () => import('./pages/login.page').then((m) => m.LoginPage),
  },
  {
    path: 'signup',
    loadComponent: () => import('./pages/signup.page').then((m) => m.SignupPage),
  },
  {
    path: 'terms',
    loadComponent: () => import('./pages/policy.page').then((m) => m.PolicyPage),
    data: { kind: 'terms' },
  },
  {
    path: 'privacy',
    loadComponent: () => import('./pages/policy.page').then((m) => m.PolicyPage),
    data: { kind: 'privacy' },
  },
  {
    path: 'register',
    loadComponent: () => import('./pages/register.page').then((m) => m.RegisterPage),
    canActivate: [authGuard],
  },
  {
    path: 'stores',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/stores.page').then((m) => m.StoresPage),
  },
  {
    path: 'stores/:id/devices',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/devices.page').then((m) => m.DevicesPage),
  },
  {
    path: 'devices/:id',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/device-detail.page').then((m) => m.DeviceDetailPage),
  },
  {
    path: 'assets',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/assets.page').then((m) => m.AssetsPage),
  },
  {
    path: 'screens',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/screens.page').then((m) => m.ScreensPage),
  },
  {
    path: 'screens/:id',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/screen-edit.page').then((m) => m.ScreenEditPage),
  },
  {
    path: 'account',
    canActivate: [authGuard],
    loadComponent: () => import('./pages/account.page').then((m) => m.AccountPage),
  },
  { path: '**', redirectTo: 'stores' },
];
