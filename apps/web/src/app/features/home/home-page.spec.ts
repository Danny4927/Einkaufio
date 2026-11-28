import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { AuthService } from '../../core/auth/auth.service';
import { HomePage } from './home-page';

describe('HomePage', () => {
  const authenticated = signal(false);
  const auth = { isAuthenticated: authenticated.asReadonly(), signIn: vi.fn() };
  let fixture: ComponentFixture<HomePage>;

  async function render(inputs: Record<string, string> = {}) {
    TestBed.configureTestingModule({ imports: [HomePage], providers: [provideRouter([]), { provide: AuthService, useValue: auth }] });
    fixture = TestBed.createComponent(HomePage);
    for (const [name, value] of Object.entries(inputs)) {
      fixture.componentRef.setInput(name, value);
    }
    await fixture.whenStable();
  }

  const text = () => (fixture.nativeElement as HTMLElement).textContent ?? '';

  beforeEach(() => {
    authenticated.set(false);
    auth.signIn.mockClear();
  });

  it('explains the product and offers sign-in when signed out', async () => {
    await render();
    for (const highlight of ['Listen gemeinsam bearbeiten', 'Änderungen synchron halten', 'Mit Familie und Freunden teilen']) {
      expect(text()).toContain(highlight);
    }
    const button = (fixture.nativeElement as HTMLElement).querySelector<HTMLButtonElement>('.actions button');
    button?.click();
    expect(auth.signIn).toHaveBeenCalled();
  });

  it('links to the lists when signed in', async () => {
    authenticated.set(true);
    await render({ anmeldung: 'erforderlich' });
    expect((fixture.nativeElement as HTMLElement).querySelector('a[href="/listen"]')).not.toBeNull();
    expect(text()).not.toContain('Bitte melde dich an');
  });

  it.each([
    [{ anmeldung: 'erforderlich' }, 'Bitte melde dich an, um deine Listen zu sehen.'],
    [{ sitzung: 'abgelaufen' }, 'Deine Sitzung ist abgelaufen.'],
  ])('shows a sign-in hint for %o', async (inputs, hint) => {
    await render(inputs);
    expect(text()).toContain(hint);
  });
});
