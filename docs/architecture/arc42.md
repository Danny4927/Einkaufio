# Einkaufio – Architecture Documentation (arc42)

**Status:** Architecture baseline  
**Date:** 2026-11-24  
**Primary AWS region:** `eu-central-1` (Frankfurt)

## 1. Introduction and goals

Einkaufio is a collaborative shopping-list application. Every user has an account and can create personal lists, share personal lists with selected users, create or join households, and manage multiple household-owned lists.

The first client is an Angular PWA. A native Android client is a future option and must be possible without duplicating backend business logic or authorization.

### Quality goals

1. Security and privacy: access is always checked server-side.
2. Low operating cost: avoid always-on infrastructure and scale compute to zero.
3. Correct collaboration: concurrent changes must not silently overwrite each other.
4. Client portability: web and Android consume the same platform-neutral API.
5. Maintainability: modular monolith, automated tests and reproducible infrastructure.
6. Usability: simple sharing and updates through polling.

## 2. Constraints

- Backend: Rust + Axum.
- Runtime: AWS Lambda custom runtime, ARM64, `provided.al2023`.
- Database: DynamoDB, one table initially, no GSI unless justified by a documented access pattern.
- Authentication: Cognito User Pool; accounts are required.
- Web client: Angular PWA.
- Future client: native Android; implementation technology deferred.
- Synchronization: polling, nominally every 15 seconds while a list is visible.
- IaC: Terraform.
- CI/CD: GitHub Actions with AWS OIDC.
- Cost target: near-zero at small scale, not a guarantee. Verify current pricing, account eligibility, quotas, region and email/data-transfer costs.

## 3. Context and scope

### Users and external systems

- Registered user: Angular PWA now; native Android later.
- Cognito: registration, sign-in and token issuance.
- S3 + CloudFront: static Angular delivery.
- Lambda Function URL: HTTPS API ingress.
- DynamoDB: profiles, memberships, lists, items and invitations.
- CloudWatch: logs and metrics.
- GitHub Actions: build, test and deploy.
- Terraform: infrastructure lifecycle.

Both clients are untrusted public clients. They authenticate with Cognito and call the same API with access tokens. No client accesses DynamoDB directly.

## 4. Solution strategy

- Deploy one Rust modular monolith as one Lambda function.
- Modules: profile, personal lists, households, items, invitations, synchronization.
- Use a versioned JSON REST API described by OpenAPI.
- Validate Cognito access-token signature (JWKS), issuer, expiry, token use and configured client claims. Use verified Cognito `sub` as principal ID.
- Store data in one DynamoDB table using explicit PK/SK access patterns.
- Use conditional writes and transactions for atomic multi-record operations.
- Use list-level versioning for sync and item-level versions for optimistic concurrency.
- Poll only while a list is visible; pause in background; refresh on focus/reconnect and after local writes.
- Keep Angular delivery separate from API compute.
- Keep browser-only concerns out of the API and domain layer.

## 5. Building block view

### Level 1

1. Angular PWA (first client).
2. Future native Android client.
3. Rust API Lambda.
4. Cognito User Pool.
5. DynamoDB table.
6. S3 + CloudFront for web assets.

### Level 2 – backend modules

- API: Axum routes, validation, response/error mapping.
- Authentication: token verification and principal extraction.
- Authorization: centralized list/household policies.
- Personal lists: ownership and explicit memberships.
- Households: metadata, members and household lists.
- Items: CRUD, check-off and ordering.
- Invitations: token generation/hash/expiry/revocation/use limits.
- Sync: list version and item snapshot retrieval.
- Persistence: DynamoDB repositories and transaction composition.

HTTP handlers depend on application use cases. Use cases depend on domain rules and repository interfaces. DynamoDB SDK code is an infrastructure adapter. Domain modules must not depend on Axum, Lambda or Angular.

## 6. Runtime and deployment view

### Web

1. Browser loads Angular assets from CloudFront; CloudFront reads a private S3 origin using OAC.
2. Angular authenticates with Cognito.
3. Angular calls the Function URL with an access token in Authorization.
4. Lambda validates identity, authorizes the requested resource and accesses DynamoDB.
5. Angular polls while the list view is active.

### Future Android

1. Android authenticates against the same Cognito User Pool using a public-client flow suitable for native apps (Authorization Code + PKCE where supported).
2. It sends the same access token to the same REST API.
3. It consumes the same schemas and error semantics.
4. Secure token storage, app navigation, Android App Links, notifications and local persistence remain Android-client concerns.

