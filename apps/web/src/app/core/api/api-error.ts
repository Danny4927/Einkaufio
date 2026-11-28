import { HttpErrorResponse } from '@angular/common/http';

export type ApiErrorKind =
  | 'unauthorized'
  | 'forbidden'
  | 'not-found'
  | 'conflict'
  | 'invalid'
  | 'too-large'
  | 'rate-limited'
  | 'unavailable'
  | 'network'
  | 'unexpected';

/** Transport failure mapped to a closed set of kinds; never carries headers or tokens. */
export class ApiError extends Error {
  constructor(
    readonly kind: ApiErrorKind,
    readonly status: number,
    readonly code: string | null = null,
  ) {
    super(`API-Fehler: ${kind} (HTTP ${status})`);
    this.name = 'ApiError';
  }
}

export function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) {
    return error;
  }
  if (!(error instanceof HttpErrorResponse)) {
    return new ApiError('unexpected', 0);
  }
  return new ApiError(kindForStatus(error.status), error.status, errorCode(error.error));
}

function kindForStatus(status: number): ApiErrorKind {
  switch (status) {
    case 0:
      return 'network';
    case 400:
      return 'invalid';
    case 401:
      return 'unauthorized';
    case 403:
      return 'forbidden';
    case 404:
      return 'not-found';
    case 409:
      return 'conflict';
    case 413:
      return 'too-large';
    case 429:
      return 'rate-limited';
    case 503:
      return 'unavailable';
    default:
      return status >= 500 ? 'unavailable' : 'unexpected';
  }
}

function errorCode(body: unknown): string | null {
  if (typeof body !== 'object' || body === null) {
    return null;
  }
  const error = (body as { error?: unknown }).error;
  if (typeof error !== 'object' || error === null) {
    return null;
  }
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : null;
}
