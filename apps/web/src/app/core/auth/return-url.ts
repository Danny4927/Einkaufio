export const DEFAULT_SIGNED_IN_URL = '/listen';
export const AUTH_CALLBACK_PATH = '/auth/callback';

/**
 * Accepts only same-origin, app-internal absolute paths so a stored return URL can never
 * become an open redirect (`//evil.example`, `/\evil.example`, `https://…`, `javascript:`).
 */
export function isSafeReturnUrl(value: unknown): value is string {
  if (typeof value !== 'string' || value.length === 0 || value.length > 512) {
    return false;
  }
  if (!value.startsWith('/') || value.startsWith('//') || value.includes('\\')) {
    return false;
  }
  // Control characters could smuggle a different target past the prefix checks.
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x20 || code === 0x7f) {
      return false;
    }
  }
  return !value.startsWith(AUTH_CALLBACK_PATH);
}
