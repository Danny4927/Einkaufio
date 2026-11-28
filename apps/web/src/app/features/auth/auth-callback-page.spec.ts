import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { AuthService } from '../../core/auth/auth.service';
import { AuthCallbackPage } from './auth-callback-page';

describe('AuthCallbackPage', () => {
  async function render(signedIn: boolean) {
    const auth = { initialize: vi.fn(async () => signedIn), consumeReturnUrl: vi.fn(() => '/listen/abc'), signIn: vi.fn() };
    TestBed.configureTestingModule({ imports: [AuthCallbackPage], providers: [provideRouter([]), { provide: AuthService, useValue: auth }] });
    const router = TestBed.inject(Router);
    const navigate = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true);
    const fixture = TestBed.createComponent(AuthCallbackPage);
    await fixture.whenStable();
    return { auth, navigate, element: fixture.nativeElement as HTMLElement };
  }

  it('returns to the remembered page after a successful callback', async () => {
    const { navigate } = await render(true);
    expect(navigate).toHaveBeenCalledWith('/listen/abc', { replaceUrl: true });
  });

  it('offers a new sign-in when the callback fails', async () => {
    const { auth, navigate, element } = await render(false);
    expect(navigate).not.toHaveBeenCalled();
    expect(element.textContent).toContain('Anmeldung fehlgeschlagen');
    element.querySelector('button')?.click();
    expect(auth.signIn).toHaveBeenCalled();
  });
});
