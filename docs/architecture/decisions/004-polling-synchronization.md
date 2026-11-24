# ADR-004: Polling synchronization

- Status: Accepted
- Date: 2026-11-24

## Context
Several people edit a list. Sub-second updates are not required in v1.

## Decision
Poll a version endpoint every 15 seconds while a list is visible. Pause when backgrounded. Refresh on focus, reconnect and after local mutations. If version changed, return current list snapshot; otherwise return only the version. Use optimistic concurrency and HTTP 409 for stale writes.

## Consequences
- Simple request/response Lambda model.
- Updates may be delayed by one polling interval plus network time.
- Traffic grows with active clients; pause hidden tabs and consider adaptive intervals.
- Reconsider WebSockets/AppSync only if user experience measurements justify it.

## Alternatives
WebSockets or AppSync subscriptions reduce latency but add connection/service complexity.
