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
- [x] Add local make verify covering the current CI validation jobs, plus focused backend targets.
- [x] Add PR evidence/security checklist.
- [x] Add opt-in Claude Code permission and commit-hook guardrail examples.
- [ ] Add and enforce repository Contract-Test suites for list and item repository ports with shared scenarios run against the fake and every production adapter.
- [ ] Introduce a claims-derived verified actor at the API authentication boundary; prohibit construction from client payloads.
- [ ] Add dependency policy and security/dependency scanners (cargo-deny, mutation testing and coverage threshold) after selecting and pinning their policy/configuration.
- [ ] Configure GitHub ruleset/branch protection with stable CI job names as required checks.

## v1 product scope

- Required accounts and sign-in.
- Personal shopping lists with explicit sharing.
- Households with owner/member roles and multiple household-owned lists.
- Item create/update/delete/check/reorder.
- Secure invitation links.
- Version-based polling (15 seconds while list view is visible).
- Responsive Angular PWA.
- Platform-neutral API that supports a future Android client.
- Backend persistence isolated behind ports so DynamoDB can be replaced.

## Initial product limits (configurable)

- 10 members per household.
- 20 lists per household.
- 100 active items per list.
- 5 simultaneously valid invitations per resource.
- Invitation lifetime default 7 days, maximum 30 days.
- Invitation maxUses default 5, maximum 10.

These are initial product/abuse-control limits, not permanent technical constraints.

## Milestones and GitHub-ready work packages

### M0 – Repository foundation
- [ ] Add license and contribution guidelines.
- [ ] Add ADR and OpenAPI validation to CI.
- [x] Define Rust workspace and Angular app structure.
- [x] Define bounded contexts, dependency rules and backend ports/adapters.
- [x] Add architecture boundary checks.
- [ ] Add local development and environment configuration docs.
- Evidence: clean CI run, architecture check output, documented project tree and reproducible local setup.

### M1 – AWS infrastructure
- [ ] Bootstrap Terraform state S3 bucket separately.
- [ ] Create private S3 frontend bucket.
- [ ] Create CloudFront distribution and OAC.
- [ ] Create ARM64 Lambda custom runtime and Function URL.
- [ ] Create DynamoDB PK/SK table.
- [ ] Create Cognito User Pool and public app client.
- [ ] Define least-privilege Lambda IAM.
- [ ] Configure CloudWatch log retention and alarms.
- [ ] Configure AWS Budget notifications.
- [ ] Configure GitHub OIDC deployment role.
- Evidence: reviewed Terraform plan, successful apply in target environment, deployed endpoint smoke test, least-privilege review and cost/resource inventory.

### M2 – Authentication/profile
- [ ] Cognito register/sign-in/password reset.
- [ ] Rust JWT verification with JWKS caching and key rotation.
- [ ] Derive principal from verified sub.
- [ ] Create/read/update profile via application use cases and profile repository port.
- [ ] Angular auth flow and protected routes.
- [ ] Tests for invalid, expired and wrong-use JWTs.
- Evidence: automated auth tests, API integration output, browser screenshots for auth states, deployed smoke test.

### M3 – Personal lists/items
- [ ] Create/list/get/rename/delete personal lists.
- [ ] Create/update/delete/check/reorder items.
- [ ] Item and list version transactions behind persistence ports.
- [ ] DynamoDB adapter contract tests.
- [ ] Pagination and input validation.
- [ ] Angular list overview and editor.
- [ ] Owner/member/unauthorized and concurrency tests.
- Evidence: authorization matrix test report, transaction tests, API contract tests, Playwright E2E and visual regression report.

### M4 – Households
- [ ] Create/list/get/rename household.
- [ ] Household membership model.
- [ ] Create/list household-owned lists.
- [ ] Owner/member policies.
- [ ] Leave household and owner transfer.
- [ ] Define bounded deletion/cascade strategy and test it.
- Evidence: authorization matrix test report, transaction tests, UI screenshots/diffs and end-to-end household flow.

### M5 – Sharing
- [ ] Cryptographic invitation tokens.
- [ ] Store token hash only.
- [ ] Invite to personal list and household.
- [ ] Authenticated acceptance.
- [ ] Atomic use count, revocation and expiry.
- [ ] Web invitation route and fallback.
- [ ] Stable HTTPS URLs compatible with future Android App Links.
- Evidence: token security tests, race-condition/limit tests, invitation E2E flow and screenshot report.

### M6 – Synchronization/PWA
- [ ] Version-based sync endpoint.
- [ ] Poll only visible active list at 15 seconds.
- [ ] Pause background tab; refresh on focus/reconnect.
- [ ] Optimistic UI and 409 conflict handling.
- [ ] PWA installability and service worker.
- [ ] Define offline semantics; do not silently queue conflicting writes.
- Evidence: multi-client synchronization test, stale-write conflict test, browser visibility test and visual regression report.

### M7 – Quality/release
- [ ] Rust fmt, Clippy and unit/integration tests.
- [ ] OpenAPI lint and compatibility check.
- [ ] Angular lint and tests.
- [ ] Playwright E2E tests.
- [ ] Playwright visual regression tests for critical viewports/states.
- [ ] Authorization matrix tests.
- [ ] Dependency and secret scanning.
- [ ] Deployed smoke tests.
- [ ] Backup/restore and incident documentation.
- [ ] Cost review for actual AWS account/region.
- Evidence: green required CI checks, published test reports, approved visual diffs, deployed smoke-test report, backup/restore evidence and cost review.

## Future Android milestone (not v1)

- [ ] Validate product demand and select native stack (Kotlin + Jetpack Compose candidate).
- [ ] Cognito public-client flow with PKCE.
- [ ] Generate/consume API models from the same OpenAPI contract.
- [ ] Keystore-backed secure token storage.
- [ ] Android App Links for invitation URLs.
- [ ] Native navigation and Android share sheet.
- [ ] Reuse same backend authorization and domain rules.
- [ ] Android UI and integration tests.
- [ ] Decide push notifications only if polling is insufficient.
- Evidence: API compatibility tests, auth security review, Android UI/integration test reports and invitation App Links test on device/emulator.

## Definition of Done

- Acceptance criteria are met and evidence is linked in the PR.
- CI checks pass; no knowingly failing required check is waived silently.
- OpenAPI schema validates and breaking changes are reviewed.
- New endpoints have authorization tests for owner/member/non-member.
- Domain/application behavior is tested without a live database.
- Persistence adapter contract tests pass.
- Multi-record writes are atomic or have documented consistency semantics.
- UI changes have reviewed visual regression results; baseline changes are intentional and approved.
- No tokens/invite secrets appear in logs.
- Terraform plan is reviewed and cost-affecting resources understood.
- Deployment, smoke test and rollback are documented.
- AWS pricing, free-tier eligibility and quotas are verified for target account.
