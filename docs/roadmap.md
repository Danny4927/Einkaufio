# Einkaufio – Implementation roadmap

This file is the single implementation-progress source of truth. Work is organised as vertical delivery phases (ADR 013) plus one continuous quality-gate track.

## Delivery rules

- Deliver vertical slices: each phase ends with a user-visible capability running in a deployed environment, not with a finished layer.
- Retire the largest technical unknowns (persistence transactions, Cognito in the PWA, Lambda/Function URL, CloudFront) before broadening feature scope.
- Use DDD and hexagonal architecture; keep DynamoDB-specific types and key design inside the DynamoDB adapter.
- Every work item has acceptance criteria and a verification plan before implementation (see `docs/quality-engineering.md`).
- Mark an item done only with objective evidence: CI output, screenshots/visual diffs for UI, Terraform plan/apply output and smoke-test results where relevant. Code inspection or an unverified claim is not evidence.
- Frontend UI changes require visual regression coverage or a documented reason why the change cannot be meaningfully screenshot-tested.
- Create one GitHub issue per independently deliverable work item and link it next to the item here when it is scheduled.
- Keep this file in sync with the code in the same PR that changes implementation state. Do not create parallel progress trackers.

## Current state (2026-11-27)

- **Done:** monorepo layout, Rust workspace crates, architecture checker with negative fixture tests, CI (backend fmt/Clippy/tests/architecture, Angular build + Playwright E2E/screenshot capture, Terraform fmt/validate, yamllint + Redocly OpenAPI lint, agent-config check), personal-list and item domain/application use cases, shared repository contract suite against an in-memory reference adapter, Axum router with fail-closed auth gate, Cognito access-token verification with JWKS caching and a `VerifiedActor` that can only be created from verified claims, asynchronous `Send + Sync` repository ports (ADR 014), list access model (ADR 015) with access-scoped list/item ports and shared-access contract scenarios.
- **Not started:** DynamoDB adapter, Lambda entry point/binary, any provisioned AWS resource (Terraform roots contain only version pins), deployment pipeline, Angular feature code, authentication UI.
- **Implication:** no request has yet travelled browser → API → database. Phase A closes that gap before more domain features are added.

## v1 product scope

- Required accounts and sign-in.
- Personal shopping lists with item create/update/delete/check/reorder.
- Collaboration: households with owner/member roles and multiple household-owned lists, joined via secure invitation links.
- Version-based polling (15 seconds while the list view is visible).
- Responsive, installable Angular PWA.
- Platform-neutral API that supports a future Android client.
- Backend persistence isolated behind ports so DynamoDB can be replaced.
- Privacy basics required for an EU deployment (account deletion, data export, privacy notice).

**Scope decision (ADR 015):** v1 collaboration is household-based. Explicit sharing of individual personal lists and personal-list invitations are deferred to v1.x.

## Initial product limits (configurable)

- 10 members per household.
- 20 lists per household.
- 100 active items per list.
- 5 simultaneously valid invitations per resource.
- Invitation lifetime default 7 days, maximum 30 days.
- Invitation maxUses default 5, maximum 10.

These are initial product/abuse-control limits, not permanent technical constraints.

## Phase A – Walking skeleton (dev)

Goal: the thinnest end-to-end path, deployed to dev without manual console steps.

**Exit criterion:** a user signs in to the deployed dev PWA, creates a personal list and sees it after reload; an automated smoke test against dev proves the same path and runs in the deploy workflow.

