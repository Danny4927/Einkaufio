# Einkaufio

Einkaufio is a collaborative shopping-list application. Users manage personal lists, share individual lists, and collaborate through households containing multiple lists.

The first client is an Angular PWA. The architecture deliberately keeps the option open for a native Android application: both clients use the same Cognito identity, platform-neutral REST API, domain rules and authorization model.

## Documentation

- [Architecture (arc42)](docs/architecture/arc42.md)
- [Architecture Decision Records](docs/architecture/decisions/README.md)
- [Data model and DynamoDB access patterns](docs/data-model.md)
- [OpenAPI 3.1 contract](docs/api/openapi.yaml)
- [Implementation roadmap](docs/roadmap.md)
- [Security model](docs/security.md)
- [Quality engineering and implementation evidence](docs/quality-engineering.md)

## Architecture at a glance

- Web: Angular PWA, static hosting on private S3 behind CloudFront.
- Future Android: native client (technology decision deferred; Kotlin/Jetpack Compose is the likely candidate).
- Backend: Rust + Axum, AWS Lambda custom runtime, ARM64.
- API: HTTPS Lambda Function URL initially.
- Identity: Amazon Cognito User Pool; accounts required.
- Domain: Domain-Driven Design (DDD) with bounded contexts and a hexagonal (ports-and-adapters) architecture.
- Persistence: DynamoDB adapter initially; domain/application layers must not depend on DynamoDB so another database can replace it.
- Synchronization: version-based polling, nominally every 15 seconds while a list is visible.
- Infrastructure: Terraform; GitHub Actions with AWS OIDC.
- Frontend quality: automated component/E2E tests plus visual regression tests.

## Guiding principles

1. Minimize idle cost and operational complexity.
2. Use DDD and hexagonal architecture; keep domain rules independent of frameworks and storage.
3. Keep business rules on the backend.
4. Keep API contracts client-independent.
5. Authorize every resource access server-side.
6. Use optimistic concurrency to prevent silent overwrites.
7. Verify every implementation step with objective evidence.
8. Add real-time infrastructure only when measured product needs justify it.

## Status

This repository contains the initial product and architecture baseline. Verify current AWS pricing, free-tier eligibility, quotas and regional availability against the target account before production deployment.
