import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { App } from './app';
import { AuthService, AuthStatus } from './core/auth/auth.service';

describe('App shell', () => {
  const status = signal<AuthStatus>('signed-out');
  const auth = {
    status: status.asReadonly(),
    isAuthenticated: () => status() === 'signed-in',
    initialize: vi.fn(async () => false),
    signIn: vi.fn(),
    signOut: vi.fn(async () => undefined),
  };
  let fixture: ComponentFixture<App>;

  async function render(state: AuthStatus) {
    status.set(state);
    TestBed.configureTestingModule({ imports: [App], providers: [provideRouter([]), { provide: AuthService, useValue: auth }] });
    fixture = TestBed.createComponent(App);
    await fixture.whenStable();
  }

  const buttons = () => [...(fixture.nativeElement as HTMLElement).querySelectorAll<HTMLButtonElement>('nav button')];

  it('initializes authentication on start', async () => {
    await render('pending');
    expect(auth.initialize).toHaveBeenCalled();
    expect(buttons()).toHaveLength(0);
  });

  it('offers sign-in when signed out', async () => {
    await render('signed-out');
    buttons()[0].click();
    expect(auth.signIn).toHaveBeenCalled();
  });

  it('offers the lists and sign-out when signed in', async () => {
    await render('signed-in');
    expect((fixture.nativeElement as HTMLElement).querySelector('nav a[href="/listen"].ghost')).not.toBeNull();
    buttons()[0].click();
    expect(auth.signOut).toHaveBeenCalled();
  });
});
