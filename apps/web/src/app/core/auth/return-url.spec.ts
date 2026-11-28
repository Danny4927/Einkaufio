import { isSafeReturnUrl } from './return-url';

describe('isSafeReturnUrl', () => {
  it.each(['/listen', '/listen/abc?x=1', '/'])('accepts the internal path %s', (url) => {
    expect(isSafeReturnUrl(url)).toBe(true);
  });

  it.each([
    '',
    'listen',
    '//evil.example/listen',
    '/\\evil.example',
    'https://evil.example/',
    'javascript:alert(1)',
    '/listen\n//evil.example',
    '/auth/callback?code=x',
    `/${'a'.repeat(600)}`,
    42,
    null,
  ])('rejects %s', (url) => {
    expect(isSafeReturnUrl(url)).toBe(false);
  });
});
