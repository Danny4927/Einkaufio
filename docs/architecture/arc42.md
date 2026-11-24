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
5. Maintainability: DDD, hexagonal architecture, automated tests and reproducible infrastructure.
6. Persistence portability: changing DynamoDB to another database must not require changes to domain rules or application use cases.
7. Verifiable delivery: every implementation step has objective evidence that its acceptance criteria are met.
8. Visual stability: frontend changes are checked with automated visual regression tests.

## 2. Constraints

- Backend: Rust + Axum.
- Runtime: AWS Lambda custom runtime, ARM64, `provided.al2023`.
- Initial database: DynamoDB, one table, no GSI unless justified by an access pattern.
- Persistence technology is an adapter, not a domain dependency; a later database replacement must be possible behind repository ports.
- Authentication: Cognito User Pool; accounts are required.
- Web client: Angular PWA.
- Future client: native Android; implementation technology deferred.
- Synchronization: polling, nominally every 15 seconds while a list is visible.
- IaC: Terraform.
- CI/CD: GitHub Actions with AWS OIDC.
- Frontend visual regression tests are required.
- Every implementation step must produce reviewable evidence.
- Cost target: near-zero at small scale, not a guarantee. Verify current pricing, account eligibility, quotas, region and email/data-transfer costs.

## 3. Context and scope

### Users and external systems

- Registered user: Angular PWA now; native Android later.
- Cognito: registration, sign-in and token issuance.
- S3 + CloudFront: static Angular delivery.
- Lambda Function URL: HTTPS API ingress.
- DynamoDB: initial persistence adapter for profiles, memberships, lists, items and invitations.
- CloudWatch: logs and metrics.
- GitHub Actions: build, test and deploy.
- Terraform: infrastructure lifecycle.

Both clients are untrusted public clients. They authenticate with Cognito and call the same API with access tokens. No client accesses the database directly.

## 4. Solution strategy

- Use Domain-Driven Design to identify bounded contexts and model domain language, invariants and aggregates.
- Implement the backend as a modular monolith using hexagonal architecture (ports and adapters).
- Domain entities, value objects, domain services and invariants must not depend on Axum, Lambda, AWS SDK, DynamoDB, serialization frameworks or Angular.
- Application use cases orchestrate domain behavior through ports (for example repository and unit-of-work/transaction ports).
- Implement DynamoDB behind infrastructure adapters. Database-specific keys, expressions, transactions and SDK types remain inside that adapter.
- Deploy the Rust modular monolith as one Lambda function initially.
- Modules/bounded contexts: identity/profile, personal lists, households, items, invitations and synchronization. Refine boundaries as domain understanding grows.
- Use a versioned JSON REST API described by OpenAPI.
- Validate Cognito access-token signature (JWKS), issuer, expiry, token use and configured client claims. Use verified Cognito `sub` as principal ID.
- Use list-level versioning for sync and item-level versions for optimistic concurrency.
- Poll only while a list is visible; pause in background; refresh on focus/reconnect and after local writes.
- Keep Angular delivery separate from API compute.
- Keep browser-only concerns out of API and domain layers.

## 5. Building block view

### Level 1

1. Angular PWA (first client).
2. Future native Android client.
3. Rust API Lambda.
4. Cognito User Pool.
5. Persistence port implemented initially by DynamoDB adapter.
6. S3 + CloudFront for web assets.

### Level 2 – backend layers

- **Domain:** aggregates, entities, value objects, domain services, domain events and invariants; pure business rules.
- **Application:** use cases, command/query orchestration, authorization policy coordination and transaction boundaries; depends on domain and ports.
- **Ports:** repository interfaces, transaction/unit-of-work abstractions, clock/ID abstractions and external service interfaces.
- **Adapters – inbound:** Axum HTTP handlers and Lambda entry point; translate transport DTOs into use-case inputs.
- **Adapters – outbound:** DynamoDB repositories, Cognito/JWKS integration, clock/ID implementations and other AWS integrations.
- **Composition root:** constructs concrete adapters and injects them into application use cases.

Suggested dependency direction:

`Inbound adapters → Application → Domain`

`Outbound adapters → Ports ← Application`

The domain must not import infrastructure. The application layer may define/use ports; infrastructure implements them. Avoid leaking DynamoDB AttributeValue, PK/SK strings or AWS transaction request types beyond the DynamoDB adapter.

### Domain boundaries

Initial candidate bounded contexts:
- **Identity & Profiles:** application profile linked to Cognito subject.
- **Shopping Lists:** personal list lifecycle, list ownership and explicit membership.
- **Households:** household lifecycle, membership and household-owned lists.
- **Shopping Items:** item lifecycle, quantity, completion and ordering; may be part of the Shopping List aggregate boundary if invariants require it.
- **Invitations:** creation, expiry, revocation, acceptance and use limits.

These are logical boundaries inside one deployable modular monolith, not a mandate for microservices.

## 6. Runtime and deployment view

### Web

1. Browser loads Angular assets from CloudFront; CloudFront reads a private S3 origin using OAC.
2. Angular authenticates with Cognito.
3. Angular calls the Function URL with an access token in Authorization.
4. Lambda validates identity, authorizes the requested resource and invokes application use cases.
5. Application use cases use persistence ports implemented by the DynamoDB adapter.
6. Angular polls while the list view is active.

### Future Android

