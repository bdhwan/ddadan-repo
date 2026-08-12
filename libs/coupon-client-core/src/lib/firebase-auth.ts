import {
  inject,
  Injectable,
  InjectionToken,
  makeEnvironmentProviders,
} from "@angular/core";
import { FirebaseApp, FirebaseOptions, initializeApp } from "firebase/app";
import {
  Auth,
  connectAuthEmulator,
  createUserWithEmailAndPassword,
  EmailAuthProvider,
  getAuth,
  reauthenticateWithCredential,
  sendPasswordResetEmail,
  signInWithEmailAndPassword,
  signOut,
  User,
} from "firebase/auth";

export const COUPON_FIREBASE_OPTIONS = new InjectionToken<FirebaseOptions>(
  "COUPON_FIREBASE_OPTIONS",
);

export interface CouponAuthEmulatorOptions {
  readonly enabled: boolean;
  /**
   * Route emulator traffic through the Angular dev server. This keeps an HTTPS
   * LAN page from making mixed-content requests to the emulator's HTTP port.
   */
  readonly useSameOrigin: boolean;
  /** Used only when useSameOrigin is false (for example, a non-browser test). */
  readonly url?: string;
}

export interface CouponClientRuntimeOptions {
  readonly production: boolean;
  readonly authEmulator?: CouponAuthEmulatorOptions;
}

export const COUPON_CLIENT_RUNTIME_OPTIONS =
  new InjectionToken<CouponClientRuntimeOptions>(
    "COUPON_CLIENT_RUNTIME_OPTIONS",
  );

export function provideCouponClientCore(
  firebaseOptions: FirebaseOptions,
  runtimeOptions: CouponClientRuntimeOptions = { production: true },
) {
  return makeEnvironmentProviders([
    { provide: COUPON_FIREBASE_OPTIONS, useValue: firebaseOptions },
    { provide: COUPON_CLIENT_RUNTIME_OPTIONS, useValue: runtimeOptions },
    AuthSessionService,
  ]);
}

export function resolveAuthEmulatorUrl(
  runtimeOptions: CouponClientRuntimeOptions,
  browserOrigin?: string,
): string | null {
  const emulator = runtimeOptions.authEmulator;
  if (!emulator?.enabled) {
    return null;
  }
  if (runtimeOptions.production) {
    throw new Error("AUTH_EMULATOR_FORBIDDEN_IN_PRODUCTION");
  }

  const configuredUrl = emulator.useSameOrigin ? browserOrigin : emulator.url;
  if (!configuredUrl) {
    throw new Error("AUTH_EMULATOR_URL_UNAVAILABLE");
  }

  const parsed = new URL(configuredUrl);
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("AUTH_EMULATOR_URL_INVALID");
  }
  return parsed.origin;
}

@Injectable()
export class AuthSessionService {
  private readonly options = inject(COUPON_FIREBASE_OPTIONS);
  private readonly runtimeOptions = inject(COUPON_CLIENT_RUNTIME_OPTIONS);
  private readonly app: FirebaseApp = initializeApp(this.options);
  private readonly auth: Auth = getAuth(this.app);

  constructor() {
    const browserOrigin =
      typeof location === "undefined" ? undefined : location.origin;
    const emulatorUrl = resolveAuthEmulatorUrl(
      this.runtimeOptions,
      browserOrigin,
    );
    if (emulatorUrl) {
      connectAuthEmulator(this.auth, emulatorUrl);
    }
  }

  get currentUser(): User | null {
    return this.auth.currentUser;
  }

  async getIdToken(forceRefresh = false): Promise<string | null> {
    await this.auth.authStateReady();
    return this.auth.currentUser?.getIdToken(forceRefresh) ?? null;
  }

  async signInWithEmail(email: string, password: string): Promise<User> {
    const credential = await signInWithEmailAndPassword(
      this.auth,
      email.trim(),
      password,
    );
    await credential.user.getIdToken();
    return credential.user;
  }

  async createAccountWithEmail(email: string, password: string): Promise<User> {
    const credential = await createUserWithEmailAndPassword(
      this.auth,
      email.trim(),
      password,
    );
    await credential.user.getIdToken();
    return credential.user;
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
