# ADR 016: Per-principal request throttling

- Status: Accepted
- Date: 2026-11-27

## Context

Roadmap B8 requires application-level abuse controls in addition to AWS-side limits. The API runs as a Lambda function behind a function URL, so there is no API Gateway usage plan, and a shared throttling store (DynamoDB counters, ElastiCache) would add a write and latency to every request and cost the free-tier budget.

## Decision

- The application layer owns a `RateLimiter` port (`application/src/rate_limit.rs`). It is synchronous because the chosen implementation is local and cheap; a distributed limiter would get its own asynchronous port (ADR 014).
- The inbound HTTP adapter consults it for every protected request **after** Cognito verification, keyed by the verified principal only (never client-supplied identifiers, never before authentication). An exhausted budget answers `429 Too Many Requests` with `Retry-After`; a limiter failure answers `503` (fail closed).
- The production adapter is an in-process token bucket (`adapters/src/token_bucket.rs`, default burst 60, refill 10/s). The number of tracked principals is capped; when the cap is reached and no idle bucket can be evicted, new principals are throttled rather than tracked.
- Throttling is therefore **best effort per Lambda instance**. The global bound is Lambda reserved concurrency (Terraform), which caps the number of instances and therefore the aggregate rate.
- Request bodies are limited to 16 KiB at the router (`413`), and domain value objects enforce field limits (names, notes, page size 1–100, cursor length, reorder size).

## Consequences

- No extra persistence cost or latency; limits reset on cold starts and a principal can obtain up to `reserved concurrency × burst` across instances.
- If abuse beyond that bound is observed, a shared-store limiter or AWS WAF can replace the adapter without changing the port or handlers.
