import { TestBed } from '@angular/core/testing';
import { AbstractSecurityStorage, DefaultSessionStorageService, LogLevel } from 'angular-auth-oidc-client';
import { TEST_RUNTIME_CONFIG } from '../testing/test-config';
import { buildOidcConfig, provideEinkaufioAuth } from './auth-config';

describe('buildOidcConfig', () => {
  const config = buildOidcConfig(TEST_RUNTIME_CONFIG, 'https://app.example.test');

  it('uses the authorization code flow with PKCE against the configured issuer', () => {
    expect(config.authority).toBe(TEST_RUNTIME_CONFIG.issuer);
    expect(config.clientId).toBe(TEST_RUNTIME_CONFIG.clientId);
    expect(config.responseType).toBe('code');
    expect(config.disablePkce).toBe(false);
    expect(config.redirectUrl).toBe('https://app.example.test/auth/callback');
  });

  it('keeps token validation strict and logging off', () => {
    expect(config.disableIdTokenValidation).toBeUndefined();
    expect(config.issValidationOff).toBeUndefined();
    expect(config.allowUnsafeReuseRefreshToken).toBe(false);
    expect(config.logLevel).toBe(LogLevel.None);
  });

  it('stores tokens in sessionStorage, not localStorage', () => {
    TestBed.configureTestingModule({ providers: [provideEinkaufioAuth(TEST_RUNTIME_CONFIG, 'https://app.example.test')] });
    expect(TestBed.inject(AbstractSecurityStorage)).toBeInstanceOf(DefaultSessionStorageService);
  });
});
