import { expect, signIn, test } from '../support/fixtures';

test.describe('sign-in with Cognito (mocked hosted UI)', () => {
  test('protected routes redirect signed-out users without calling the API', async ({ page, backend }) => {
    await page.goto('/listen');
    await expect(page).toHaveURL('/?anmeldung=erforderlich');
    await expect(page.getByText('Bitte melde dich an, um deine Listen zu sehen.')).toBeVisible();
    await expect(page.getByRole('navigation').getByRole('button', { name: 'Anmelden' })).toBeVisible();
    expect(backend.api.requests).toHaveLength(0);
  });

  test('authorization code + PKCE flow signs in and calls the API with the access token', async ({ page, backend }) => {
    await signIn(page);
    await expect(page.getByRole('heading', { name: 'Meine Listen' })).toBeVisible();

    const authorize = backend.idp.authorizeRequests[0];
    expect(authorize.get('response_type')).toBe('code');
    expect(authorize.get('code_challenge_method')).toBe('S256');
    expect(authorize.get('scope')).toBe('openid');
    expect(backend.idp.pkceVerified).toBe(true);
    expect(backend.idp.tokenRequests[0].has('client_secret')).toBe(false);

    const apiCalls = backend.api.requests;
    expect(apiCalls.length).toBeGreaterThan(0);
    expect(apiCalls.every((request) => request.authorization === `Bearer ${backend.idp.accessToken}`)).toBe(true);
    // The authorization code is removed from the address bar after the exchange.
    expect(page.url()).not.toContain('code=');
  });

  test('tokens stay out of localStorage', async ({ page, backend }) => {
    await signIn(page);
    await expect(page.getByRole('heading', { name: 'Meine Listen' })).toBeVisible();
    const local = await page.evaluate(() => JSON.stringify({ ...localStorage }));
    expect(local).not.toContain(backend.idp.accessToken);
    expect(local).not.toContain(backend.idp.refreshToken);
    const session = await page.evaluate(() => JSON.stringify({ ...sessionStorage }));
    expect(session).toContain(backend.idp.accessToken);
  });

  test('returns to the originally requested list after sign-in', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch']);
    await signIn(page, `/listen/${listId}`);
    await expect(page.getByRole('heading', { name: 'Wocheneinkauf' })).toBeVisible();
  });

  test('a forged callback is rejected', async ({ page, backend }) => {
    await page.goto('/auth/callback?code=forged&state=forged');
    await expect(page.getByRole('heading', { name: 'Anmeldung fehlgeschlagen' })).toBeVisible();
    expect(backend.idp.tokenRequests).toHaveLength(0);
  });

  test('an API 401 ends the session and asks to sign in again', async ({ page, backend }) => {
    await signIn(page);
    await expect(page.getByRole('heading', { name: 'Meine Listen' })).toBeVisible();
    backend.api.failNext('GET', '/api/lists', 401);
    await page.reload();
    await expect(page).toHaveURL('/?sitzung=abgelaufen');
    await expect(page.getByText('Deine Sitzung ist abgelaufen. Bitte melde dich erneut an.')).toBeVisible();
    await page.goto('/listen');
    await expect(page).toHaveURL('/?anmeldung=erforderlich');
  });

  test('sign-out clears the session and ends the hosted-UI session', async ({ page, backend }) => {
    await signIn(page);
    await page.getByRole('button', { name: 'Abmelden' }).click();
    await page.waitForURL('/');
    expect(backend.idp.logoutRequests).toHaveLength(1);
    await expect(page.getByRole('navigation').getByRole('button', { name: 'Anmelden' })).toBeVisible();
    const session = await page.evaluate(() => JSON.stringify({ ...sessionStorage }));
    expect(session).not.toContain(backend.idp.accessToken);
    await page.goto('/listen');
    await expect(page).toHaveURL('/?anmeldung=erforderlich');
  });
});
