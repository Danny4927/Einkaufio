# ADR-005: Personal lists and households

- Status: Accepted
- Date: 2026-11-24

## Context
Users need both personal lists shared with selected people and persistent households with multiple lists.

## Decision
Support two scopes:
- PERSONAL: owned by one user; additional users have explicit list memberships.
- HOUSEHOLD: owned by a household; active household members access all household lists.

Household has owner and members. Membership grants access to all household-owned lists. Personal-list membership is independent.

## Consequences
- Users can combine private lists and shared household lists.
- Household membership changes automatically affect all household lists.
- Owner transfer is required before owner leaves.
- Do not duplicate household memberships on each list.

## Alternatives
Workspace-only model does not support private lists. Per-list membership for household lists creates redundant writes and administration.
