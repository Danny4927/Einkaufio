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
- [x] Add local make verify plus focused targets; CI invokes the same Makefile targets (see CI workflow).
- [x] Add PR evidence/security guidance.
- [x] Register the Claude Code PreToolUse commit hook and add a deterministic hook test. A real Claude Code runtime commit test remains environment-specific.
- [ ] **Verify the hook in a real Claude Code runtime:** confirm it appears in `/hooks` and project settings in `/status`, then make a disposable test commit and capture evidence that `make check` ran before Git executed the commit. Do not treat the fixture test as a runtime integration test.
- [x] **Repository contract scenarios and reference adapter**
  - Shared list/item scenarios provision their own fixtures and run against the application-owned ports.
  - List coverage: owner scoping, duplicate create, unauthorized existence non-disclosure, stale no-op rename, stale delete, version increments, repeated delete, and item cascade on list deletion.
  - Item coverage: unauthorized actor, duplicate item ID, stale list version, stale item versions for check/delete, check and uncheck transitions, idempotent check, list/item version semantics, 100-active-item limit, repository-assigned unique positions, and repeated-delete `NotFound`.
  - Capacity and ordering are verified through the shared `list_items` port, not adapter-only inspection helpers.
  - Reuse: enable the adapters crate's `test-support` feature and invoke both contracts with a fresh `ShoppingListRepository`/`ShoppingItemRepository` pair.
  - Limitation: the in-memory adapter is single-threaded test support (`RefCell`, therefore `!Sync`); it does not prove durable storage or production concurrency behavior. DynamoDB must add transaction/conditional-write integration tests.

- [ ] **HTTP transport foundation (in progress in PR):** Axum router, public health endpoint, personal-list route DTOs and fail-closed authentication gate. The routes intentionally return `503 authentication_unavailable` until Cognito JWT verification can supply a trusted principal; no client-provided owner ID is accepted.
- [ ] **Verified actor at the authentication boundary:** introduce a type (e.g. `VerifiedActor`) that can only be created from successfully verified Cognito claims/principal data.
  - Acceptance: request DTOs cannot provide owner/actor identity; no public constructor/from-string conversion exists for untrusted input; application use cases receive the verified actor explicitly.
  - Tests: valid claims produce the actor; missing/invalid/expired/wrong-audience tokens fail before use-case invocation; client-supplied owner/actor fields are ignored or rejected.
  - Sequencing: implement alongside Cognito JWT verification in M2, not as an isolated string newtype before the trust boundary exists.
- [ ] **Dependency and test-strength tooling:** add a pinned `cargo-deny` policy first, then mutation testing with `cargo-mutants` scoped to domain/application, and finally a coverage threshold once a stable baseline exists.
  - Acceptance: CI fails on configured license/advisory/ban/source violations; mutation report identifies surviving mutants; coverage threshold is documented, measured and ratcheted rather than chosen arbitrarily.
  - Keep fast local targets available; expensive mutation runs may be a separate CI job or scheduled workflow.
- [ ] **GitHub merge protection:** configure a repository ruleset/branch protection after CI check names are stable.
  - Acceptance: PR required, direct pushes/force pushes blocked, architecture/backend/web/E2E/Terraform checks required, and production deployment protected by environment approval.
  - Note: this is repository configuration, not something a committed workflow file enforces by itself.
- [ ] **Agent review automation:** evaluate Claude Code GitHub Action only after checking its current official setup/security documentation.
  - Acceptance: PR review is advisory (not an auto-merge authority), uses least-privilege permissions, does not expose secrets, and its findings are visible as a PR check/comment.
  - Keep human review and required CI as merge gates; document cost, token permissions and failure behavior before enabling.

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

### M0a – Monorepo skeleton and developer experience

- [x] Establish the agreed top-level layout: `apps/web`, `backend`, `infra/terraform`, `contracts/openapi`, `tests`, `docs`, `.github/workflows`.
- [ ] Move the existing OpenAPI contract into `contracts/openapi` and update references (or keep a compatibility pointer during transition).
- [x] Establish Rust workspace crates for domain, application, adapters and API/composition root.
- [x] Add Angular app under `apps/web`; pin Node/package manager and commit lockfiles.
- [x] Add root-level developer commands/documentation that invoke each toolchain reproducibly.
- [ ] Add path-aware CI orchestration with safe dependency triggers.
- Evidence: clean checkout setup on CI, workspace builds, Angular build, lockfile consistency and documented tree.

### M0b – Architecture enforcement

- [ ] Add Rust dependency rules that reject forbidden crate edges beyond the current architecture checker.
- [ ] Add automated negative tests proving domain/application independence from AWS and HTTP frameworks (a temporary forbidden dependency/import must fail the relevant check).
- [x] Add a reusable adapter contract-test harness; execute against each adapter when available.
- [ ] Add Angular import-boundary lint rules.
- [ ] Add OpenAPI lint and breaking-change detection.
- [ ] Configure GitHub branch protection/ruleset so required checks actually block merge.
- Evidence: compliant architecture passes; controlled forbidden dependency/import is shown to fail and then removed.

### M0c – CI quality gates