- [ ] **A1 Terraform state and dev root:** bootstrap root for a protected S3 state bucket (versioning, encryption, public access blocked, S3 native locking); migrate bootstrap state; dev root with its own state key. Pin Terraform/provider versions and commit the provider lock file. *Prepared (code, lock file, runbook `docs/runbooks/terraform-bootstrap.md`); not applied, state not migrated.*
- [ ] **A2 Minimal dev infrastructure:** modules for DynamoDB PK/SK table, Cognito User Pool + public app client, ARM64 Lambda custom runtime + Function URL, private S3 + CloudFront/OAC, least-privilege Lambda IAM, log retention. Set reserved Lambda concurrency from the start as the first cost/abuse guard. *Prepared as Terraform modules + dev root (validated, scanned); not applied.*
- [ ] **A3 GitHub OIDC and deploy workflow:** OIDC role(s) with least privilege (separate plan/apply where practical), PR plan workflow, dev apply, build of Rust Lambda artifact and Angular assets, commit SHA recorded on the deployment.
- [ ] **A4 Lambda entry point and composition root:** Lambda binary wiring `ApiState::from_cognito_env`, the DynamoDB adapter and an ID generator; abort startup on configuration errors; exact-origin CORS for the CloudFront domain.
- [ ] **A5 DynamoDB adapter for personal lists:** implements the list port (create with owner index, list accessible lists, rename, delete with item cascade) with access resolution and atomic enforcement per ADR 015, using conditional writes/transactions; passes the shared list contract suite against DynamoDB Local or an equivalent in CI, plus adapter-specific transaction/conditional-write tests.
- [ ] **A6 HTTP routes for personal lists:** create/list routes complete; add rename/delete routes with expected-version handling and 401/403/404/409 contract tests aligned with OpenAPI.
- [ ] **A7 Angular sign-in and list overview:** Cognito hosted UI or SDK flow (Authorization Code + PKCE), token handling per `docs/security.md`, protected routes, list overview with create; Playwright E2E with reviewed visual baselines for signed-out, empty and populated states.
- [ ] **A8 Deployed smoke test:** automated smoke test against dev (health, authenticated create/list) run by the deploy workflow; documented rollback.
- Evidence: Terraform plan/apply output, deploy run linked to commit, smoke-test output, contract-test report for the DynamoDB adapter, Playwright report and screenshots.

## Phase B – Single-user shopping

Goal: one user can do a complete shop with the list open on two devices.

**Exit criterion:** deployed dev E2E test edits a list on two browser contexts; changes appear on the other within one polling interval; a stale write yields a visible 409 conflict instead of silent overwrite.

- [ ] **B1 DynamoDB adapter for items:** implements the item port (add, update details, check/uncheck, delete) with atomic parent-list version updates, 100-active-item limit and unique positions; passes the shared item contract suite plus concurrency tests.
- [ ] **B2 Item reorder use case:** domain/application/contract tests, then adapter and route. *Domain (`ItemOrder`, `ShoppingItem::move_to`), `ReorderShoppingItems` use case, `reorder_items` port, in-memory adapter and `run_item_reorder_contract` done; DynamoDB adapter and HTTP route pending.*
- [ ] **B3 List detail query and cursor-based pagination** for lists and items. *`PageSize` (1–100, default 50)/opaque `PageCursor`, `PageAccessibleLists`/`PageShoppingItems` use cases, `list_accessible_page`/`list_items_page` ports, in-memory adapter and `run_pagination_contract` done; DynamoDB adapter, list detail and HTTP routes pending.*
- [ ] **B4 Item HTTP routes** with authorization and concurrency contract tests.
- [ ] **B5 Version-based sync endpoint** (`/api/lists/{listId}/sync`). *`ListSync` (snapshot or changes since version N with tombstones), `SyncShoppingList` use case, `sync_list` port, in-memory adapter and `run_sync_contract`/`run_shared_query_contract` done; DynamoDB adapter, OpenAPI and HTTP route pending.*
- [ ] **B6 Angular list editor:** item interactions, optimistic UI with 409 conflict handling, poll only the visible active list every 15 seconds, pause in background tabs, refresh on focus/reconnect; visual baselines for open/completed/empty/error states and mobile/desktop viewports.
- [ ] **B7 Profile:** `/api/me` create/read/update via a profile use case and repository port. *`UserProfile`/`DisplayName` domain, `CreateProfile`/`GetProfile`/`UpdateProfile`, `ProfileRepository` port, in-memory adapter and `run_profile_contract` done; DynamoDB adapter, OpenAPI and HTTP route pending.*
- [ ] **B8 Abuse controls:** request-size and input limits, application-level throttling per principal; CloudWatch alarms for Lambda errors/throttles and DynamoDB throttling; AWS Budget notification. *Alarms/budget prepared in `modules/monitoring` (not applied); request limits and per-principal throttling still open.*
- Evidence: adapter contract and concurrency test reports, API contract tests, multi-client Playwright test, browser visibility test, visual regression report, alarm/budget configuration in Terraform plan.

## Phase C – Access model and collaboration

Goal: two users collaborate on household lists through an invitation link.

**Exit criterion:** user A creates a household and invitation, user B accepts it in another browser and both edit the same household list; the authorization matrix (owner/member/non-member/unauthenticated) passes for every endpoint.