### Deployment

- CI: formatting, linting, unit/integration tests and OpenAPI validation.
- Build Rust for Lambda ARM64 custom runtime.
- Terraform provisions AWS resources.
- Deploy Lambda artifact and Angular assets.
- GitHub assumes AWS deployment role through OIDC; no long-lived AWS keys.

## 7. Crosscutting concepts

### Identity and authorization

- Cognito `sub` is canonical user ID.
- Never trust caller IDs supplied in request bodies.
- Personal list: owner or active explicit list member.
- Household list: active member of its owning household.
- Household membership grants access to all household-owned lists.
- Every operation checks persisted authorization server-side.
- Avoid leaking whether inaccessible resources exist.

### Collaboration and concurrency

- Each list has monotonically increasing `version`.
- Each item has `itemVersion`.
- Mutations include expected item version when changing/deleting an existing item.
- Item mutation and list version increment must be atomic.
- Stale writes return HTTP 409.
- Sync must not return item data that is inconsistent with the reported list version; use a consistent read or a retryable version-check protocol.

### Invitations

- Generate cryptographically random high-entropy tokens.
- Store only SHA-256 token hash.
- Return raw token only once.
- Require authenticated acceptance.
- Enforce target permission, expiry, revocation and use count in backend.
- Use conditional transaction for use limit and membership creation.
- DynamoDB TTL is cleanup only; always validate expiry in application code.

### Client portability

- OpenAPI is the source of truth for HTTP contract.
- Use stable IDs, ISO-8601 UTC timestamps and explicit enums.
- No browser-only cookies, DOM, localStorage or Angular assumptions in API.
- Keep business validation and authorization server-side.
- Generate or validate client models from OpenAPI.
- Android implementation stack is deferred; Kotlin/Jetpack Compose is a candidate, not a backend constraint.

### Observability

- Structured JSON logs with request/correlation ID.
- Never log tokens, passwords or invitation secrets.
- Finite CloudWatch retention.
- Monitor Lambda errors, throttles, duration and concurrency; DynamoDB throttling and consumed capacity.
- AWS Budgets are alerts, not hard spending caps.

## 8. Architecture decisions

See [ADRs](decisions/README.md): serverless modular monolith, Cognito, DynamoDB single-table, polling, personal lists + households, platform-neutral API, S3/CloudFront hosting and secure invitation links.

## 9. Quality requirements

### Security

- All application endpoints require valid Cognito access token except minimal health check.
- Every resource operation checks current persisted ownership/membership.
- Lambda IAM is least privilege.
- Invitations are hashed and never logged.
- CORS is restricted to configured web origins; CORS is not authorization.
- Function URL may use `AuthType=NONE` so web/mobile clients do not need AWS SigV4, but protected routes must enforce Cognito JWT validation.
- Validate request size and inputs. Function URL does not provide API Gateway usage plans; add application-level controls and revisit edge/API protections if abuse becomes material.

### Performance and cost

- No provisioned Lambda concurrency initially.
- No VPC/NAT Gateway unless a concrete requirement appears.
- Use bounded paginated queries.
- Poll only active visible lists.
- Verify current free-tier terms and prices for the actual AWS account and region.
- Monitor capacity and avoid claiming zero cost as guaranteed.

### Availability and recovery

- DynamoDB is durable system of record.
- Decide backup/PITR before meaningful production data.
- Deployment must be reproducible and rollbackable.
- Client recovers after connectivity loss by fetching current server state.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Polling read volume | Pause background tabs, bounded active lists, measure usage |
| Public Function URL abuse | JWT validation, input limits, concurrency guard, application throttling |
| Single-table complexity | Document access patterns; add GSI only with ADR |
| Offline edits conflict | Start with cached reads; explicit pending write queue and conflict UX before claiming full offline |
| AWS pricing/free-tier changes | Budget alerts and recurring cost review |
| Android requirements differ | Stable OpenAPI and shared backend domain rules |

## 11. Glossary

- Personal list: user-owned list with optional explicit members.
- Household: shared container with members and multiple household-owned lists.
- Owner: principal allowed to administer a resource.
- Member: principal with collaborative access.
- List version: revision used for synchronization.
- Item version: revision for optimistic concurrency.
- Invitation: time-limited secret link accepted by an authenticated user.
- PWA: installable web client; not a product architecture constraint.
