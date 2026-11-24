# Einkaufio

Einkaufio is a collaborative shopping-list application. Users manage personal lists, share individual lists, and collaborate through households containing multiple lists.

The first client is an Angular PWA. The architecture deliberately keeps the option open for a native Android application: both clients use the same Cognito identity, platform-neutral REST API, domain rules and authorization model.

## Documentation

- [Architecture (arc42)](docs/architecture/arc42.md)
- [Architecture Decision Records](docs/architecture/decisions/README.md)
- [Data model and DynamoDB access patterns](docs/data-model.md)
- [OpenAPI 3.1 contract](contracts/openapi/openapi.yaml)
- [Implementation roadmap](docs/roadmap.md)
- [Security model](docs/security.md)
- [Quality engineering and implementation evidence](docs/quality-engineering.md)

## Architecture at a glance

- **Monorepo:** web client, backend, infrastructure, tests and documentation versioned together.
- Web: Angular PWA, static hosting on private S3 behind CloudFront.
- Future Android: native client (technology decision deferred; Kotlin/Jetpack Compose is the likely candidate).
- Backend: Rust + Axum, AWS Lambda custom runtime, ARM64.
- API: HTTPS Lambda Function URL initially.
- Identity: Amazon Cognito User Pool; accounts required.
- Domain: Domain-Driven Design (DDD) with bounded contexts and a hexagonal (ports-and-adapters) architecture.
- Persistence: DynamoDB adapter initially; domain/application layers must not depend on DynamoDB so another database can replace it.
- Synchronization: version-based polling, nominally every 15 seconds while a list is visible.
- Infrastructure: Terraform, including AWS resources; GitHub Actions with AWS OIDC.
- Frontend quality: Angular build, Playwright E2E and screenshot smoke capture are in CI; pixel-baseline regression is still pending.
- Delivery: architecture rules, tests, security checks and Terraform validation are intended to become required CI checks; branch protection must be configured separately.

## Guiding principles

1. Minimize idle cost and operational complexity.
2. Use DDD and hexagonal architecture; keep domain rules independent of frameworks and storage.
3. Keep business rules on the backend.
4. Keep API contracts client-independent.
5. Authorize every resource access server-side.
6. Use optimistic concurrency to prevent silent overwrites.
7. Verify every implementation step with objective evidence.
8. Add real-time infrastructure only when measured product needs justify it.
9. Provision and change AWS resources through reviewed Terraform plans, never undocumented console changes.

## Monorepo structure (target)

```text
.
├── apps/
│   └── web/                 # Angular PWA
├── backend/
│   ├── crates/
│   │   ├── domain/          # Pure domain model and invariants
│   │   ├── application/     # Use cases and owned ports
│   │   ├── adapters/        # DynamoDB and other outbound adapters
│   │   └── api/             # Axum/Lambda inbound adapter and composition root
│   └── Cargo.toml           # Rust workspace
├── infra/
│   └── terraform/
│       ├── bootstrap/       # Remote state bucket bootstrap
│       ├── modules/         # Reusable AWS building blocks
│       └── environments/   # dev/prod roots and environment values
├── contracts/
│   └── openapi/             # Platform-neutral API contract
├── tests/
│   ├── e2e/                 # Playwright end-to-end tests
│   ├── visual/              # Visual regression specs/baselines
│   └── architecture/        # Architecture/dependency rule tests
├── docs/
├── .github/workflows/       # CI, Terraform plan/apply and release workflows
└── README.md
```

The exact crate split may evolve with bounded contexts, but dependency direction is fixed. Keep a single repository and a coordinated PR for cross-component contract changes. Avoid introducing multiple independent package managers or build orchestrators without a demonstrated need.

## Developer commands

- make architecture: run architecture checker unit tests and inspect the actual Rust workspace dependency graph.
- make fmt, make lint, make test: run Rust formatting, Clippy and workspace tests.
- make terraform-fmt: check Terraform formatting.
- make check: run the checks currently implemented in this repository.
- In apps/web: npm install, npm run build, npm run e2e.

The web toolchain is Angular 22.3.0, TypeScript 6.0.0, Node.js 24.15.0 and Playwright 1.65.1. Direct dependency versions are exact. The npm lockfile still needs to be generated and committed before the frontend can be considered fully reproducible.

The commands require Rust stable with rustfmt/Clippy, Python 3.12+, Terraform 1.10.5+, and Node/npm for the web client.

## Status

The repository foundation includes the Rust workspace skeleton, architecture-boundary checks, Terraform root placeholders and CI. The Angular workspace, initial landing page, Playwright E2E tests and screenshot smoke capture are implemented in the current frontend work. A committed npm lockfile and reviewed screenshot baselines are still outstanding. Production Terraform resources, deployment workflows, GitHub branch protection and AWS deployment are not implemented yet. The structure above is a target architecture, not a claim that all components are already operational. Verify current AWS pricing, free-tier eligibility, quotas and regional availability against the target account before production deployment.
