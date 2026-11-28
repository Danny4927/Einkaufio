import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto';
import type { Page, Route } from '@playwright/test';

export const IDP_ORIGIN = 'https://idp.einkaufio.test';
export const ISSUER = `${IDP_ORIGIN}/eu-central-1_E2E`;
export const CLIENT_ID = 'e2e-client';
export const LOGOUT_URL = `${IDP_ORIGIN}/logout`;
export const APP_ORIGIN = 'http://127.0.0.1:4200';

const KEY_ID = 'e2e-key';
const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const publicJwk = { ...publicKey.export({ format: 'jwk' }), kid: KEY_ID, alg: 'RS256', use: 'sig' };

const CORS = { 'access-control-allow-origin': APP_ORIGIN, 'access-control-allow-headers': 'content-type' };

function base64Url(value: Buffer | string): string {
  return Buffer.from(value).toString('base64url');
}

interface PendingAuthorization {
  readonly codeChallenge: string;
  readonly nonce: string;
  readonly redirectUri: string;
}

/**
 * Stand-in for the Cognito hosted UI and OAuth endpoints. It signs real RS256 ID tokens, so the
 * OIDC library runs its full validation, and it verifies PKCE (S256) on the token exchange.
 */
export class MockIdp {
  readonly accessToken = `e2e-access-${randomBytes(12).toString('hex')}`;
  readonly refreshToken = `e2e-refresh-${randomBytes(12).toString('hex')}`;
  readonly authorizeRequests: URLSearchParams[] = [];
  readonly tokenRequests: URLSearchParams[] = [];
  readonly logoutRequests: URLSearchParams[] = [];
  pkceVerified = false;
  private readonly codes = new Map<string, PendingAuthorization>();

  async install(page: Page): Promise<void> {
    await page.route(`${IDP_ORIGIN}/**`, (route) => this.handle(route));
  }

  private async handle(route: Route): Promise<void> {
    const request = route.request();
    const url = new URL(request.url());
    if (request.method() === 'OPTIONS') {
      return route.fulfill({ status: 204, headers: { ...CORS, 'access-control-allow-methods': 'GET, POST' } });
    }
    switch (url.pathname) {
      case '/eu-central-1_E2E/.well-known/openid-configuration':
        return route.fulfill({ headers: CORS, json: this.discovery() });
      case '/eu-central-1_E2E/.well-known/jwks.json':
        return route.fulfill({ headers: CORS, json: { keys: [publicJwk] } });
      case '/oauth2/authorize':
        return this.authorize(route, url.searchParams);
      case '/oauth2/token':
        return this.token(route, new URLSearchParams(request.postData() ?? ''));
      case '/logout':
        return this.logout(route, url.searchParams);
      default:
        return route.fulfill({ status: 404, headers: CORS, body: 'not found' });
    }
  }

  private discovery() {
    return {
      issuer: ISSUER,
      authorization_endpoint: `${IDP_ORIGIN}/oauth2/authorize`,
      token_endpoint: `${IDP_ORIGIN}/oauth2/token`,
      jwks_uri: `${ISSUER}/.well-known/jwks.json`,
      response_types_supported: ['code'],
      subject_types_supported: ['public'],
      id_token_signing_alg_values_supported: ['RS256'],
      scopes_supported: ['openid', 'email', 'profile'],
    };
  }

  private authorize(route: Route, params: URLSearchParams): Promise<void> {
    this.authorizeRequests.push(params);
    const redirectUri = params.get('redirect_uri') ?? '';
    const valid =
      params.get('response_type') === 'code' &&
      params.get('client_id') === CLIENT_ID &&
      params.get('code_challenge_method') === 'S256' &&
      (params.get('code_challenge') ?? '').length >= 43 &&
      redirectUri === `${APP_ORIGIN}/auth/callback`;
    if (!valid) {
      return route.fulfill({ status: 400, contentType: 'text/html', body: '<h1>invalid_request</h1>' });
    }
    const code = randomBytes(16).toString('hex');
    this.codes.set(code, { codeChallenge: params.get('code_challenge') ?? '', nonce: params.get('nonce') ?? '', redirectUri });
    const target = `${redirectUri}?code=${code}&state=${encodeURIComponent(params.get('state') ?? '')}`;
    return htmlRedirect(route, target);
  }

  private token(route: Route, body: URLSearchParams): Promise<void> {
    this.tokenRequests.push(body);
    const pending = this.codes.get(body.get('code') ?? '');
    const verifier = body.get('code_verifier') ?? '';
    const challenge = createHash('sha256').update(verifier).digest('base64url');
    if (
      body.get('grant_type') !== 'authorization_code' ||
      body.get('client_id') !== CLIENT_ID ||
      body.has('client_secret') ||
      pending === undefined ||
      pending.codeChallenge !== challenge ||
      pending.redirectUri !== body.get('redirect_uri')
    ) {
      return route.fulfill({ status: 400, headers: CORS, json: { error: 'invalid_grant' } });
    }
    this.codes.delete(body.get('code') ?? '');
    this.pkceVerified = true;
    return route.fulfill({
      headers: CORS,
      json: {
        access_token: this.accessToken,
        id_token: this.idToken(pending.nonce),
        refresh_token: this.refreshToken,
        token_type: 'Bearer',
        expires_in: 3600,
      },
    });
  }

  private logout(route: Route, params: URLSearchParams): Promise<void> {
    this.logoutRequests.push(params);
    const target = params.get('logout_uri') ?? '';
    if (params.get('client_id') !== CLIENT_ID || target !== `${APP_ORIGIN}/`) {
      return route.fulfill({ status: 400, contentType: 'text/html', body: '<h1>invalid logout</h1>' });
    }
    return htmlRedirect(route, target);
  }

  private idToken(nonce: string): string {
    const now = Math.floor(Date.now() / 1000);
    const header = base64Url(JSON.stringify({ alg: 'RS256', kid: KEY_ID, typ: 'JWT' }));
    const payload = base64Url(
      JSON.stringify({
        iss: ISSUER,
        aud: CLIENT_ID,
        sub: 'e2e-user-anna',
        token_use: 'id',
        email: 'anna@example.test',
        auth_time: now,
        iat: now,
        exp: now + 3600,
        nonce,
      }),
    );
    const signature = sign('RSA-SHA256', Buffer.from(`${header}.${payload}`), privateKey);
    return `${header}.${payload}.${base64Url(signature)}`;
  }
}

/** Navigations cannot be fulfilled with a cross-origin 302 reliably; a tiny page redirects instead. */
function htmlRedirect(route: Route, target: string): Promise<void> {
  return route.fulfill({
    status: 200,
    contentType: 'text/html',
    body: `<!doctype html><title>Weiterleitung</title><script>location.replace(${JSON.stringify(target)});</script>`,
  });
}
