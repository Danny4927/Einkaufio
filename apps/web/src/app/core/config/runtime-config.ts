import { InjectionToken } from '@angular/core';

/**
 * Deployment-specific settings loaded from `/config.json` before bootstrap.
 *
 * The file is public: it must only contain public-client identifiers, never secrets.
 */
export interface RuntimeConfig {
  /** Cognito user-pool issuer, e.g. https://cognito-idp.eu-central-1.amazonaws.com/eu-central-1_abc. */
  readonly issuer: string;
  /** Public Cognito app-client id (no client secret exists for the browser client). */
  readonly clientId: string;
  /** API origin (plus optional path prefix) without trailing slash. */
  readonly apiBaseUrl: string;
  /** OAuth scopes; always contains `openid`. */
  readonly scope: string;
  /** Optional Cognito hosted-UI `/logout` endpoint; null signs out locally only. */
  readonly logoutUrl: string | null;
}

export const RUNTIME_CONFIG = new InjectionToken<RuntimeConfig>('RUNTIME_CONFIG');

export const RUNTIME_CONFIG_URL = '/config.json';

export class RuntimeConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RuntimeConfigError';
  }
}

const CLIENT_ID_PATTERN = /^[A-Za-z0-9_-]{1,128}$/;
const SCOPE_PATTERN = /^[A-Za-z0-9_.:/-]+( [A-Za-z0-9_.:/-]+)*$/;
const LOOPBACK_HOSTS = new Set(['localhost', '127.0.0.1', '[::1]']);

/** Validates untrusted JSON and returns a normalized configuration or throws. */
export function parseRuntimeConfig(raw: unknown): RuntimeConfig {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    throw new RuntimeConfigError('Konfiguration muss ein JSON-Objekt sein.');
  }
  const record = raw as Record<string, unknown>;
  const clientId = requiredString(record, 'clientId');
  if (!CLIENT_ID_PATTERN.test(clientId)) {
    throw new RuntimeConfigError('clientId ist ungültig.');
  }
  const scope = optionalString(record, 'scope') ?? 'openid';
  if (!SCOPE_PATTERN.test(scope) || !scope.split(' ').includes('openid')) {
    throw new RuntimeConfigError('scope muss "openid" enthalten.');
  }
  const logoutUrl = optionalString(record, 'logoutUrl');
  return {
    issuer: secureUrl(requiredString(record, 'issuer'), 'issuer'),
    clientId,
    apiBaseUrl: secureUrl(requiredString(record, 'apiBaseUrl'), 'apiBaseUrl'),
    scope,
    logoutUrl: logoutUrl === null ? null : secureUrl(logoutUrl, 'logoutUrl'),
  };
}

/** Fetches and validates the runtime configuration; any failure is fatal (fail closed). */
export async function loadRuntimeConfig(
  fetchFn: typeof fetch = fetch,
  url: string = RUNTIME_CONFIG_URL,
): Promise<RuntimeConfig> {
  const response = await fetchFn(url, { cache: 'no-store', credentials: 'same-origin' });
  if (!response.ok) {
    throw new RuntimeConfigError(`Konfiguration nicht verfügbar (HTTP ${response.status}).`);
  }
  return parseRuntimeConfig(await response.json());
}

function requiredString(record: Record<string, unknown>, key: string): string {
  const value = optionalString(record, key);
  if (value === null) {
    throw new RuntimeConfigError(`${key} fehlt.`);
  }
  return value;
}

function optionalString(record: Record<string, unknown>, key: string): string | null {
  const value = record[key];
  if (value === undefined || value === null) {
    return null;
  }
  if (typeof value !== 'string' || value.trim() === '') {
    throw new RuntimeConfigError(`${key} muss eine nicht leere Zeichenkette sein.`);
  }
  return value.trim();
}

/** HTTPS only, except plain HTTP on loopback hosts for local development. */
function secureUrl(value: string, key: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new RuntimeConfigError(`${key} ist keine gültige URL.`);
  }
  const loopbackHttp = url.protocol === 'http:' && LOOPBACK_HOSTS.has(url.hostname);
  if (url.protocol !== 'https:' && !loopbackHttp) {
    throw new RuntimeConfigError(`${key} muss HTTPS verwenden.`);
  }
  if (url.username || url.password || url.search || url.hash) {
    throw new RuntimeConfigError(`${key} darf keine Zugangsdaten, Query oder Fragment enthalten.`);
  }
  return `${url.origin}${url.pathname}`.replace(/\/+$/, '');
}