- [x] **C1 Access-model ADR (before any household code):** ADR 015 decides access through personal ownership or household membership, defers explicit list sharing to v1.x, defines the Administrator/Contributor rule table and sketches DynamoDB access patterns. List/item ports take a `ListAccessRequest` (principal + required permission); `list_personal_for_owner` became `list_accessible`; contract scenarios include `run_shared_access_contract`. Evidence: `make test-workspace` and `make check` output in the implementing PR.
- [ ] **C2 Households:** create/list/get/rename, membership model, household-owned lists, owner/member policies, per-user household limit; add the household-membership path to adapter access resolution (ADR 015) and pass `run_shared_access_contract` via real household membership in every adapter.
- [ ] **C3 Membership lifecycle:** leave household, owner transfer before owner departure, bounded deletion/cascade strategy with tests.
- [ ] **C4 Invitations:** ≥256-bit random tokens, store hash only, atomic use count/revocation/expiry, authenticated acceptance, limits from "Initial product limits", stable HTTPS URLs compatible with Android App Links, web route with fallback.
- [ ] **C5 Explicit list sharing — deferred to v1.x (ADR 015).** Not part of v1; adds a Contributor access source via explicit list membership and personal-list invitations.
- [ ] **C6 Angular household and invitation flows** with visual baselines.
- Evidence: authorization matrix report, transaction and race-condition tests (invitation use count), token security tests, invitation E2E flow, screenshot report.

## Phase D – Release readiness (prod)

Goal: v1 is live in production with recovery, privacy and cost under control.

**Exit criterion:** production deployed through the protected pipeline, smoke tests green, restore test performed, privacy requirements met, cost review signed off.

- [ ] **D1 PWA:** installability, service worker, defined offline semantics (cached reads; no silent queuing of conflicting writes).
- [ ] **D2 Privacy (GDPR):** account deletion including memberships/invitations, data export, privacy notice and imprint, documented data region and retention.
- [ ] **D3 Prod environment:** prod Terraform root/state key, protected production apply with environment approval and a fresh plan, production promotion of the same artifacts as dev.
- [ ] **D4 Backup and recovery:** PITR/backup decision, documented and executed restore test, incident runbook, Terraform state recovery test.
- [ ] **D5 Cost and quota review** for the actual AWS account/region, including polling read volume measured in dev.
- [ ] **D6 Accessibility and localisation check** of critical screens (keyboard use, contrast, UI language).
- Evidence: production deploy run, smoke-test report, restore evidence, cost review, privacy checklist, accessibility check results.

## Quality-gate track (continuous)

Items with a trigger are scheduled when the trigger is met, not before; they must not delay Phase A.

### Done

- [x] Root agent instructions, PR evidence/security template, Claude Code PreToolUse commit hook with deterministic fixture test (ADR 012).
- [x] `make check` / `make verify`; CI invokes the same Makefile targets.
- [x] CI: Rust fmt, Clippy (`-D warnings`), workspace tests, architecture checks.
- [x] CI: Angular production build, Playwright E2E and screenshot smoke capture with uploaded artifacts.
- [x] CI: Terraform fmt and validate for all roots.
- [x] CI: strict yamllint and Redocly OpenAPI lint (`make ci-contracts`); OpenAPI contract lives in `contracts/openapi`.
- [x] Architecture checker rejects forbidden crate edges, unregistered or missing crates; negative fixture tests prove the checker fails on violations.
- [x] Reusable repository contract suite (see "Completed foundations").

### Open

- [ ] **Pixel-baseline visual regression** (`toHaveScreenshot`) with reviewed baselines — *trigger:* first Angular feature screen (A7).
- [ ] **Angular lint, unit tests in CI and import-boundary rules** — *trigger:* first Angular feature module (A7).
- [ ] **Terraform static security scan** (Checkov or Trivy config) with reviewed findings *(prepared: `make terraform-scan`, Checkov, findings in `docs/terraform-scan.md`; not yet wired into CI)* — *trigger:* first real AWS resource (A2).
- [ ] **Publish Terraform plan summary as PR artifact** — *trigger:* A3.
- [ ] **Secret scanning** in CI — *trigger:* A3 (first workflow handling cloud credentials).
- [ ] **`cargo-deny`** policy (licenses, advisories, bans, sources), pinned — *trigger:* A4/A5 introduce AWS SDK dependencies.
- [ ] **OpenAPI breaking-change detection** against the default branch — *trigger:* first deployed API (end of Phase A).
- [ ] **GitHub ruleset/branch protection:** PR required, no direct/force pushes, required checks for architecture/backend/web/E2E/Terraform/contracts, protected production environment — *trigger:* CI check names stable after A3; must be in place before D3.
- [ ] **Path-aware CI orchestration** with safe dependency triggers — *trigger:* CI duration becomes a bottleneck.
- [ ] **Mutation testing** (`cargo-mutants`, domain/application, separate or scheduled job) — *trigger:* after Phase B.
- [ ] **Coverage threshold**, measured and ratcheted from a stable baseline — *trigger:* after mutation testing baseline.
- [ ] **Agent review automation** (Claude Code GitHub Action): advisory only, least privilege, no secret exposure, findings visible on the PR; document cost and failure behaviour first — *trigger:* optional, after branch protection.
- [ ] **Hook runtime verification:** confirm the hook in `/hooks` and `/status` of a real Claude Code runtime with a disposable commit — low priority, environment-specific.
- [ ] **Repository hygiene:** license, contribution guidelines, local development and environment configuration docs — *trigger:* before the repository is opened to other contributors, at the latest D3.

