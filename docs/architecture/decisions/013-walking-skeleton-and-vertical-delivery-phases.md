# ADR 013: Walking skeleton first and vertical delivery phases

- Status: Accepted
- Date: 2026-11-27

## Context

The original roadmap was organised as horizontal milestones (repository foundation, infrastructure, authentication, lists, households, sharing, sync, quality). Several quality milestones overlapped (M0–M0c and M7, M1 and M1.0) and the recorded status drifted from the code.

By 2026-11-27 the domain and application layers, an in-memory reference adapter and Cognito token verification were implemented and tested, but no AWS resource, persistence adapter, Lambda entry point or authenticated UI existed. The highest-risk assumptions — DynamoDB transactional semantics behind the ports, Cognito sign-in from the PWA, Lambda Function URL with CORS, CloudFront/OAC hosting — were unverified. In addition, all list/item ports are owner-scoped, while v1 requires household members (and possibly explicit list members) to access lists; each further owner-only use case increases the later refactoring cost.

## Decision

- Organise implementation into four vertical phases, each with an exit criterion demonstrated in a deployed environment: A walking skeleton (dev), B single-user shopping, C access model and collaboration, D release readiness (prod).
- Phase A delivers the thinnest browser → API → DynamoDB path in dev before further domain features are added.
- Decide the access model (owner, household member, explicit list member) in an ADR at the start of Phase C, before household code, and adapt the existing ports and contract scenarios to it. The same ADR decides whether explicit list sharing ships in v1 or v1.x.
- Track quality tooling as a single continuous track. Each open item names the trigger that schedules it, so tooling follows the code it protects rather than preceding user-visible value.
- Treat GDPR basics (account deletion, data export, privacy notice) and Function URL abuse controls as explicit roadmap items, not implicit quality concerns.

## Consequences

- Infrastructure, deployment and adapter risks surface early, when changing the architecture is cheapest.
- Some quality tooling (mutation testing, coverage threshold, agent review automation) is deliberately deferred; the evidence policy of ADR 010 remains unchanged.
- The roadmap remains the single progress source of truth; its "Current state" section must be updated whenever a PR changes implementation state.
- Owner-only port signatures are known technical debt until the Phase C access-model ADR is implemented.
