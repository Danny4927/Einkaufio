import { expect, test as base, type Page } from '@playwright/test';
import { API_ORIGIN, MockApi } from './mock-api';
import { CLIENT_ID, ISSUER, LOGOUT_URL, MockIdp } from './mock-idp';

export interface Backend {
  readonly idp: MockIdp;
  readonly api: MockApi;
}

/**
 * Every test runs against the mocked IdP and API (auto fixture, also when a test does not use
 * `backend`). Requests to any other non-local host are aborted, so nothing leaves the machine.
 */
export const test = base.extend<{ backend: Backend }>({
  backend: [async ({ page }, use) => {
    const idp = new MockIdp();
    const api = new MockApi(idp.accessToken);
    // Registered first, so the specific mocks below (matched last-registered-first) win.
    await page.route(/^https?:\/\/(?!127\.0\.0\.1[:/])/, (route) => route.abort('blockedbyclient'));
    await page.route('**/config.json', (route) =>
      route.fulfill({ json: { issuer: ISSUER, clientId: CLIENT_ID, apiBaseUrl: API_ORIGIN, scope: 'openid', logoutUrl: LOGOUT_URL } }),
    );
    await idp.install(page);
    await api.install(page);
    await use({ idp, api });
  }, { auto: true }],
});

export { expect };

/** Opens `path`, gets redirected to sign-in, signs in through the mocked hosted UI and returns. */
export async function signIn(page: Page, path = '/listen'): Promise<void> {
  await page.goto(path);
  await expect(page.getByText('Bitte melde dich an, um deine Listen zu sehen.')).toBeVisible();
  await page.getByRole('main').getByRole('button', { name: 'Anmelden' }).click();
  await page.waitForURL((url) => url.pathname === path);
}

/** Freezes everything that animates so screenshots are stable. */
export async function stabilize(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
  await page.addStyleTag({ content: '*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }' });
}