## Completed foundations (reference)

Detail of finished work that later phases rely on.

### Personal lists

- Domain types for list/user identifiers, validated list names and personal shopping lists; new lists start at version 1.
- Create (personal) plus access-scoped list, rename and delete use cases (`ListAccessibleLists`, `RenameShoppingList`, `DeleteShoppingList`) with validated `AggregateVersion`; rename/delete require Administer permission (ADR 015).
- Rename increments the version only on a real name change; stale expected versions conflict before no-op detection; version exhaustion is an explicit error; repeated delete returns `NotFound`.

### Items

- Item IDs and validated name, quantity, unit and note value objects; new items start unchecked at item version 1.
- Add (requires expected list version), update details (full replacement via `ItemDetails`), check/uncheck (idempotent) and delete (require expected item version).
- Item version tracks item changes; list version tracks list-wide changes. Check returns the updated item plus the new list version; add/delete return the new list version. Item version exhaustion fails with `VersionExhausted` without mutation; port defaults are fail-closed.

### Repository contract suite

- Shared scenarios provision their own fixtures and run against the application-owned ports; enable the adapters crate's `test-support` feature and invoke the list, item and shared-access contracts with fresh `ShoppingListRepository`/`ShoppingItemRepository` instances (plus a `SharedListFixture` for shared access).
- Lists: access-based listing (all owned lists, never foreign ones), duplicate create, non-disclosure to unauthorized actors, stale no-op rename, stale delete, version increments, repeated delete, item cascade on list deletion.
- Items: unauthorized actor, duplicate item ID, stale list/item versions, check/uncheck transitions, idempotent check, version semantics, 100-active-item limit, repository-assigned unique positions, repeated-delete `NotFound`. Capacity and ordering are verified through `list_items`, not adapter-only helpers.
- Shared access (ADR 015): contributors use list content but get `Forbidden` for rename/delete (also with stale versions), outsiders get `NotFound` everywhere, the requested permission is enforced generically and revocation applies on the next request. Lists are provisioned through the `SharedListFixture` test-support trait.
- Scenarios are async (ADR 014). The in-memory adapter serializes every operation behind one `Mutex` (`Send + Sync`); a multi-threaded test proves that parallel writers with the same expected version commit exactly once. It proves neither durability nor database transaction behaviour; the DynamoDB adapter adds transaction/conditional-write integration tests (A5, B1).

### HTTP and authentication

- Axum router with public `/health` and personal-list create/list routes; protected routes return `503 authentication_unavailable` when no verifier is configured.
- Cognito access-token verification: JWKS caching, bounded and single-flight refresh, key rotation, refresh throttling, documented clock skew; deterministic tests for valid, bad-signature, invalid-claims, rotated-key, unknown-key, JWKS outage and concurrent cold-cache cases.
- `VerifiedActor` is crate-private and created only from verified claims; request DTOs cannot supply owner/actor identity.

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

Per work item:

- Acceptance criteria are met and evidence is linked in the PR.
- CI checks pass; no knowingly failing required check is waived silently.
- OpenAPI schema validates; breaking changes are reviewed.
- New endpoints have authorization tests for administrator/contributor/no access/unauthenticated (ADR 015).
- Domain/application behavior is tested without a live database; persistence adapter contract tests pass.
- Multi-record writes are atomic or have documented consistency semantics.
- UI changes have reviewed visual regression results; baseline changes are intentional and approved.
- No tokens/invite secrets appear in logs.
- Terraform plans are reviewed and cost-affecting resources understood.
- This roadmap reflects the new implementation state.

Per phase (additionally):

- The phase exit criterion is demonstrated in a deployed environment with linked evidence.
- Deployment, smoke test and rollback are documented.

Before production (Phase D): AWS pricing, free-tier eligibility, quotas and regional availability are verified for the target account.
