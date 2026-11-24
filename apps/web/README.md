# Einkaufio Web Client

Angular PWA workspace for the first Einkaufio client.

## Toolchain

- Angular / Angular CLI: 22.3.0
- TypeScript: 6.0.0
- Node.js: 24.15.0 (see `.nvmrc`)
- npm: 10.9+
- Playwright: 1.65.1

Angular 22 requires a supported Node.js and TypeScript version; see the
[official compatibility matrix](https://angular.dev/reference/versions).

## Commands

Run from this directory:

- `npm install` – install exact direct dependency versions
- `npm start` – start the local development server
- `npm run build` – production build
- `npx playwright install chromium` – install the local browser
- `npm run e2e` – end-to-end tests
- `npm run e2e:visual` – deterministic desktop screenshot smoke test

The app is standalone and explicitly zoneless. Component templates and styles
live in separate files. Playwright fixes locale, timezone, color scheme,
viewport and reduced-motion settings; the visual test attaches a screenshot
to the test report for review.

## Reproducibility

Direct dependency versions are exact, not ranges. A committed npm lockfile is
still required before this workspace is considered fully reproducible. The
current bootstrap CI resolves the exact direct versions and tests the resulting
dependency tree; do not treat this as equivalent to a committed lockfile.
