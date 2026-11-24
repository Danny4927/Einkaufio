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

## Architecture at a glance

- Web: Angular PWA, static hosting on private S3 behind CloudFront.
- Future Android: native client (technology decision deferred; Kotlin/Jetpack Compose is the likely candidate).
- Backend: Rust + Axum, AWS Lambda custom runtime, ARM64.
- API: HTTPS Lambda Function URL initially.
- Identity: Amazon Cognito User Pool; accounts required.
- Persistence: one DynamoDB table, single-table design, no GSI initially.
- Synchronization: version-based polling, nominally every 15 seconds while a list is visible.
- Infrastructure: Terraform; GitHub Actions with AWS OIDC.

## Guiding principles

1. Minimize idle cost and operational complexity.
2. Keep business rules on the backend.
3. Keep API contracts client-independent.
4. Authorize every resource access server-side.
5. Use optimistic concurrency to prevent silent overwrites.
6. Add real-time infrastructure only when measured product needs justify it.

## Status

This repository begins with product and architecture documentation. Verify current AWS pricing, free-tier eligibility, quotas and regional availability against the target account before production deployment.
