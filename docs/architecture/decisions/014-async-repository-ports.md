# ADR 014: Asynchronous repository ports

- Status: Accepted
- Date: 2026-11-27

## Context

The application-owned repository ports (`ShoppingListRepository`, `ShoppingItemRepository`) were synchronous. Their callers are not: Axum handlers run on a multi-threaded Tokio runtime inside Lambda, and the planned DynamoDB adapter (roadmap A5/B1) uses the asynchronous AWS SDK. A synchronous port would force the adapter to block a runtime worker (`block_on` inside a runtime panics or deadlocks; `spawn_blocking` wastes threads and complicates cancellation). The in-memory reference adapter used `RefCell`, so it was `!Sync` and could not back a real router.

The domain and application crates must stay free of external dependencies; the architecture checker (`scripts/check_architecture.py`) rejects any non-workspace dependency of these crates, including dev-dependencies.

## Decision

- Repository ports are async traits using Rust's native `async fn`-in-trait support. Trait methods are declared as `fn …(&self, …) -> impl Future<Output = Result<…>> + Send`, so every returned future is `Send`; implementors simply write `async fn`.
- Repository traits have `Send + Sync` supertraits. ID generators stay synchronous (generation does no I/O) and also require `Send + Sync`.
- Fail-closed default methods return `async { Err(RepositoryError::Unavailable) }`.
- Ports are therefore not object-safe: dispatch is static. Use cases are generic over the repository type (`CreatePersonalList<'a, R>`), `ApiState<L>` and the router are generic over the list repository, and the composition root chooses the concrete adapter. ID generators remain `dyn`.
- No `async-trait` crate: it would add an external dependency to the application crate and box every call.
- Application unit tests drive use cases with a std-only single-poll executor (`test_support::BlockOn`, using `Waker::noop`). Fakes must complete without suspending; a suspension panics the test instead of hanging. The adapters and API crates use Tokio in tests.
- The in-memory adapter keeps all state behind one `std::sync::Mutex`. Each port operation takes the lock once and never holds it across an `.await`; a poisoned lock fails closed with `Unavailable`.
- The shared repository contract scenarios are `async fn`s that need no particular runtime, so any adapter can drive them from its own async tests.

## Consequences

- The compiler checks `Send` futures at every implementation and at every `tokio::spawn`; a multi-threaded in-memory test proves parallel writers with the same expected version commit exactly once.
- Adding a port method changes the generic signatures only where the method is used; no boxing or allocation per call.
- Selecting a persistence adapter at runtime (rather than at compile time) would need an enum or type-erasing wrapper in the composition root. No such requirement exists today.
- Port futures must not borrow non-`Sync` state; adapters needing interior mutability use `Send + Sync` primitives.

## Alternatives

- **`async-trait` crate:** object-safe, but adds an external dependency to a pure crate (violates the architecture check) and boxes every call.
- **Hand-written boxed futures (`Pin<Box<dyn Future + Send>>`):** object-safe without dependencies, but verbose for every implementor and allocates per call. Reconsider only if dynamic dispatch becomes necessary.
- **Keep synchronous ports and use `spawn_blocking` in adapters:** blocks threads for network I/O and leaks runtime concerns into adapters.
