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
- `npm test` – vitest unit tests for core services and feature components
- `npm run lint` – ESLint incl. import boundaries (core / shared / features)
- `npm run e2e` – Playwright E2E and visual baselines (desktop + mobile)
- `npm run e2e:visual` – only the screenshot baselines
- `npx playwright test tests/visual --update-snapshots` – re-record baselines (review every diff first, ADR 010)

If the pinned Playwright browser cannot be downloaded, point Playwright at a local Chromium:
`PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/path/to/chrome npm run e2e`. CI does not set it.

Use `npm install` only when intentionally changing dependencies; review and
commit the resulting package.json and package-lock.json together.

## Runtime configuration and sign-in

`/config.json` is loaded and validated before Angular starts (ADR 020); without a valid file the
app shows an error page instead of starting. Fields: `issuer` (Cognito user-pool issuer),
`clientId` (public app client, no secret), `apiBaseUrl`, optional `scope` (must contain `openid`)
and optional `logoutUrl` (Cognito hosted-UI `/logout`). URLs must use HTTPS except on loopback.
`public/config.json` holds local-development placeholders; the deployment writes the real file
and must serve it with a short cache lifetime. The Cognito app client needs
`<origin>/auth/callback` as callback URL, `<origin>/` as sign-out URL and refresh-token rotation.

Sign-in uses the authorization code flow with PKCE (`angular-auth-oidc-client`). Tokens live in
sessionStorage only and are attached solely to `${apiBaseUrl}/api/` requests.

## Layout

- `src/app/core`: runtime config, auth (service, guard, interceptor), typed API client, polling.
- `src/app/features/home|auth|lists`: signed-out page, OIDC callback, list overview and editor.
- `tests/support`: mocked Cognito IdP (signs RS256 ID tokens, verifies PKCE) and in-memory API;
  every E2E test runs against them and requests to any other host are aborted.

The app is standalone and explicitly zoneless. Component templates and styles
live in separate files. Playwright fixes locale, timezone, color scheme,
viewport and reduced-motion settings; visual tests compare against reviewed
baselines in `tests/visual/__screenshots__` (Linux, per project).

## Reproducibility

Direct dependency versions and the npm version are pinned. The committed
package-lock.json records the full transitive dependency tree. CI validates
that tree with `npm ci`.
