# ADR 011: Monorepo, Terraform IaC and enforceable architecture

- **Status:** Accepted
- **Date:** 2026-11-24

## Context

Einkaufio consists of a web client, a Rust backend, AWS infrastructure, API contracts, tests and architecture documentation. Keeping these in separate repositories would make coordinated contract and infrastructure changes harder to review. DDD and hexagonal architecture must be technically enforced rather than relying on folder naming or contributor discipline. AWS resources must be reproducible and auditable.

## Decision

1. Use one Git monorepo for first-party application code, infrastructure, contracts, tests, CI workflows and documentation.
2. Use Terraform as the sole source of truth for AWS infrastructure.
3. Keep Terraform bootstrap, reusable modules and isolated dev/prod environment roots under infra/terraform.
4. Use a protected S3 remote state backend with versioning, encryption, public access blocked and S3 native state locking. Bootstrap state separately and migrate it only after protections are verified.
5. Pin Terraform/provider versions and commit lock files.
6. Use GitHub Actions with AWS OIDC and least-privilege roles; never store long-lived AWS access keys.
7. Generate Terraform plans in pull requests. Production apply is a separate protected workflow with explicit environment approval and a fresh reviewed plan.
8. Use Rust workspace crate boundaries to enforce domain → application → ports/adapters direction. Domain and application cannot depend on AWS SDK, Axum, Lambda, DynamoDB or transport concerns.
9. Add automated dependency/architecture checks and prove their effectiveness with negative tests.
10. Enforce Angular module/library import boundaries with lint rules.
11. Require CI checks through GitHub branch protection/rulesets; merely defining a workflow is not considered enforcement.
12. Keep CI evidence (test reports, visual diffs, Terraform plan summaries, deployment and smoke-test references) linked to the PR.

## Target repository layout

```text
apps/web/
backend/Cargo.toml
backend/crates/domain/
backend/crates/application/
backend/crates/adapters/
backend/crates/api/
infra/terraform/bootstrap/
infra/terraform/modules/
infra/terraform/environments/dev/
infra/terraform/environments/prod/
contracts/openapi/
tests/e2e/
tests/visual/
tests/architecture/
.github/workflows/
docs/
```

## Consequences

### Positive
- Coordinated API/client/backend/IaC changes are atomic and reviewable.
- Architecture boundaries are checked by the compiler and CI.
- Infrastructure changes are reproducible, reviewed and auditable.
- No long-lived AWS credentials are needed in GitHub Actions.
- CI artifacts provide objective evidence for each change.

### Trade-offs
- CI orchestration and path filtering require care to avoid skipping affected checks.
- Terraform state bootstrap and IAM/OIDC setup require deliberate initial operations.
- Monorepo ownership and build boundaries must remain clear as the codebase grows.
- Production deployment requires approvals and a disciplined plan/apply process.

## Verification

- A clean checkout can build/test each workspace independently.
- An intentionally forbidden dependency causes architecture CI to fail.
- Terraform formatting, validation and static security checks pass.
- A PR produces a reviewable plan; a protected deployment can apply it using OIDC.
- GitHub branch protection marks all required checks as mandatory.
