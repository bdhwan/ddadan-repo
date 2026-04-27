import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { AuthService } from './auth.service';

export const authGuard: CanActivateFn = async () => {
  const auth = inject(AuthService);
  const router = inject(Router);

  // Wait for the initial auth state to settle.
  if (!auth.ready()) {
    await new Promise<void>((resolve) => {
      const id = setInterval(() => {
        if (auth.ready()) {
          clearInterval(id);
          resolve();
        }
      }, 30);
    });
  }

  if (auth.isLoggedIn()) return true;
  return router.parseUrl('/login');
};
