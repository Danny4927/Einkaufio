# ADR-006: Platform-neutral API and future Android client

- Status: Accepted
- Date: 2026-11-24

## Context
Angular PWA is the first client, but a native Android app may be added. The design must not force a PWA-only product.

## Decision
The backend is client-independent. OpenAPI is the source of truth. Web and Android use the same Cognito User Pool, user `sub`, HTTPS REST API, authorization rules and request/response semantics.

Keep business logic server-side. Use bearer access tokens rather than browser-only sessions. Keep secure token storage, app navigation, Android App Links, notification APIs and local persistence in the native client. Defer the Android stack decision; Kotlin/Jetpack Compose is a reasonable default candidate.

## Consequences
- Native Android can be added without duplicating backend logic.
- OpenAPI compatibility is a release requirement.
- Client models can be generated or validated from OpenAPI.
- Android may use Authorization Code + PKCE and Keystore-backed token storage.
- Invitation URLs remain stable HTTPS links; Android App Links can later open them in-app with web fallback.
- Offline write queues and conflict UX are client concerns.
- Push notifications are deferred; polling is used initially.

## Alternatives
PWA-only assumptions and a separate Android backend were rejected. A shared UI framework is not required; Angular web and native Android can coexist.
