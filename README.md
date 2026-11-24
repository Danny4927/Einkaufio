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
- Frontend quality: automated component/E2E tests plus visual regression tests.
- Delivery: architecture rules, tests, security checks and Terraform validation are enforced by required CI checks.

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

## Status

This repository contains the initial product and architecture baseline. The structure above is the target layout; it is not a claim that all application code or workflows already exist. Verify current AWS pricing, free-tier eligibility, quotas and regional availability against the target account before production deployment.
