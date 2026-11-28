import { HttpClient, provideHttpClient, withInterceptors } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { firstValueFrom, of } from 'rxjs';
import { RUNTIME_CONFIG } from '../config/runtime-config';
import { TEST_RUNTIME_CONFIG } from '../testing/test-config';
import { authInterceptor, isApiRequest } from './auth.interceptor';
import { AuthService } from './auth.service';

describe('isApiRequest', () => {
  it('matches only application routes of the configured API origin', () => {
    const base = 'https://api.example.test';
    expect(isApiRequest('https://api.example.test/api/lists', base)).toBe(true);
    expect(isApiRequest('https://api.example.test/health', base)).toBe(false);
    expect(isApiRequest('https://api.example.test.evil.test/api/lists', base)).toBe(false);
    expect(isApiRequest('https://idp.example.test/oauth2/token', base)).toBe(false);
    expect(isApiRequest('/config.json', base)).toBe(false);
  });
});

describe('authInterceptor', () => {
  let token: string;
  const auth = { accessToken: vi.fn(() => of(token)), expireSession: vi.fn() };

  beforeEach(() => {
    token = 'secret-access-token';
    auth.expireSession.mockClear();
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([authInterceptor])),
        provideHttpClientTesting(),
        { provide: RUNTIME_CONFIG, useValue: TEST_RUNTIME_CONFIG },
        { provide: AuthService, useValue: auth },
      ],
    });
  });

  afterEach(() => TestBed.inject(HttpTestingController).verify());

  it('adds the bearer token to API requests', async () => {
    const response = firstValueFrom(TestBed.inject(HttpClient).get('https://api.example.test/api/lists'));
    const request = TestBed.inject(HttpTestingController).expectOne('https://api.example.test/api/lists');
    expect(request.request.headers.get('Authorization')).toBe('Bearer secret-access-token');
    request.flush({ items: [] });
    await expect(response).resolves.toEqual({ items: [] });
  });

  it('never sends the token to other origins', () => {
    const http = TestBed.inject(HttpClient);
    http.get('https://idp.example.test/.well-known/openid-configuration').subscribe();
    http.get('https://api.example.test.evil.test/api/lists').subscribe();
    const controller = TestBed.inject(HttpTestingController);
    for (const url of ['https://idp.example.test/.well-known/openid-configuration', 'https://api.example.test.evil.test/api/lists']) {
      const request = controller.expectOne(url);
      expect(request.request.headers.has('Authorization')).toBe(false);
      request.flush({});
    }
  });

  it('fails closed without a token and does not send the request', async () => {
    token = '';
    const response = firstValueFrom(TestBed.inject(HttpClient).get('https://api.example.test/api/lists'));
    await expect(response).rejects.toMatchObject({ status: 401 });
    TestBed.inject(HttpTestingController).expectNone('https://api.example.test/api/lists');
    expect(auth.expireSession).toHaveBeenCalled();
  });

  it('expires the session when the API answers 401', async () => {
    const response = firstValueFrom(TestBed.inject(HttpClient).get('https://api.example.test/api/lists'));
    TestBed.inject(HttpTestingController)
      .expectOne('https://api.example.test/api/lists')
      .flush({ error: { code: 'unauthorized', message: 'x' } }, { status: 401, statusText: 'Unauthorized' });
    await expect(response).rejects.toMatchObject({ status: 401 });
    expect(auth.expireSession).toHaveBeenCalledTimes(1);
  });

  it('passes other API errors through without ending the session', async () => {
    const response = firstValueFrom(TestBed.inject(HttpClient).get('https://api.example.test/api/lists'));
    TestBed.inject(HttpTestingController)
      .expectOne('https://api.example.test/api/lists')
      .flush(null, { status: 409, statusText: 'Conflict' });
    await expect(response).rejects.toMatchObject({ status: 409 });
    expect(auth.expireSession).not.toHaveBeenCalled();
  });
});
