# Einkaufio – Implementation roadmap

## Delivery rules

- Use DDD and hexagonal architecture from the first backend implementation.
- Keep DynamoDB-specific types and key design inside the DynamoDB adapter.
- Every work item must have acceptance criteria and a verification plan before implementation.
- Every completed step must attach objective evidence: passing test/CI output, screenshots and visual diffs for UI changes, infrastructure plan/deployment output where relevant, and smoke-test results.
- Do not mark work complete based on code inspection or an unverified claim alone.
- Frontend UI changes require visual regression coverage or a documented reason why the change cannot be meaningfully screenshot-tested.

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

### Initial product limits (configurable)
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
- [ ] Define Rust workspace and Angular app structure.
- [ ] Define bounded contexts, dependency rules and backend ports/adapters.
- [ ] Add architecture boundary checks (for example cargo dependency checks).
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
- [ ] Angular personal-list overview and editor.
- [ ] Owner/member/unauthorized and concurrency tests.
- Evidence: domain/application tests with fake ports, adapter integration tests, API contract tests, Playwright E2E and visual regression report.

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


## Monorepo and enforcement implementation sequence

The following foundation work is a prerequisite to feature implementation. Keep it in the same repository and deliver it through small reviewable PRs.

### M0a – Monorepo skeleton and developer experience
- [ ] Create the agreed top-level layout: apps/web, backend, infra/terraform, contracts/openapi, tests, docs, .github/workflows.
- [ ] Move the existing OpenAPI contract into contracts/openapi and update references (or keep a compatibility pointer during transition).
- [ ] Establish Rust workspace crates for domain, application, adapters and API/composition root.
- [ ] Add Angular app under apps/web; pin Node/package manager and commit lockfiles.
- [ ] Add root-level developer commands/documentation that invoke each toolchain reproducibly.
- [ ] Add path-aware CI orchestration with safe dependency triggers.
- Evidence: clean checkout setup on CI, workspace builds, Angular build, lockfile consistency and documented tree.

### M0b – Architecture enforcement
- [ ] Add Rust dependency rules that reject forbidden crate edges.
- [ ] Add automated tests for domain/application independence from AWS and HTTP frameworks.
- [ ] Add adapter contract-test harness, initially implemented for DynamoDB.
- [ ] Add Angular import-boundary lint rules.
- [ ] Add OpenAPI lint and breaking-change detection.
- [ ] Configure GitHub branch protection/ruleset so required checks actually block merge.
- Evidence: CI passes on compliant architecture; a temporary forbidden dependency/import is shown to fail the relevant check (negative test), then removed.

### M0c – CI quality gates
- [ ] Rust fmt, Clippy, unit/integration tests.
- [ ] Angular lint, unit/component tests, Playwright E2E and visual regression.
- [ ] Security/dependency/secret scans.
- [ ] Terraform fmt, validate and static security scan.
- [ ] Publish test reports, visual diffs and Terraform plan summary as PR artifacts.
- Evidence: green required checks on a PR and inspected failure artifacts from a controlled failing test.

### M1 – Terraform AWS foundation
- [ ] Create a dedicated Terraform bootstrap root for remote state.
- [ ] Provision protected S3 state bucket (versioning, encryption, public access blocked, least-privilege IAM) and enable S3 native state locking.
- [ ] Migrate bootstrap state to remote backend and verify state recovery/versioning.
- [ ] Create separate dev and prod Terraform roots/state keys.
- [ ] Add reusable modules for Cognito, DynamoDB, Lambda/Function URL, frontend S3/CloudFront/OAC, IAM, logs/alarms and budgets.
- [ ] Add GitHub OIDC roles with least privilege; separate plan and apply permissions where practical.
- [ ] Add PR plan workflow and protected production apply workflow.
- [ ] Pin Terraform and provider versions; commit provider lock file.
- Evidence: fmt/validate/security scans, reviewed plan, successful dev apply, post-apply smoke tests, state recovery test and resource/cost inventory.

### M1.1 – Application deployment pipeline
- [ ] Build Rust Lambda artifact and Angular assets from the monorepo.
- [ ] Deploy artifacts without manual console steps.
- [ ] Record commit SHA and artifact versions.
- [ ] Run deployed API and frontend smoke tests.
- [ ] Gate production promotion with environment approval and a fresh Terraform plan.
- Evidence: deployment run linked to commit, successful smoke tests and documented rollback.
