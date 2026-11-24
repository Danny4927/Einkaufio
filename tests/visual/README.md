# Visual tests

The browser-based visual capture currently lives in `apps/web/tests/visual`.
It fixes viewport, locale, timezone, color scheme and motion settings, then
attaches a full-page PNG to the Playwright report.

This is an initial screenshot smoke test, **not yet a pixel-baseline regression
gate**. Committed baselines must be captured from the supported CI browser
image and reviewed before enabling `toHaveScreenshot` as a required gate.
Do not update baselines automatically in normal pull-request CI.
