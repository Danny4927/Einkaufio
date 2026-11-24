# Visual tests

The browser-based visual capture currently lives in `apps/web/tests/visual`.
It fixes viewport, locale, timezone, color scheme and motion settings, then
writes a full-page PNG into Playwright's test-results directory. CI uploads
that image as a downloadable artifact for visual review.

This is an initial screenshot smoke test, **not yet a pixel-baseline regression
gate**. Committed baselines must be captured from the supported CI browser
image and reviewed before enabling `toHaveScreenshot` as a required gate.
Do not update baselines automatically in normal pull-request CI.
