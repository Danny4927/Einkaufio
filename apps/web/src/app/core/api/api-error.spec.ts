import { HttpErrorResponse } from '@angular/common/http';
import { ApiError, toApiError } from './api-error';

describe('toApiError', () => {
  it.each([
    [0, 'network'],
    [400, 'invalid'],
    [401, 'unauthorized'],
    [403, 'forbidden'],
    [404, 'not-found'],
    [409, 'conflict'],
    [413, 'too-large'],
    [429, 'rate-limited'],
    [503, 'unavailable'],
    [500, 'unavailable'],
    [418, 'unexpected'],
  ])('maps HTTP %i to %s', (status, kind) => {
    expect(toApiError(new HttpErrorResponse({ status })).kind).toBe(kind);
  });

  it('extracts the contract error code', () => {
    const error = toApiError(new HttpErrorResponse({ status: 409, error: { error: { code: 'version_conflict', message: 'x' } } }));
    expect(error.code).toBe('version_conflict');
  });

  it('keeps existing ApiErrors and maps unknown failures to unexpected', () => {
    const existing = new ApiError('conflict', 409);
    expect(toApiError(existing)).toBe(existing);
    expect(toApiError(new TypeError('boom')).kind).toBe('unexpected');
  });
});
