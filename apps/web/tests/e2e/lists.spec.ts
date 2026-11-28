import { expect, signIn, test } from '../support/fixtures';

test.describe('list overview', () => {
  test('creates the first list from the empty state and opens it', async ({ page, backend }) => {
    await signIn(page);
    await expect(page.getByRole('heading', { name: 'Noch keine Listen' })).toBeVisible();
    await page.getByRole('textbox', { name: 'Neue Liste' }).fill('Grillabend');
    await page.getByRole('button', { name: 'Anlegen' }).click();
    await expect(page.getByRole('heading', { name: 'Grillabend' })).toBeVisible();
    await expect(page).toHaveURL(/\/listen\/[0-9a-f-]{36}$/);
    expect(backend.api.listNames()).toEqual(['Grillabend']);
    const create = backend.api.requests.find((request) => request.method === 'POST');
    expect(create?.path).toBe('/api/lists');
  });

  test('rejects an empty name without calling the API', async ({ page, backend }) => {
    await signIn(page);
    await page.getByRole('button', { name: 'Anlegen' }).click();
    await expect(page.getByRole('alert')).toContainText('Bitte gib einen Namen');
    expect(backend.api.requests.filter((request) => request.method === 'POST')).toHaveLength(0);
  });

  test('shows personal and household lists and opens one', async ({ page, backend }) => {
    backend.api.seedList('Wocheneinkauf', ['Milch']);
    backend.api.seedList('WG-Küche', [], 'HOUSEHOLD');
    await signIn(page);
    const lists = page.getByRole('list', { name: 'Einkaufslisten' });
    await expect(lists.getByRole('link')).toHaveCount(2);
    await expect(lists.getByRole('link', { name: /WG-Küche/ })).toContainText('Haushalt');
    await lists.getByRole('link', { name: /Wocheneinkauf/ }).click();
    await expect(page.getByRole('heading', { name: 'Wocheneinkauf' })).toBeVisible();
  });

  test('offers a retry when the lists cannot be loaded', async ({ page, backend }) => {
    backend.api.failNext('GET', '/api/lists', 503);
    await signIn(page);
    await expect(page.getByRole('heading', { name: 'Listen konnten nicht geladen werden' })).toBeVisible();
    await page.getByRole('button', { name: 'Erneut versuchen' }).click();
    await expect(page.getByRole('heading', { name: 'Noch keine Listen' })).toBeVisible();
  });
});
