import { DOCUMENT } from '@angular/common';
import { computed, DestroyRef, inject, Injectable, InjectionToken, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Router } from '@angular/router';
import { OidcSecurityService } from 'angular-auth-oidc-client';
import { firstValueFrom, Observable } from 'rxjs';
import { RUNTIME_CONFIG } from '../config/runtime-config';
import { DEFAULT_SIGNED_IN_URL, isSafeReturnUrl } from './return-url';

/** `failed`: the IdP callback could not be completed (invalid state, token error, IdP down). */
export type AuthStatus = 'pending' | 'signed-in' | 'signed-out' | 'failed';

const RETURN_URL_KEY = 'einkaufio.returnUrl';

/** Full-page navigation to another origin (the Cognito hosted UI); replaceable in tests. */
export const EXTERNAL_REDIRECT = new InjectionToken<(url: string) => void>('EXTERNAL_REDIRECT', {
  providedIn: 'root',
  factory: () => {
    const document = inject(DOCUMENT);
    return (url: string) => document.location.assign(url);
  },
});

/**
 * The only entry point features use for authentication. Wraps the OIDC library so the rest of
 * the app never touches tokens, except the HTTP interceptor via {@link accessToken}.
 */
@Injectable({ providedIn: 'root' })
export class AuthService {
  private readonly oidc = inject(OidcSecurityService);
  private readonly config = inject(RUNTIME_CONFIG);
  private readonly router = inject(Router);
  private readonly document = inject(DOCUMENT);
  private readonly destroyRef = inject(DestroyRef);
  private readonly redirect = inject(EXTERNAL_REDIRECT);
  private readonly statusState = signal<AuthStatus>('pending');
  private ready: Promise<boolean> | null = null;

  readonly status = this.statusState.asReadonly();
  readonly isAuthenticated = computed(() => this.statusState() === 'signed-in');

  /** Restores a stored session or completes an authorization-code callback. Idempotent. */
  initialize(): Promise<boolean> {
    this.ready ??= firstValueFrom(this.oidc.checkAuth()).then(
      (result) => {
        this.statusState.set(result.isAuthenticated ? 'signed-in' : result.errorMessage ? 'failed' : 'signed-out');
        this.followLibraryState();
        return result.isAuthenticated;
      },
      () => {
        this.statusState.set('failed');
        return false;
      },
    );
    return this.ready;
  }

  /**
   * Starts the hosted-UI redirect; `returnUrl` is restored after the callback if it is safe.
   * Waits for {@link initialize} so a click during the initial session check cannot interleave
   * with it (both write the library's flow state).
   */
  async signIn(returnUrl?: string): Promise<void> {
    if (returnUrl !== undefined) {
      this.rememberReturnUrl(returnUrl);
    }
    await this.initialize();
    this.oidc.authorize();
  }

  /** Clears local tokens and, when configured, ends the Cognito hosted-UI session too. */
  async signOut(): Promise<void> {
    this.oidc.logoffLocal();
    this.storage()?.removeItem(RETURN_URL_KEY);
    this.statusState.set('signed-out');
    if (this.config.logoutUrl === null) {
      await this.router.navigateByUrl('/');
      return;
    }
    const origin = this.document.location.origin;
    const params = new URLSearchParams({ client_id: this.config.clientId, logout_uri: `${origin}/` });
    this.redirect(`${this.config.logoutUrl}?${params.toString()}`);
  }

  /** Called when the API rejects the token: drop the local session and ask to sign in again. */
  expireSession(): void {
    if (this.statusState() !== 'signed-in') {
      return;
    }
    this.oidc.logoffLocal();
    this.statusState.set('signed-out');
    void this.router.navigate(['/'], { queryParams: { sitzung: 'abgelaufen' } });
  }

  /** Current access token, or an empty string when signed out. Only the interceptor uses this. */
  accessToken(): Observable<string> {
    return this.oidc.getAccessToken();
  }

  rememberReturnUrl(url: string): void {
    if (isSafeReturnUrl(url)) {
      this.storage()?.setItem(RETURN_URL_KEY, url);
    }
  }

  /** Returns and forgets the stored return URL, falling back to the list overview. */
  consumeReturnUrl(): string {
    const storage = this.storage();
    const stored = storage?.getItem(RETURN_URL_KEY) ?? null;
    storage?.removeItem(RETURN_URL_KEY);
    return isSafeReturnUrl(stored) ? stored : DEFAULT_SIGNED_IN_URL;
  }

  private followLibraryState(): void {
    // Picks up silent-renew failures and sign-outs in the library after initialization.
    this.oidc.isAuthenticated$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(({ isAuthenticated }) => {
      if (isAuthenticated) {
        this.statusState.set('signed-in');
      } else if (this.statusState() === 'signed-in') {
        this.statusState.set('signed-out');
      }
    });
  }

  private storage(): Storage | null {
    try {
      return this.document.defaultView?.sessionStorage ?? null;
    } catch {
      return null;
    }
  }
}
