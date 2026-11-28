# ADR-020: Web client authentication, runtime configuration and list synchronisation

- Status: Accepted
- Date: 2026-11-28

## Context

The Angular PWA (A7, B6) needs Cognito sign-in, must call the API with access tokens without
weakening `docs/security.md`, must run unchanged against several environments, and must keep a
shared list current with polling (ADR 004) and optimistic concurrency (HTTP 409).

## Decision

**Authentication.** Use the maintained OIDC library `angular-auth-oidc-client` (pinned exactly)
with the Authorization Code flow and PKCE (S256) against the Cognito user pool. The browser is a
public client: no client secret exists. The library is wrapped by `core/auth/AuthService`;
features never touch tokens.

- Tokens, PKCE verifier, state and nonce are stored in `sessionStorage` (tab-scoped, cleared when
  the tab closes), configured explicitly; nothing is written to `localStorage`.
- ID-token validation (signature via JWKS, iss, aud, nonce, iat, exp) stays on; library logging
  is off because debug logs contain tokens.
- Refresh tokens renew the session silently; reuse without rotation is refused, so the Cognito
  app client must enable refresh-token rotation (otherwise the session ends when the access token
  expires and the user signs in again).
- An HTTP interceptor adds `Authorization: Bearer` only to `${apiBaseUrl}/api/…`; config, IdP and
  any other origin never receive the token. A missing token or an API 401 ends the local session
  (fail closed) and asks the user to sign in again.
- Sign-out clears the local session and, when `logoutUrl` is configured, ends the hosted-UI
  session through Cognito `/logout` (the sign-out URL must be registered on the app client).
- Return URLs after sign-in are restricted to app-internal paths (no open redirect).

**Runtime configuration.** `/config.json` (issuer, clientId, apiBaseUrl, optional scope and
logoutUrl) is fetched with `no-store` and validated before Angular boots; invalid or missing
configuration shows a static error page instead of starting with defaults. URLs must be HTTPS
(plain HTTP only on loopback). The file holds public identifiers only and must be served with a
short cache lifetime like `index.html` (ADR 007); the deployment (A3) writes it per environment.

**Synchronisation.** Only the list open in the editor polls `GET /api/lists/{id}/sync?since=` every
15 seconds, and only while the document is visible and online; becoming visible, focus and the
`online` event refresh immediately; leaving the editor stops polling. The editor shows the last
server-confirmed state with pending operations projected on top, so a poll never overwrites an
optimistic change. Requests run strictly one after another and read expected versions from the
confirmed state when sent. A 409 (or an item that vanished) drops all pending operations, reloads a
snapshot and explains the conflict; other failures roll back only the failed operation.

## Consequences

- XSS in the app could still read `sessionStorage`; CSP/security headers (ADR 007, security.md)
  remain required. Refresh-token revocation on sign-out is not implemented yet.
- Behaviour against a real Cognito pool (discovery document, refresh rotation, `/logout`) is only
  verified with a mocked IdP until a pool exists (A2); E2E tests sign real RS256 ID tokens and
  verify PKCE in the mock, so the library's full validation path runs.
- Polling traffic is bounded to one request per visible editor per 15 seconds.
- Strictly serial mutations add latency under fast repeated edits but keep versions consistent.

## Alternatives

- Hand-rolled PKCE: rejected; security.md asks for a maintained library.
- In-memory token storage: breaks the redirect flow (verifier/state must survive the redirect)
  and loses the session on every reload.
- `localStorage`: rejected by security.md (long-lived secrets).
- Build-time `environment.ts`: requires one build per environment and prevents promoting the same
  artifact from dev to prod (D3).
