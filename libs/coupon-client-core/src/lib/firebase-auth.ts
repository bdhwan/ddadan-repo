import {
  inject,
  Injectable,
  InjectionToken,
  makeEnvironmentProviders,
} from "@angular/core";
import { FirebaseApp, FirebaseOptions, initializeApp } from "firebase/app";
import {
  Auth,
  EmailAuthProvider,
  getAuth,
  reauthenticateWithCredential,
  sendPasswordResetEmail,
  signOut,
  User,
} from "firebase/auth";

export const COUPON_FIREBASE_OPTIONS = new InjectionToken<FirebaseOptions>(
  "COUPON_FIREBASE_OPTIONS",
);

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

  async reauthenticateWithPassword(password: string): Promise<void> {
    const user = this.auth.currentUser;
    if (!user?.email) {
      throw new Error("EMAIL_REAUTHENTICATION_UNAVAILABLE");
    }
    await reauthenticateWithCredential(
      user,
      EmailAuthProvider.credential(user.email, password),
    );
    await user.getIdToken(true);
  }

  async sendPasswordReset(): Promise<void> {
    const email = this.auth.currentUser?.email;
    if (!email) {
      throw new Error("PASSWORD_RESET_UNAVAILABLE");
    }
    await sendPasswordResetEmail(this.auth, email);
  }

  async signOut(): Promise<void> {
    await signOut(this.auth);
  }
}
