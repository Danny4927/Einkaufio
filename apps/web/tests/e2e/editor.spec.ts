import type { Page } from '@playwright/test';
import { expect, signIn, test } from '../support/fixtures';

const itemNames = (page: Page) => page.getByRole('list', { name: 'Artikel' }).locator('.item-name');

test.describe('list editor', () => {
  test('adds, checks, edits, reorders and deletes items', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch', 'Brot']);
    await signIn(page, `/listen/${listId}`);

    await page.getByRole('textbox', { name: 'Artikel' }).fill('Äpfel');
    await page.getByLabel('Menge').fill('6');
    await page.getByLabel('Einheit').fill('Stück');
    await page.getByRole('button', { name: 'Hinzufügen' }).click();
    await expect(itemNames(page)).toHaveText(['Milch', 'Brot', 'Äpfel']);
    await expect(page.getByText('6 Stück')).toBeVisible();

    await page.getByRole('checkbox', { name: 'Milch' }).check();
    await expect(page.getByText('1 von 3 erledigt')).toBeVisible();

    await page.getByRole('button', { name: '„Brot“ bearbeiten' }).click();
    const edit = page.getByRole('form', { name: '„Brot“ bearbeiten' });
    await edit.getByLabel('Artikel').fill('Vollkornbrot');
    await edit.getByLabel('Notiz').fill('vom Bäcker');
    await edit.getByRole('button', { name: 'Speichern' }).click();
    await expect(page.getByText('vom Bäcker')).toBeVisible();

    await page.getByRole('button', { name: '„Äpfel“ nach oben verschieben' }).click();
    await expect(itemNames(page)).toHaveText(['Milch', 'Äpfel', 'Vollkornbrot']);

    await page.getByRole('button', { name: '„Milch“ löschen' }).click();
    await expect(itemNames(page)).toHaveText(['Äpfel', 'Vollkornbrot']);
    await expect(page.getByText('Aktuell')).toBeVisible();

    expect(backend.api.items(listId)).toMatchObject([
      { name: 'Äpfel', quantity: '6', unit: 'Stück', checked: false },
      { name: 'Vollkornbrot', note: 'vom Bäcker' },
    ]);
  });

  test('reloads and explains a 409 conflict caused by another person', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch', 'Brot']);
    await signIn(page, `/listen/${listId}`);
    await expect(itemNames(page)).toHaveText(['Milch', 'Brot']);

    const [milk] = backend.api.items(listId);
    backend.api.otherClientChecks(listId, milk.id);
    await page.getByRole('button', { name: '„Milch“ bearbeiten' }).click();
    await page.getByRole('form', { name: '„Milch“ bearbeiten' }).getByLabel('Artikel').fill('Hafermilch');
    await page.getByRole('button', { name: 'Speichern' }).click();

    await expect(page.getByRole('alert')).toContainText('Die Liste wurde zwischenzeitlich geändert');
    // The server state wins: the other person's check is shown, the stale rename is not.
    await expect(itemNames(page)).toHaveText(['Milch', 'Brot']);
    await expect(page.getByRole('checkbox', { name: 'Milch' })).toBeChecked();
    expect(backend.api.items(listId)[0].name).toBe('Milch');
  });

  test('rolls back a change the server could not store', async ({ page, backend }) => {
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch']);
    await signIn(page, `/listen/${listId}`);
    backend.api.failNext('DELETE', `/items/${backend.api.items(listId)[0].id}`, 503);
    await page.getByRole('button', { name: '„Milch“ löschen' }).click();
    await expect(page.getByRole('alert')).toContainText('Die Änderung konnte nicht gespeichert werden');
    await expect(itemNames(page)).toHaveText(['Milch']);
  });

  test('shows a not-found state for inaccessible lists', async ({ page }) => {
    await signIn(page, '/listen/00000000-0000-4000-8000-999999999999');
    await expect(page.getByRole('heading', { name: 'Liste nicht gefunden' })).toBeVisible();
  });
});

test.describe('list synchronisation', () => {
  test('polls the visible list every 15 seconds with since, pauses when hidden and refreshes on focus', async ({ page, backend }) => {
    await page.clock.install();
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch']);
    await signIn(page, `/listen/${listId}`);
    await expect(itemNames(page)).toHaveText(['Milch']);
    const initial = backend.api.syncRequests(listId).length;

    backend.api.otherClientAdds(listId, { name: 'Kaffee' });
    await page.clock.fastForward(15_000);
    await expect(itemNames(page)).toHaveText(['Milch', 'Kaffee']);
    const polls = backend.api.syncRequests(listId).slice(initial);
    expect(polls.length).toBeGreaterThanOrEqual(1);
    expect(polls.every((request) => request.search.has('since'))).toBe(true);

    // Background tab: no polling at all.
    await page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    const beforeHidden = backend.api.syncRequests(listId).length;
    backend.api.otherClientAdds(listId, { name: 'Tee' });
    await page.clock.fastForward(60_000);
    expect(backend.api.syncRequests(listId).length).toBe(beforeHidden);
    await expect(itemNames(page)).toHaveText(['Milch', 'Kaffee']);

    // Visible again: refresh immediately, without waiting for the interval.
    await page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await expect(itemNames(page)).toHaveText(['Milch', 'Kaffee', 'Tee']);

    backend.api.otherClientAdds(listId, { name: 'Zucker' });
    await page.evaluate(() => window.dispatchEvent(new Event('focus')));
    await expect(itemNames(page)).toHaveText(['Milch', 'Kaffee', 'Tee', 'Zucker']);
  });

  test('leaving the editor stops polling', async ({ page, backend }) => {
    await page.clock.install();
    const listId = backend.api.seedList('Wocheneinkauf', ['Milch']);
    await signIn(page, `/listen/${listId}`);
    await expect(itemNames(page)).toHaveText(['Milch']);
    await page.getByRole('link', { name: 'Alle Listen' }).click();
    await expect(page.getByRole('heading', { name: 'Meine Listen' })).toBeVisible();
    const count = backend.api.syncRequests(listId).length;
    await page.clock.fastForward(45_000);
    expect(backend.api.syncRequests(listId).length).toBe(count);
  });
});
