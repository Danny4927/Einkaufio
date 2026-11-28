import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { inject } from '@angular/core';
import { catchError, switchMap, take, throwError } from 'rxjs';
import { RUNTIME_CONFIG } from '../config/runtime-config';
import { AuthService } from './auth.service';

/**
 * True only for application routes of the configured API. The trailing slash in the prefix
 * prevents host-suffix tricks such as `https://api.example.com.evil.test/api/...`.
 */
export function isApiRequest(url: string, apiBaseUrl: string): boolean {
  return url.startsWith(`${apiBaseUrl}/api/`);
}

/**
 * Attaches the Cognito access token to API requests only; config, IdP and any other origin
 * never see it. A missing token or an API 401 ends the local session (fail closed).
 */
export const authInterceptor: HttpInterceptorFn = (request, next) => {
  const { apiBaseUrl } = inject(RUNTIME_CONFIG);
  if (!isApiRequest(request.url, apiBaseUrl)) {
    return next(request);
  }
  const auth = inject(AuthService);
  return auth.accessToken().pipe(
    take(1),
    switchMap((token) => {
      if (!token) {
        auth.expireSession();
        return throwError(() => new HttpErrorResponse({ status: 401, statusText: 'Unauthorized', url: request.url }));
      }
      return next(request.clone({ setHeaders: { Authorization: `Bearer ${token}` } }));
    }),
    catchError((error: unknown) => {
      if (error instanceof HttpErrorResponse && error.status === 401) {
        auth.expireSession();
      }
      return throwError(() => error);
    }),
  );
};
