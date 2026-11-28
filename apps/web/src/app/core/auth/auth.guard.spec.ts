import { TestBed } from '@angular/core/testing';
import { ActivatedRouteSnapshot, provideRouter, RouterStateSnapshot, UrlTree } from '@angular/router';
import { authGuard } from './auth.guard';
import { AuthService } from './auth.service';

describe('authGuard', () => {
  function run(authenticated: boolean) {
    const auth = {
      initialize: vi.fn(async () => authenticated),
      isAuthenticated: () => authenticated,
      rememberReturnUrl: vi.fn(),
    };
    TestBed.configureTestingModule({ providers: [provideRouter([]), { provide: AuthService, useValue: auth }] });
    const state = { url: '/listen/abc' } as RouterStateSnapshot;
    const result = TestBed.runInInjectionContext(() => authGuard({} as ActivatedRouteSnapshot, state));
    return { auth, result: result as Promise<boolean | UrlTree> };
  }

  it('allows signed-in users', async () => {
    const { result, auth } = run(true);
    await expect(result).resolves.toBe(true);
    expect(auth.rememberReturnUrl).not.toHaveBeenCalled();
  });

  it('redirects signed-out users home and remembers the target', async () => {
    const { result, auth } = run(false);
    const tree = (await result) as UrlTree;
    expect(tree.toString()).toBe('/?anmeldung=erforderlich');
    expect(auth.rememberReturnUrl).toHaveBeenCalledWith('/listen/abc');
  });
});
