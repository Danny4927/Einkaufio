import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { AuthService } from './auth.service';

/** Protects signed-in routes; remembers the target so sign-in can return to it. */
export const authGuard: CanActivateFn = async (_route, state) => {
  const auth = inject(AuthService);
  const router = inject(Router);
  await auth.initialize();
  if (auth.isAuthenticated()) {
    return true;
  }
  auth.rememberReturnUrl(state.url);
  return router.createUrlTree(['/'], { queryParams: { anmeldung: 'erforderlich' } });
};
