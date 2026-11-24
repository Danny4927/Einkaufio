# Einkaufio Web Client

Angular PWA workspace for the first Einkaufio client.

## Toolchain

- Angular / Angular CLI: 22.3.0
- TypeScript: 6.0.3
- Node.js: 24.15.0 (see `.nvmrc`)
- npm: 10.9.2
- Playwright: 1.65.1

Angular 22 requires a supported Node.js and TypeScript version; see the
[official compatibility matrix](https://angular.dev/reference/versions).

## Commands

Run from this directory:

- `npm ci` – reproducible clean install from the committed lockfile
- `npm start` – start the local development server
- `npm run build` – production build
- `npx playwright install chromium` – install the local browser
- `npm run e2e` – end-to-end tests
- `npm run e2e:visual` – deterministic desktop screenshot smoke test

Use `npm install` only when intentionally changing dependencies; review and
commit the resulting package.json and package-lock.json together.

The app is standalone and explicitly zoneless. Component templates and styles
live in separate files. Playwright fixes locale, timezone, color scheme,
viewport and reduced-motion settings; the visual test attaches a screenshot
to the test report for review.

## Reproducibility

Direct dependency versions and the npm version are pinned. The committed
package-lock.json records the full transitive dependency tree. CI validates
that tree with `npm ci`.
