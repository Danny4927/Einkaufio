# Einkaufio – Implementation roadmap

## Delivery rules

- Use DDD and hexagonal architecture from the first backend implementation.
- Keep DynamoDB-specific types and key design inside the DynamoDB adapter.
- Every work item must have acceptance criteria and a verification plan before implementation.
- Every completed step must attach objective evidence: passing test/CI output, screenshots and visual diffs for UI changes, infrastructure plan/deployment output where relevant, and smoke-test results.
- Do not mark work complete based on code inspection or an unverified claim alone.
- Frontend UI changes require visual regression coverage or a documented reason why the change cannot be meaningfully screenshot-tested.
- Keep this file as the single implementation-progress source of truth. Create one GitHub issue per independently deliverable work item with acceptance criteria and verification steps; link it here when scheduled.

## Agent and quality foundation (ADR 012)

- [x] Add concise root agent instructions and canonical documentation links.
- [x] Add local make verify plus focused targets; CI invokes the same Makefile targets.
- [x] Add PR evidence/security guidance.
- [x] Register the Claude Code PreToolUse commit hook and add a deterministic hook test. Real runtime verification remains environment-specific.
- [ ] Verify the hook in a real Claude Code runtime with /hooks, /status, and a disposable test commit.
- [x] Add shared list/item repository contract scenarios and an in-memory reference adapter. DynamoDB still requires transaction and conditional-write integration tests.
- [x] Add the Axum HTTP transport foundation with a public health endpoint and personal-list routes.
- [ ] **Cognito JWT authentication (PR #12):** verify RS256 access-token signatures using issuer-derived Cognito JWKS; validate issuer, expiry, not-before, token use, app client and subject; inject a verified actor only after validation. Add deterministic token/JWKS tests and wire runtime configuration.
- [ ] Add dependency/security policy checks and scoped mutation testing.
- [ ] Configure GitHub branch protection after stable CI check names.
- [ ] Evaluate advisory agent review automation with least-privilege permissions.

## v1 product scope

- Required accounts and sign-in.
- Personal shopping lists with explicit sharing.
- Households with owner/member roles and multiple household-owned lists.
- Item create/update/delete/check/reorder.
- Secure invitation links.
- Version-based polling (15 seconds while list view is visible).
- Responsive Angular PWA.
- Platform-neutral API for a future Android client.
- Replaceable persistence behind application-owned ports.

## Initial product limits (configurable)

- 10 members per household.
- 20 lists per household.
- 100 active items per list.
- 5 simultaneously valid invitations per resource.
- Invitation lifetime default 7 days, maximum 30 days.
- Invitation maxUses default 5, maximum 10.

## Milestones

### M0 – Repository foundation

- [ ] Add license and contribution guidelines.
- [ ] Validate ADRs and OpenAPI in CI.
- [x] Establish Rust workspace, Angular app, monorepo and architecture boundaries.
- [ ] Add local development and environment configuration docs.

### M1 – AWS infrastructure

- [ ] Bootstrap remote Terraform state with encryption, versioning and locking.
- [ ] Provision frontend S3, CloudFront and OAC.
- [ ] Provision ARM64 Lambda and DynamoDB.
- [ ] Provision Cognito User Pool and public app client.
- [ ] Define least-privilege Lambda IAM, CloudWatch retention/alarms and AWS Budget notifications.
- [ ] Add GitHub OIDC deployment roles and protected deployment workflow.
- Evidence: reviewed plan, successful dev apply, endpoint smoke test, least-privilege review and resource/cost inventory.

### M2 – Authentication/profile

- [ ] Cognito register/sign-in/password reset.
- [ ] Finish Cognito JWT verifier and deterministic security tests (PR #12).
- [ ] Wire Cognito settings into the runtime composition root and deployment configuration.
- [ ] Add Angular auth flow and protected routes.
- [ ] Add profile use cases and repository port.
- Evidence: invalid/expired/wrong-use JWT tests, API integration output and browser evidence for auth states.

### M3 – Personal lists/items

- [x] Personal-list domain types, aggregate and version semantics.
- [x] Create/list/rename/delete application use cases.
- [x] Item domain types and add/check/delete application use cases.
- [x] Shared repository contract scenarios execute against the in-memory reference adapter.
- [ ] Implement DynamoDB persistence adapter with atomic owner reference/list writes, item capacity/position and conditional version checks.
- [ ] Complete authenticated list/item HTTP handlers and API contract tests.
- [ ] Add item update and reorder use cases.
- [ ] Add list detail query and cursor-based pagination.
- [ ] Add Angular list overview/editor and item interactions with visual regression.
- Evidence: authorization matrix, transaction tests, API contract tests, Playwright and visual regression.

### M4 – Households

- [ ] Household aggregate, membership model and owner/member policies.
- [ ] Create/list/get/rename household and household-owned lists.
- [ ] Leave, removal, owner transfer and bounded deletion strategy.
- Evidence: authorization matrix, transaction tests and end-to-end UI flow.

### M5 – Sharing

- [ ] Generate cryptographic invitation tokens and store token hashes only.
- [ ] Invite to personal lists/households and authenticated acceptance.
- [ ] Enforce atomic use count, revocation and expiry.
- [ ] Add stable HTTPS invitation URLs for web and future Android App Links.
- Evidence: token, race-condition, limit and E2E tests.

### M6 – Synchronization/PWA

- [ ] Version-based sync endpoint.
- [ ] Poll visible active list every 15 seconds; pause hidden tabs and refresh on focus/reconnect.
- [ ] Optimistic UI and 409 conflict handling.
- [ ] PWA installability and service worker.
- [ ] Define offline semantics; never silently queue conflicting writes.
- Evidence: multi-client synchronization and stale-write tests.

### M7 – Quality/release

- [ ] Rust fmt, Clippy and unit/integration tests.
- [ ] OpenAPI lint and compatibility check.
- [ ] Angular tests and Playwright E2E/visual tests.
- [ ] Authorization matrix, dependency and secret scans.
- [ ] Deployed smoke tests, backup/restore and incident documentation.
- [ ] Cost review for actual AWS account/region.

## Future Android (not v1)

- [ ] Validate demand and choose native stack (Kotlin + Jetpack Compose candidate).
- [ ] Cognito public-client flow with PKCE and secure token storage.
- [ ] Consume models from the shared OpenAPI contract.
- [ ] Android App Links and native sharing.
- [ ] Reuse backend authorization and domain rules.

## Definition of Done

- Acceptance criteria and objective evidence linked in the PR.
- CI green; no knowingly failing check is waived silently.
- OpenAPI validates and breaking changes are reviewed.
- Authorization tests cover relevant roles and unauthorized callers.
- Domain/application behavior tests do not require a live database.
- Adapter contract and concurrency tests cover concrete persistence guarantees.
- UI changes have reviewed visual regression results.
- No tokens, invitation secrets or hashes appear in logs.
- Terraform plan, deployment, smoke test and rollback are reviewed.
