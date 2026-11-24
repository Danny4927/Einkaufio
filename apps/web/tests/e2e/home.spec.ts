import { expect, test } from '@playwright/test';

test('home page explains the shared shopping-list concept', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle(/Einkaufio/);
  await expect(page.getByRole('heading', { level: 1 })).toContainText('zusammen');
  await expect(page.getByRole('navigation', { name: 'Hauptnavigation' })).toBeVisible();
  await expect(page.getByRole('list', { name: 'Vorschau einer Einkaufsliste' })).toBeVisible();
});

test('feature section is reachable from the primary link', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('link', { name: /So funktioniert Einkaufio/ }).click();
  await expect(page.getByRole('heading', { name: 'Alles auf einer Liste.' })).toBeInViewport();
});
