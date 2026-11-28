import { loadRuntimeConfig, parseRuntimeConfig, RuntimeConfigError } from './runtime-config';

const valid = {
  issuer: 'https://cognito-idp.eu-central-1.amazonaws.com/eu-central-1_abc/',
  clientId: 'abc123client',
  apiBaseUrl: 'https://api.example.com/',
};

describe('parseRuntimeConfig', () => {
  it('normalizes a valid configuration and defaults scope and logout', () => {
    expect(parseRuntimeConfig(valid)).toEqual({
      issuer: 'https://cognito-idp.eu-central-1.amazonaws.com/eu-central-1_abc',
      clientId: 'abc123client',
      apiBaseUrl: 'https://api.example.com',
      scope: 'openid',
      logoutUrl: null,
    });
  });

  it('accepts plain HTTP only on loopback hosts', () => {
    expect(parseRuntimeConfig({ ...valid, apiBaseUrl: 'http://127.0.0.1:3000' }).apiBaseUrl).toBe('http://127.0.0.1:3000');
    expect(() => parseRuntimeConfig({ ...valid, apiBaseUrl: 'http://api.example.com' })).toThrow(RuntimeConfigError);
  });

  it.each([
    ['not an object', 'x'],
    ['an array', []],
    ['missing issuer', { ...valid, issuer: undefined }],
    ['empty client id', { ...valid, clientId: ' ' }],
    ['client id with separators', { ...valid, clientId: 'a/b' }],
    ['a non-string api base', { ...valid, apiBaseUrl: 42 }],
    ['a javascript url', { ...valid, issuer: 'javascript:alert(1)' }],
    ['credentials in the url', { ...valid, apiBaseUrl: 'https://user:pw@api.example.com' }],
    ['a query string', { ...valid, issuer: 'https://idp.example.com/?x=1' }],
    ['a scope without openid', { ...valid, scope: 'email profile' }],
    ['an insecure logout url', { ...valid, logoutUrl: 'http://auth.example.com/logout' }],
  ])('rejects %s', (_label, raw) => {
    expect(() => parseRuntimeConfig(raw)).toThrow(RuntimeConfigError);
  });

  it('keeps an explicit scope and logout url', () => {
    const config = parseRuntimeConfig({ ...valid, scope: 'openid email', logoutUrl: 'https://auth.example.com/logout' });
    expect(config.scope).toBe('openid email');
    expect(config.logoutUrl).toBe('https://auth.example.com/logout');
  });
});

describe('loadRuntimeConfig', () => {
  it('fetches without cache and validates the payload', async () => {
    const fetchFn = vi.fn(async () => new Response(JSON.stringify(valid), { status: 200 }));
    const config = await loadRuntimeConfig(fetchFn as unknown as typeof fetch, '/config.json');
    expect(config.clientId).toBe('abc123client');
    expect(fetchFn).toHaveBeenCalledWith('/config.json', { cache: 'no-store', credentials: 'same-origin' });
  });

  it('fails closed when the file is missing', async () => {
    const fetchFn = vi.fn(async () => new Response('', { status: 404 }));
    await expect(loadRuntimeConfig(fetchFn as unknown as typeof fetch)).rejects.toThrow(RuntimeConfigError);
  });
});
