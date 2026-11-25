# Agent instructions

Read this first. Keep this file short; detailed decisions and policies live in docs/.

## Commands

- make check: fast backend formatting, lint, tests and architecture checks plus Terraform formatting.
- make verify: complete CI-equivalent local verification (backend, web build/E2E, Terraform format/validate).
- make test-domain, make test-application, make fmt, make lint: focused feedback loops.
- Web dependencies are installed from apps/web/package-lock.json with npm ci in apps/web.

## Architecture

- Preserve DDD and hexagonal dependency direction.
- Domain and application must not depend on HTTP, AWS SDKs, DynamoDB, infrastructure adapters, or transport DTOs.
- Application owns outbound ports; adapters implement them; API is an inbound adapter/composition root.
- Keep persistence replaceable. Do not leak storage-specific types through ports.
- Update or add an ADR under docs/architecture/decisions/ when an architectural decision changes.

## Security invariants

- Actor identity comes only from a verified Cognito principal/claims at the API boundary. Never trust a client-supplied owner/actor ID.
- Fail closed: unimplemented repository operations return an error, never permissive data or success.
- Authorize before disclosing resource/list/item existence; avoid existence leaks.
- Enforce expected versions atomically with mutations; multi-record writes and aggregate versions must remain consistent.
- Never log access tokens, invitation secrets, full invite URLs, or token hashes.
- See docs/security.md and docs/architecture/arc42.md.

## Definition of Done

- Add tests for behavior, boundaries, failures, authorization, and concurrency as relevant.
- Run the narrowest useful target, then make verify before declaring complete.
- Update docs/roadmap.md when a roadmap milestone or implementation state changes; do not create parallel progress trackers.
- Update OpenAPI/ADR/security/quality docs when affected.
- CI for the PR must be green. Report checks that could not be run; never claim unobserved success.

## References

- Architecture and decisions: docs/architecture/
- Security: docs/security.md
- Quality/evidence policy: docs/quality-engineering.md
- Work plan and implementation state: docs/roadmap.md
