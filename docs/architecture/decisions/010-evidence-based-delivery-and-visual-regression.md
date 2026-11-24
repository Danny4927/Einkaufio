# ADR-010: Evidence-based delivery and frontend visual regression

- Status: Accepted
- Date: 2026-11-24

## Context

Implementation claims must be trustworthy. A feature is not complete merely because code was written or a build succeeds. Angular layout regressions can also escape functional tests.

## Decision

Require acceptance criteria and a verification plan for every implementation work item. Each completed step must provide reviewable evidence appropriate to its scope.

Evidence includes automated test and CI results, API contract validation, security/authorization tests, visual screenshots/diffs, Terraform plan/apply output, deployed smoke tests, migration reconciliation and rollback evidence.

Use Playwright screenshot assertions for frontend visual regression tests. Cover critical Angular screens and states at agreed desktop/mobile viewports. Make screenshots deterministic with seeded data, pinned browser environment, stable fonts/time/locale and controlled animations.

Intentional visual baseline changes require human review and explanation. Unexpected diffs must be investigated, not blindly re-recorded. CI should retain actual and diff screenshots as artifacts on failure.

A work item is verified only when each acceptance criterion maps to evidence, required checks pass, and a reviewer can inspect or reproduce the result. If verification cannot be performed, mark the work partially verified or blocked; do not claim success.

## Consequences

- PRs carry more explicit evidence and may take longer to complete.
- Visual regressions are more likely to be detected before merge.
- CI must publish actionable reports and screenshot artifacts.
- Baseline maintenance becomes part of UI development.
- Passing tests support claims only for the behaviors/environments actually covered; they do not prove absence of all defects.

## Alternatives

- Relying on manual review alone was rejected because it is not repeatable.
- Treating successful compilation as completion was rejected because it does not verify runtime behavior or requirements.