1. Android authenticates against the same Cognito User Pool using a public-client flow suitable for native apps (Authorization Code + PKCE where supported).
2. It sends the same access token to the same REST API.
3. It consumes the same schemas and error semantics.
4. Secure token storage, app navigation, Android App Links, notifications and local persistence remain Android-client concerns.

### Deployment

- CI: formatting, linting, unit/integration tests, architecture boundary checks, OpenAPI validation and frontend visual regression.
- Build Rust for Lambda ARM64 custom runtime.
- Terraform provisions AWS resources.
- Deploy Lambda artifact and Angular assets.
- GitHub assumes AWS deployment role through OIDC; no long-lived AWS keys.
- Every implementation PR attaches or links evidence for the checks relevant to its scope.

## 7. Crosscutting concepts

### DDD and persistence independence

- Express business behavior in domain language; keep invariants inside aggregates/domain services.
- Application use cases depend on repository ports, not concrete database clients.
- Repository interfaces express domain/application needs, not generic DynamoDB operations.
- Map persistence records to domain objects inside adapters.
- Keep transactions behind a port that represents required atomic behavior; DynamoDB transaction mechanics stay in the adapter.
- Do not make DynamoDB single-table keys part of public API or domain identifiers.
- Prove persistence independence with domain/application tests using in-memory fakes and adapter contract tests.
- A future database replacement should require a new adapter, composition-root/configuration changes and migration work, but no domain rule rewrite. Data migration and operational cutover are still required; database replacement is not assumed to be cost-free.

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
- Sync must not return item data inconsistent with reported list version.

### Invitations

- Generate cryptographically random high-entropy tokens.
- Store only SHA-256 token hash.
- Return raw token only once.
- Require authenticated acceptance.
- Enforce target permission, expiry, revocation and use count in backend.
- Use conditional transaction through an application persistence port; DynamoDB mechanics remain in adapter.
- DynamoDB TTL is cleanup only; always validate expiry in application code.

### Client portability

- OpenAPI is the source of truth for HTTP contract.
- Use stable IDs, ISO-8601 UTC timestamps and explicit enums.
- No browser-only cookies, DOM, localStorage or Angular assumptions in API.
- Keep business validation and authorization server-side.
- Generate or validate client models from OpenAPI.
- Android implementation stack is deferred; Kotlin/Jetpack Compose is a candidate, not a backend constraint.

### Quality and evidence

- Every implementation step has explicit acceptance criteria before work begins.
- CI results, test reports, visual diffs, coverage where meaningful, deployment plans and smoke-test outputs are evidence, not assumptions.
- A step is not complete merely because code was written or a PR was opened.
- Visual changes require Playwright visual regression results; intentional baseline changes require reviewed screenshots.
- Failed checks must be fixed or explicitly documented as a blocker; do not report success without a passing result.

### Observability

- Structured JSON logs with request/correlation ID.
- Never log tokens, passwords or invitation secrets.
- Finite CloudWatch retention.
- Monitor Lambda errors, throttles, duration and concurrency; DynamoDB throttling and consumed capacity.
- AWS Budgets are alerts, not hard spending caps.

## 8. Architecture decisions

See [ADRs](decisions/README.md): serverless modular monolith, Cognito, DynamoDB single-table, polling, personal lists + households, platform-neutral API, S3/CloudFront hosting, secure invitation links, DDD/hexagonal architecture with replaceable persistence, and evidence-based delivery with visual regression tests.

## 9. Quality requirements

### Security

- All application endpoints require valid Cognito access token except minimal health check.
- Every resource operation checks current persisted ownership/membership.
- Lambda IAM is least privilege.
- Invitations are hashed and never logged.
- CORS is restricted to configured web origins; CORS is not authorization.
- Function URL may use `AuthType=NONE` so web/mobile clients do not need AWS SigV4, but protected routes must enforce Cognito JWT validation.
- Validate request size and inputs.

### Testability and verifiability

- Domain tests do not require AWS or a live database.
- Application tests use fake ports to prove use-case behavior.
- DynamoDB adapter has contract/integration tests against a local or test environment.
- API contract is validated against OpenAPI.
- Angular has unit/component/E2E tests and visual regression coverage for critical screens and states.
- CI must publish actionable test reports and visual diff artifacts.
- Critical flows have deployed smoke tests before release.

### Performance and cost

- No provisioned Lambda concurrency initially.
- No VPC/NAT Gateway unless a concrete requirement appears.
- Use bounded paginated queries.
- Poll only active visible lists.
- Verify current free-tier terms and prices for actual account and region.

### Availability and recovery

- Database is durable system of record.
- Decide backup/PITR before meaningful production data.
- Deployment must be reproducible and rollbackable.
- Client recovers after connectivity loss by fetching current server state.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Polling read volume | Pause background tabs, bounded active lists, measure usage |
| Public Function URL abuse | JWT validation, input limits, concurrency guard, application throttling |
| Single-table complexity | Isolate in adapter; document access patterns; add GSI only with ADR |
| Database coupling | Ports, adapter contract tests and domain tests without AWS |
| Visual drift | Automated screenshot comparison and reviewed baseline updates |
| Unverified implementation claims | Evidence checklist and CI artifacts per step |
| Offline edits conflict | Start with cached reads; explicit pending write queue and conflict UX |
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
- Port: interface owned by the application/domain boundary.
- Adapter: technology-specific implementation of a port.
- Aggregate: DDD consistency boundary for domain invariants.
