import { DOCUMENT } from '@angular/common';
import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { OidcSecurityService } from 'angular-auth-oidc-client';
import { Observable, throwError } from 'rxjs';
import { RUNTIME_CONFIG, RuntimeConfig } from '../config/runtime-config';
import { TEST_RUNTIME_CONFIG } from '../testing/test-config';
import { AuthService, EXTERNAL_REDIRECT } from './auth.service';
import { FakeOidcSecurityService } from '../testing/fake-oidc';

const redirect = vi.fn<(url: string) => void>();

function setup(oidc: FakeOidcSecurityService, config: RuntimeConfig = TEST_RUNTIME_CONFIG) {
  redirect.mockClear();
  TestBed.configureTestingModule({
    providers: [
      { provide: EXTERNAL_REDIRECT, useValue: redirect },
      provideRouter([]),
      { provide: OidcSecurityService, useValue: oidc },
      { provide: RUNTIME_CONFIG, useValue: config },
    ],
  });
  const router = TestBed.inject(Router);
  vi.spyOn(router, 'navigate').mockResolvedValue(true);
  vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true);
  return { auth: TestBed.inject(AuthService), router };
}

describe('AuthService', () => {
  afterEach(() => sessionStorage.clear());

  it('restores a session once, even when initialized repeatedly', async () => {
    const oidc = new FakeOidcSecurityService().signedIn();
    const { auth } = setup(oidc);
    await expect(auth.initialize()).resolves.toBe(true);
    await auth.initialize();
    expect(oidc.checkAuth).toHaveBeenCalledTimes(1);
    expect(auth.status()).toBe('signed-in');
    expect(auth.isAuthenticated()).toBe(true);
  });

  it('reports signed-out without a session and failed on callback errors', async () => {
    const { auth } = setup(new FakeOidcSecurityService());
    await expect(auth.initialize()).resolves.toBe(false);
    expect(auth.status()).toBe('signed-out');

    TestBed.resetTestingModule();
    const failing = new FakeOidcSecurityService();
    failing.loginResponse = throwError(() => new Error('state mismatch'));
    const second = setup(failing);
    await expect(second.auth.initialize()).resolves.toBe(false);
    expect(second.auth.status()).toBe('failed');
  });

  it('follows library sign-outs such as failed silent renewals', async () => {
    const oidc = new FakeOidcSecurityService().signedIn();
    const { auth } = setup(oidc);
    await auth.initialize();
    oidc.isAuthenticated$.next({ isAuthenticated: false, allConfigsAuthenticated: [] });
    expect(auth.status()).toBe('signed-out');
  });

  it('remembers only safe return urls and consumes them once', async () => {
    const { auth } = setup(new FakeOidcSecurityService());
    await auth.signIn('//evil.example');
    expect(auth.consumeReturnUrl()).toBe('/listen');
    await auth.signIn('/listen/abc');
    expect(auth.consumeReturnUrl()).toBe('/listen/abc');
    expect(auth.consumeReturnUrl()).toBe('/listen');
  });

  it('starts the authorization redirect only after the initial session check', async () => {
    const oidc = new FakeOidcSecurityService();
    let finishCheck!: () => void;
    oidc.loginResponse = new Observable((subscriber) => {
      finishCheck = () => {
        subscriber.next({ isAuthenticated: false, userData: null, accessToken: '', idToken: '' });
        subscriber.complete();
      };
    });
    const { auth } = setup(oidc);
    const signingIn = auth.signIn();
    await Promise.resolve();
    expect(oidc.authorize).not.toHaveBeenCalled();
    finishCheck();
    await signingIn;
    expect(oidc.authorize).toHaveBeenCalledTimes(1);
    expect(oidc.checkAuth).toHaveBeenCalledTimes(1);
  });

  it('signs out locally and returns home without a hosted-UI logout url', async () => {
    const oidc = new FakeOidcSecurityService().signedIn();
    const { auth, router } = setup(oidc);
    await auth.initialize();
    await auth.signOut();
    expect(oidc.logoffLocal).toHaveBeenCalled();
    expect(auth.status()).toBe('signed-out');
    expect(router.navigateByUrl).toHaveBeenCalledWith('/');
    expect(redirect).not.toHaveBeenCalled();
  });

  it('ends the hosted-UI session when a logout url is configured', async () => {
    const oidc = new FakeOidcSecurityService().signedIn();
    const { auth } = setup(oidc, { ...TEST_RUNTIME_CONFIG, logoutUrl: 'https://auth.example.test/logout' });
    const location = TestBed.inject(DOCUMENT).location;
    await auth.signOut();
    const target = new URL(redirect.mock.calls[0][0]);
    expect(target.origin + target.pathname).toBe('https://auth.example.test/logout');
    expect(target.searchParams.get('client_id')).toBe('test-client');
    expect(target.searchParams.get('logout_uri')).toBe(`${location.origin}/`);
  });

  it('expires an active session once and asks to sign in again', async () => {
    const oidc = new FakeOidcSecurityService().signedIn();
    const { auth, router } = setup(oidc);
    await auth.initialize();
    auth.expireSession();
    auth.expireSession();
    expect(oidc.logoffLocal).toHaveBeenCalledTimes(1);
    expect(router.navigate).toHaveBeenCalledWith(['/'], { queryParams: { sitzung: 'abgelaufen' } });
  });
});
