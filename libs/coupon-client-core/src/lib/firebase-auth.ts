import { inject, Injectable, InjectionToken, makeEnvironmentProviders } from '@angular/core';
import { FirebaseApp, FirebaseOptions, initializeApp } from 'firebase/app';
import { Auth, getAuth, User } from 'firebase/auth';

export const COUPON_FIREBASE_OPTIONS = new InjectionToken<FirebaseOptions>('COUPON_FIREBASE_OPTIONS');

export function provideCouponClientCore(firebaseOptions: FirebaseOptions) {
  return makeEnvironmentProviders([
    { provide: COUPON_FIREBASE_OPTIONS, useValue: firebaseOptions },
    AuthSessionService,
  ]);
}

@Injectable()
export class AuthSessionService {
  private readonly options = inject(COUPON_FIREBASE_OPTIONS);
  private readonly app: FirebaseApp = initializeApp(this.options);
  private readonly auth: Auth = getAuth(this.app);

  get currentUser(): User | null {
    return this.auth.currentUser;
  }

  async getIdToken(forceRefresh = false): Promise<string | null> {
    return this.auth.currentUser?.getIdToken(forceRefresh) ?? null;
  }
}
