import { expect, signIn, stabilize, test } from '../support/fixtures';

/**
 * Pixel baselines for the critical screens on desktop and mobile (ADR 010). Baselines live in
 * tests/visual/__screenshots__ and must only be updated after a human reviewed the diff.
 */
test.describe('visual baselines', () => {
  test('signed-out home', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('navigation').getByRole('button', { name: 'Anmelden' })).toBeVisible();
    await stabilize(page);
    await expect(page).toHaveScreenshot('signed-out.png', { fullPage: true });
  });

  test('empty overview', async ({ page }) => {
    await signIn(page);
    await expect(page.getByRole('heading', { name: 'Noch keine Listen' })).toBeVisible();
    await stabilize(page);
    await expect(page).toHaveScreenshot('overview-empty.png', { fullPage: true });
  });

  test('populated overview', async ({ page, backend }) => {
    backend.api.seedList('Wocheneinkauf', ['Milch']);
    backend.api.seedList('WG-Küche', [], 'HOUSEHOLD');
    backend.api.seedList('Grillabend am Samstag');
    await signIn(page);
    await expect(page.getByRole('list', { name: 'Einkaufslisten' }).getByRole('link')).toHaveCount(3);
    await stabilize(page);
    await expect(page).toHaveScreenshot('overview-populated.png', { fullPage: true });
  });

  test('editor with open items', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', [
      { name: 'Hafermilch', quantity: '1', unit: 'l', checked: true },
      { name: 'Äpfel', quantity: '6', unit: 'Stück' },
      { name: 'Vollkornbrot', note: 'vom Bäcker an der Ecke' },
      { name: 'Kaffee', quantity: '1', unit: 'Packung' },
    ]);
    await signIn(page, `/listen/${listId}`);
    await expect(page.getByText('1 von 4 erledigt')).toBeVisible();
    await stabilize(page);
    await expect(page).toHaveScreenshot('editor-open.png', { fullPage: true });
  });

  test('editor with all items completed', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', [
      { name: 'Hafermilch', checked: true },
      { name: 'Äpfel', checked: true },
    ]);
    await signIn(page, `/listen/${listId}`);
    await expect(page.getByText('Alles erledigt.')).toBeVisible();
    await stabilize(page);
    await expect(page).toHaveScreenshot('editor-completed.png', { fullPage: true });
  });

  test('editor with an empty list', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf');
    await signIn(page, `/listen/${listId}`);
    await expect(page.getByText('Diese Liste ist noch leer')).toBeVisible();
    await stabilize(page);
    await expect(page).toHaveScreenshot('editor-empty.png', { fullPage: true });
  });

  test('editor load error', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch']);
    backend.api.failNext('GET', `/api/lists/${listId}/sync`, 503);
    await signIn(page, `/listen/${listId}`);
    await expect(page.getByRole('heading', { name: 'Liste konnte nicht geladen werden' })).toBeVisible();
    await stabilize(page);
    await expect(page).toHaveScreenshot('editor-error.png', { fullPage: true });
  });
});
