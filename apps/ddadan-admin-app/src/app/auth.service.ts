import { Injectable, signal } from '@angular/core';
import { initializeApp } from 'firebase/app';
import {
  GoogleAuthProvider,
  createUserWithEmailAndPassword,
  getAuth,
  onAuthStateChanged,
  signInWithEmailAndPassword,
  signInWithPopup,
  signOut,
  type Auth,
  type User,
} from 'firebase/auth';
import { environment } from '../environment';

@Injectable({ providedIn: 'root' })
export class AuthService {
  private readonly auth: Auth;
  readonly currentUser = signal<User | null>(null);
  readonly ready = signal(false);

  constructor() {
    const app = initializeApp(environment.firebase);
    this.auth = getAuth(app);
    onAuthStateChanged(this.auth, (user) => {
      this.currentUser.set(user);
      this.ready.set(true);
    });
  }

  async signInWithEmail(email: string, password: string): Promise<void> {
    await signInWithEmailAndPassword(this.auth, email, password);
  }

  async signUpWithEmail(email: string, password: string): Promise<void> {
    await createUserWithEmailAndPassword(this.auth, email, password);
  }

  async signInWithGoogle(): Promise<void> {
    const provider = new GoogleAuthProvider();
    await signInWithPopup(this.auth, provider);
  }

  async signOut(): Promise<void> {
    await signOut(this.auth);
  }

  async getIdToken(): Promise<string | null> {
    const u = this.auth.currentUser;
    if (!u) return null;
    return u.getIdToken();
  }

  isLoggedIn(): boolean {
    return !!this.auth.currentUser;
  }
}
