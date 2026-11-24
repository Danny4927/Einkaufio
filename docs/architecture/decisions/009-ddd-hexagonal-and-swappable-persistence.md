# ADR-009: Domain-Driven Design, hexagonal architecture and replaceable persistence

- Status: Accepted
- Date: 2026-11-24

## Context

Einkaufio starts with DynamoDB to minimize operational overhead and cost. The product should not become permanently coupled to DynamoDB. Domain behavior must remain testable without AWS, and a future database change should be possible without rewriting business rules.

## Decision

Apply Domain-Driven Design (DDD) and hexagonal architecture (ports and adapters) in the Rust backend from the first implementation.

### DDD

- Define bounded contexts around business capabilities (initial candidates: Identity & Profiles, Shopping Lists, Households, Shopping Items and Invitations).
- Use ubiquitous language, explicit domain types, aggregates and invariants.
- Keep business rules in the domain model/domain services, not in HTTP handlers or persistence code.
- Treat bounded contexts as modules inside a modular monolith; do not create microservices prematurely.

### Hexagonal architecture

- Inbound adapters: Axum routes, Lambda entry point and transport DTO mapping.
- Application layer: use cases and orchestration.
- Domain layer: entities, value objects, domain services and invariants.
- Outbound ports: repository interfaces and explicit abstractions for atomic operations, clock, ID generation and external services where needed.
- Outbound adapters: DynamoDB implementation, Cognito/JWKS integration and AWS-specific services.
- Composition root wires ports to concrete adapters.

### Database independence

- Domain and application layers must not import AWS SDK or DynamoDB types.
- DynamoDB PK/SK design, AttributeValue, expressions and transaction request objects stay inside the DynamoDB adapter.
- Repository ports express domain/application needs; do not expose generic database operations.
- Map persistence records to domain objects inside adapters.
- Represent required atomic behavior through application-owned ports; implement the mechanics in the database adapter.
- Public API identifiers and domain identifiers must not encode DynamoDB keys.

## Consequences

- Domain and application tests can run with in-memory fake ports and no AWS credentials.
- DynamoDB can be replaced by implementing the same ports with another adapter and changing composition/configuration.
- A database replacement still requires data migration, operational cutover, performance/capacity validation and adapter contract tests; it is not a zero-effort switch.
- Avoid over-generalized abstractions: ports should represent actual use cases and consistency requirements.
- DynamoDB-specific access patterns remain documented in the adapter/data model documentation, not in domain rules.

## Verification

- Automated architecture/dependency checks prevent domain/application imports of AWS SDK and DynamoDB modules.
- Domain tests run without AWS or a database.
- Application tests use fake repository/transaction ports.
- DynamoDB adapter contract/integration tests verify the same port semantics.
- A persistence portability review is required for changes to ports or domain aggregates.
