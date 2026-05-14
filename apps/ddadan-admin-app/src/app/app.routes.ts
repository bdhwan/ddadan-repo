import { Routes } from '@angular/router';

export const routes: Routes = [
  {
    path: '',
    pathMatch: 'full',
    redirectTo: 'devices',
  },
  {
    path: 'login',
    pathMatch: 'full',
    redirectTo: 'devices',
  },
  {
    path: 'signup',
    pathMatch: 'full',
    redirectTo: 'devices',
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
  },
  {
    path: 'stores',
    pathMatch: 'full',
    redirectTo: 'devices',
  },
  {
    path: 'stores/:id/devices',
    pathMatch: 'full',
    redirectTo: 'devices',
  },
  {
    path: 'devices',
    loadComponent: () => import('./pages/devices.page').then((m) => m.DevicesPage),
  },
  {
    path: 'devices/:id',
    loadComponent: () => import('./pages/device-detail.page').then((m) => m.DeviceDetailPage),
  },
  {
    path: 'assets',
    loadComponent: () => import('./pages/assets.page').then((m) => m.AssetsPage),
  },
  {
    path: 'screens',
    loadComponent: () => import('./pages/screens.page').then((m) => m.ScreensPage),
  },
  {
    path: 'screens/:id',
    loadComponent: () => import('./pages/screen-edit.page').then((m) => m.ScreenEditPage),
  },
  { path: '**', redirectTo: 'devices' },
];
