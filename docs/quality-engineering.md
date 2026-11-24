# Einkaufio – Quality engineering and implementation evidence

## Purpose

Every implementation step must be proven correct against explicit acceptance criteria. A commit, code change, successful build or PR description alone is not sufficient evidence.

The required evidence depends on the change, but must be objective, repeatable and reviewable by another developer.

## 1. Work-item contract

Before implementation, every issue or PR-sized work item must define:

1. **Scope:** what is changing and what is explicitly out of scope.
2. **Acceptance criteria:** observable behavior, not implementation intentions.
3. **Verification plan:** exact tests, checks, screenshots or deployment checks that will prove the criteria.
4. **Risks:** relevant security, data migration, compatibility, cost and accessibility risks.
5. **Evidence location:** CI run, report, screenshot artifact, Terraform plan, smoke-test output or other durable link.

Do not start a multi-step implementation with vague criteria such as "make sharing work".

## 2. Evidence required by change type

| Change type | Minimum evidence |
|---|---|
| Domain rule | Unit tests for normal, boundary and invalid cases |
| Application use case | Tests with fake ports, including failure and authorization paths |
| Persistence adapter | Adapter contract/integration tests; transaction and conditional-write cases |
| HTTP/API | OpenAPI validation, request/response tests, status/error contract tests |
| Authorization/security | Positive and negative permission matrix tests; token and invitation abuse cases |
| Angular behavior | Component/unit tests and Playwright E2E for critical user flow |
| Angular visual change | Visual regression report with reviewed screenshots/diffs |
| Infrastructure | Terraform fmt/validate/plan; apply output in target environment when deployment is in scope |
| Deployment | Deployed smoke tests and relevant logs/metrics |
| Data migration | Migration test, before/after reconciliation and rollback/restore plan |
| Documentation/ADR | Link check or format/lint check where available; review against implemented behavior |

## 3. Frontend visual regression tests

Visual regression tests are mandatory for critical Angular screens and states.

### Tooling

Use Playwright screenshot assertions initially (for example `toHaveScreenshot`). Store approved baselines in version control or another controlled artifact store.

### Coverage

At minimum, establish baselines for:
- Sign-in and registration states.
- Main dashboard with personal lists and households.
- Shopping list with open and completed items.
- Empty states.
- Loading and error states.
- Invitation acceptance.
- Responsive layouts at agreed desktop and mobile viewport sizes.

Add states as features are delivered. Focus first on stable, high-value user journeys rather than attempting to screenshot every possible permutation.

### Determinism

- Use seeded test data and stable test accounts.
- Fix viewport, device scale factor, locale, timezone and color scheme.
- Wait for fonts and relevant UI data before capture.
- Disable or stabilize animations, transitions, blinking cursors and dynamic timestamps.
- Mask only genuinely nondeterministic regions; do not mask away the UI being tested.
- Run in a pinned browser/container environment to reduce rendering noise.

### Baseline policy

- A changed screenshot is not automatically accepted.
- Every intentional baseline update must be reviewed in the PR with before/after images and a reason.
- Unexpected diffs must be investigated and fixed, not blindly re-recorded.
- CI must upload actual and diff screenshots as artifacts when a test fails.
- Visual tests should be repeatable locally and in CI.

## 4. Backend architecture verification

DDD and hexagonal architecture must be enforced, not just described.

- Domain crate/module has no dependency on Axum, Lambda, AWS SDK, DynamoDB or transport DTOs.
- Application use cases depend on domain types and application-owned ports.
- Infrastructure adapters implement ports.
- Inbound adapters call application use cases; they do not contain business rules.
- Use architecture/dependency checks (for example Rust workspace crate boundaries and `cargo deny`/dependency analysis where appropriate).
- Unit tests for domain/application must run without AWS credentials or a live database.
- DynamoDB adapter contract tests verify behavior expected by the ports.

A future persistence adapter should be able to implement the same application ports. The migration itself must include data conversion, reconciliation, cutover and rollback planning.

## 5. Pull request evidence checklist

Every implementation PR must include:

- [ ] Acceptance criteria listed and individually addressed.
- [ ] Test commands/checks run and their results linked.
- [ ] Required CI workflow is green.
- [ ] Security/authorization tests added where relevant.
- [ ] OpenAPI updated and validated for API changes.
- [ ] Visual regression screenshots/diffs attached for UI changes.
- [ ] Terraform plan/apply and deployed smoke-test evidence for infrastructure changes.
- [ ] Data migration/rollback evidence for schema or persistence changes.
- [ ] Documentation/ADR updated where architecture changes.
- [ ] Known limitations and skipped checks explicitly stated.

Do not claim a check passed unless its result was observed. If a check cannot run, mark the work as blocked or partially verified and explain why.

## 6. Definition of verified

A work item is **verified** only when:
- Every acceptance criterion has corresponding evidence.
- Required tests/checks pass in the target CI environment.
- Relevant failure paths are covered.
- UI visual differences are reviewed and intentional.
- Deployment-sensitive changes pass the relevant deployed smoke tests.
- A reviewer can reproduce or inspect the evidence.

Passing tests do not prove absence of all defects. The claim is limited to the behaviors and environments covered by the evidence.
