import { inject, Injectable, signal } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import type { StoreReviewStatus, UserRole } from '@coupon/contracts';

@Injectable({ providedIn: 'root' })
export class CouponAccessState {
  readonly requiredTermsAccepted = signal(false);
  readonly roles = signal<ReadonlySet<UserRole>>(new Set());
  readonly storeStatus = signal<StoreReviewStatus | null>(null);

  setTermsAccepted(accepted: boolean): void {
    this.requiredTermsAccepted.set(accepted);
  }

  setRoles(roles: readonly UserRole[]): void {
    this.roles.set(new Set(roles));
  }

  setStoreStatus(status: StoreReviewStatus | null): void {
    this.storeStatus.set(status);
  }
}

export const termsConsentGuard: CanActivateFn = () => {
  const access = inject(CouponAccessState);
  return access.requiredTermsAccepted() || inject(Router).createUrlTree(['/terms']);
};

export const roleGuard: CanActivateFn = (route) => {
  const requiredRole = route.data['role'] as UserRole | undefined;
  if (!requiredRole || inject(CouponAccessState).roles().has(requiredRole)) {
    return true;
  }
  return inject(Router).createUrlTree(['/login'], { queryParams: { reason: 'role' } });
};

export const storeStatusGuard: CanActivateFn = (route) => {
  const allowed = (route.data['storeStatuses'] as readonly StoreReviewStatus[] | undefined) ?? ['APPROVED'];
  const status = inject(CouponAccessState).storeStatus();
  if (status && allowed.includes(status)) {
    return true;
  }
  return inject(Router).createUrlTree(['/onboarding/store'], { queryParams: { status: status ?? 'missing' } });
};
