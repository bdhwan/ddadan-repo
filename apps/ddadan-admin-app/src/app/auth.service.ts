import { Injectable, signal } from '@angular/core';

/** Local-only admin: no Firebase or bearer tokens. */
@Injectable({ providedIn: 'root' })
export class AuthService {
  readonly currentUser = signal<null>(null);
  readonly ready = signal(true);

  isLoggedIn(): boolean {
    return true;
  }

  async getIdToken(): Promise<string | null> {
    return null;
  }

  async signOut(): Promise<void> {}

  async signInWithEmail(_email: string, _password: string): Promise<void> {}

  async signUpWithEmail(_email: string, _password: string): Promise<void> {}

  async signInWithGoogle(): Promise<void> {}
}