- [ ] Rust fmt, Clippy, unit/integration tests.
- [ ] Angular lint, unit/component tests, Playwright E2E and visual regression.
- [ ] Security/dependency/secret scans.
- [ ] Terraform fmt, validate and static security scan.
- [ ] Publish test reports, visual diffs and Terraform plan summary as PR artifacts.
- Evidence: green required checks on a PR and inspected failure artifacts from a controlled failing test.

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

### M1.0 – Terraform AWS foundation and delivery controls

- [ ] Create a dedicated Terraform bootstrap root for remote state.
- [ ] Provision protected S3 state bucket (versioning, encryption, public access blocked, least-privilege IAM) and enable S3 native state locking.
- [ ] Migrate bootstrap state to remote backend and verify state recovery/versioning.
- [ ] Create separate dev and prod Terraform roots/state keys.
- [ ] Add reusable modules for Cognito, DynamoDB, Lambda/Function URL, frontend S3/CloudFront/OAC, IAM, logs/alarms and budgets.
- [ ] Add GitHub OIDC roles with least privilege; separate plan and apply permissions where practical.
- [ ] Add PR plan workflow and protected production apply workflow.
- [ ] Pin Terraform and provider versions; commit provider lock file.
- [ ] Add Terraform static security scanning (for example Checkov or Trivy config) and review all findings.
- Evidence: fmt/validate/security scans, reviewed plan, successful dev apply, post-apply smoke tests, state recovery test and resource/cost inventory.

### M1.1 – Application deployment pipeline

- [ ] Build Rust Lambda artifact and Angular assets from the monorepo.
- [ ] Deploy artifacts without manual console steps.
- [ ] Record commit SHA and artifact versions.
- [ ] Run deployed API and frontend smoke tests.
- [ ] Gate production promotion with environment approval and a fresh Terraform plan.
- Evidence: deployment run linked to commit, successful smoke tests and documented rollback.

### M2 – Authentication/profile

- [ ] Cognito register/sign-in/password reset.
- [ ] Rust JWT verification with JWKS caching, bounded refresh, single-flight refresh and key rotation.
- [ ] Derive principal from verified sub; protect personal-list routes with a verified actor.
- [ ] Add deterministic signed-token and middleware tests (valid, malformed, invalid signature/claims, key rotation, JWKS outage).
- [ ] Wire Cognito settings into the Lambda/runtime composition root and Terraform environment.
- [ ] Create/read/update profile via application use cases and profile repository port.
- [ ] Angular auth flow and protected routes.
- [ ] Tests for invalid, expired and wrong-use JWTs.
- Evidence: automated auth tests, API integration output, browser screenshots for auth states, deployed smoke test.

### M3 – Personal lists/items

#### Implemented domain and application slices

- [x] Personal-list domain types for list/user identifiers, validated list names and personal shopping lists.
- [x] New personal-list aggregate starts at version 1.
- [x] Create and list personal-list application use cases.
- [x] Actor-scoped rename and delete use cases with validated `AggregateVersion` value object.
- [x] Rename increments the aggregate version only when the name changes; version exhaustion is represented safely.
- [x] Repository errors explicitly represent version exhaustion; mutation use cases preserve repository failure semantics.
- [x] Fake-repository tests cover actor scoping, invalid inputs, successful version increments, stale no-op rename conflicts, repeated-delete NotFound semantics and repository failures.
- [x] Port contract documents atomic expected-version checks before no-op rename detection and NotFound for repeated deletion.
- [x] Item domain IDs and validated name, quantity, unit and note value objects.
- [x] New items start unchecked at item version 1.
- [x] Check/uncheck is idempotent and increments the item version only when the checked state changes.
- [x] Application use cases add, check/uncheck and delete items through application-owned ports; mutation use cases receive actor identity explicitly.
- [x] Add requires an expected list version; check and delete require an expected item version.
- [x] Adapter contract requires authorization before disclosing list/item existence.
- [x] Port contract specifies atomic parent-list version updates, the 100-active-item limit and unique position assignment; these guarantees are not yet implemented by a concrete persistence adapter.
- [x] Version semantics are deliberately distinct: item version tracks item changes; list version tracks list-wide changes. Check returns updated item + new list version; add/delete return the new list version.
- [x] Existing use-case tests exercise the fake repository behavior; the shared adapter contract suite is tracked under Agent and quality foundation (ADR 012).

#### Remaining before the capability is complete

- [ ] Authenticated HTTP handlers derive owner/actor ID exclusively from verified Cognito claims.
- [ ] Concrete ID generator and persistence adapter.
- [ ] Atomic persistence of list metadata and owner index/reference.
- [x] Shared repository contract scenarios run against the in-memory reference adapter; adapter-specific integration tests remain required for transaction/conditional-write behavior.
- [x] Item detail update use case: full replacement of name/quantity/unit/note via `ItemDetails`; stale item versions conflict before no-op detection; item and list versions advance only on real changes; check state and position are preserved; item version exhaustion fails with `VersionExhausted` without mutating the item (also for check/uncheck); the port default is fail-closed. Covered by domain, use-case and shared repository contract tests (in-memory reference adapter).
- [ ] Item reorder use case.
- [ ] List detail query and cursor-based pagination.
- [ ] Angular list overview/editor and item interaction flows with reviewed visual regression.
- [ ] API contract tests plus owner/member/unauthorized and concurrency integration tests.
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
