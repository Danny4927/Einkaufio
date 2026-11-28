import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import {
  AbstractSecurityStorage,
  DefaultSessionStorageService,
  LogLevel,
  OpenIdConfiguration,
  provideAuth,
} from 'angular-auth-oidc-client';
import { RuntimeConfig } from '../config/runtime-config';
import { AUTH_CALLBACK_PATH } from './return-url';

/**
 * Authorization Code + PKCE against the Cognito user pool (ADR 018).
 *
 * - No client secret: the browser is a public client.
 * - Tokens, PKCE verifier, state and nonce live in sessionStorage (tab-scoped, cleared when
 *   the tab closes), never in localStorage.
 * - Library logging is off because debug logs contain tokens.
 * - Refresh tokens require Cognito refresh-token rotation; reuse without rotation is refused.
 */
export function buildOidcConfig(config: RuntimeConfig, origin: string): OpenIdConfiguration {
  return {
    authority: config.issuer,
    clientId: config.clientId,
    redirectUrl: `${origin}${AUTH_CALLBACK_PATH}`,
    postLogoutRedirectUri: `${origin}/`,
    responseType: 'code',
    scope: config.scope,
    disablePkce: false,
    useRefreshToken: true,
    silentRenew: true,
    renewTimeBeforeTokenExpiresInSeconds: 60,
    allowUnsafeReuseRefreshToken: false,
    disableRefreshTokenOfflineAccessScopeWarning: true,
    autoUserInfo: false,
    renewUserInfoAfterTokenRenew: false,
    triggerAuthorizationResultEvent: true,
    historyCleanupOff: false,
    logLevel: LogLevel.None,
  };
}

export function provideEinkaufioAuth(config: RuntimeConfig, origin: string): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideAuth({ config: buildOidcConfig(config, origin) }),
    // Explicit so a library default change cannot silently move tokens to localStorage.
    { provide: AbstractSecurityStorage, useClass: DefaultSessionStorageService },
  ]);
}
