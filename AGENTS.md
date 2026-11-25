# Agent instructions

Read this first. Keep it concise; detailed decisions and policies live in docs/.

## Repository map

- `apps/web/`: Angular PWA, package lock, Playwright configuration and browser tests.
- `backend/crates/domain/`: pure domain model, value objects and invariants.
- `backend/crates/application/`: use cases and application-owned ports.
- `backend/crates/adapters/`: persistence and external-system adapters.
- `backend/crates/api/`: Axum/Lambda inbound adapter and composition root.
- `contracts/openapi/`: versioned HTTP contract.
- `infra/terraform/`: bootstrap, modules and environment roots.
- `tests/architecture/`: automated architecture boundary tests.
- `tests/e2e/` and frontend Playwright specs: end-to-end/browser verification.
- `docs/architecture/`: arc42 and ADRs; `docs/roadmap.md` is the implementation source of truth.

## Commands

- `make check`: fast backend formatting, lint, tests and architecture checks plus Terraform formatting.
- `make verify`: full local verification through the same Makefile targets invoked by CI.
- `make test-domain`, `make test-application`, `make fmt`, `make lint`: focused feedback loops.
- `make web-e2e-setup`: install the Playwright browser without installing OS packages or requiring root.
- `make agent-config-check`: validate Claude settings and hook behavior; requires jq, Python and ShellCheck if installed.
- Web dependencies use `apps/web/package-lock.json` and `npm ci` from `apps/web`.

## Code conventions

- Keep Rust domain/application crates free of HTTP, AWS SDK, DynamoDB, infrastructure and transport DTO dependencies.
- Use `Result` and explicit domain/application errors for fallible behavior. Do not use `unwrap()` or `expect()` in production paths; tests may use them when failure is intentional and clear.
- Propagate errors with context where useful; map infrastructure errors at adapter boundaries and transport errors at the API boundary. Never silently convert failures into success or permissive defaults.
- Keep handlers thin: parse/validate transport input, derive the verified actor, call a use case, map its result.
- Put unit tests alongside Rust modules in `#[cfg(test)]`; put cross-crate/integration tests in the owning crate's `tests/` directory; architecture tests live in `tests/architecture/`.
- Angular templates and styles remain in separate files. Keep feature code behind typed services and enforce import boundaries.
- Prefer small, explicit functions and types over generic abstractions without a demonstrated use case.

## Architecture

- Preserve DDD and hexagonal dependency direction.
- Application owns outbound ports; adapters implement them; API is an inbound adapter/composition root.
- Keep persistence replaceable. Do not leak storage-specific types through ports.
- Record architectural changes as ADRs under `docs/architecture/decisions/`.

## Security invariants

- Actor identity comes only from verified Cognito principal/claims at the API boundary. Never trust a client-supplied owner/actor ID.
- Fail closed: unimplemented repository operations return an error, never permissive data or success.
- Authorize before disclosing resource/list/item existence; avoid existence leaks.
- Enforce expected versions atomically with mutations; multi-record writes and aggregate versions must remain consistent.
- Never log access tokens, invitation secrets, full invite URLs, or token hashes.
- See `docs/security.md` and `docs/architecture/arc42.md`.

## Definition of Done

- Add tests for behavior, boundaries, failures, authorization, and concurrency as relevant.
- Run the narrowest useful target, then `make verify` before declaring complete.
- Update `docs/roadmap.md` when implementation state changes; do not create parallel progress trackers.
- Update OpenAPI/ADR/security/quality docs when affected.
- PR CI must be green. Report checks that could not be run; never claim unobserved success.

## References

- Architecture and decisions: `docs/architecture/`
- Security: `docs/security.md`
- Quality/evidence policy: `docs/quality-engineering.md`
- Work plan and implementation state: `docs/roadmap.md`
