import { BehaviorSubject, Observable, of } from 'rxjs';
import { LoginResponse } from 'angular-auth-oidc-client';

/** Minimal stand-in for OidcSecurityService covering what AuthService uses. */
export class FakeOidcSecurityService {
  readonly isAuthenticated$ = new BehaviorSubject({ isAuthenticated: false, allConfigsAuthenticated: [] as never[] });
  loginResponse: Observable<LoginResponse> = of({ isAuthenticated: false, userData: null, accessToken: '', idToken: '' });
  token = '';
  readonly checkAuth = vi.fn(() => this.loginResponse);
  readonly authorize = vi.fn();
  readonly logoffLocal = vi.fn();
  readonly getAccessToken = vi.fn(() => of(this.token));

  signedIn(): this {
    this.loginResponse = of({ isAuthenticated: true, userData: null, accessToken: 'access', idToken: 'id' });
    this.isAuthenticated$.next({ isAuthenticated: true, allConfigsAuthenticated: [] });
    this.token = 'access';
    return this;
  }
}
