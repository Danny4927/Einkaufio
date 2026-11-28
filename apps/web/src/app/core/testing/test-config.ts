import { RuntimeConfig } from '../config/runtime-config';

/** Runtime configuration used by unit tests. */
export const TEST_RUNTIME_CONFIG: RuntimeConfig = {
  issuer: 'https://idp.example.test/pool',
  clientId: 'test-client',
  apiBaseUrl: 'https://api.example.test',
  scope: 'openid',
  logoutUrl: null,
};
