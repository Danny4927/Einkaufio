# ADR-003: DynamoDB single-table design

- Status: Accepted
- Date: 2026-11-24

## Context
Required access patterns are user list/household overview, household members/lists, list metadata/items/members and invitation lookup.

## Decision
Use one DynamoDB table with PK and SK; no GSI initially. Encapsulate keys in repositories. Use conditional writes and transactions for multi-record consistency.

## Consequences
- Fewer resources and no GSI overhead.
- Access patterns must be designed explicitly.
- Some screens compose multiple bounded queries.
- Add an index/table only when a measured access pattern requires it and record an ADR.
- Confirm provisioned/on-demand mode and free-tier eligibility for target account/region.

## Alternatives
Per-entity tables are conceptually simpler but add resources/calls. A relational database is not needed for current access patterns.
