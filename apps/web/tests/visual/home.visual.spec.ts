import { expect, test } from '@playwright/test';

test('desktop home renders a stable visual capture', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'chromium', 'Desktop visual capture runs once per CI build.');
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/');
  await page.evaluate(() => document.fonts.ready);
  await page.addStyleTag({ content: '*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }' });
  const screenshot = await page.screenshot({ path: testInfo.outputPath('home-desktop.png'), fullPage: true, animations: 'disabled' });
  expect(screenshot.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
  expect(screenshot.byteLength).toBeGreaterThan(10_000);
  await testInfo.attach('home-desktop.png', { body: screenshot, contentType: 'image/png' });
});
