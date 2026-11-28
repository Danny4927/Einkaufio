# ADR 019: DynamoDB adapter design

- Status: Accepted
- Date: 2026-11-28
- Builds on: ADR 003 (single table), ADR 014 (async ports), ADR 015 (access model), ADR 017 (households, invitations, bounded deletion)

## Context

Roadmap A5/B1 (and the DynamoDB parts of B2, B3, B5, B7, C2–C4, D2) need one persistent adapter that implements every repository port and passes the shared contract suite that the in-memory reference adapter passes. The in-memory adapter gets atomicity from one mutex; DynamoDB offers only single-item conditions and `TransactWriteItems` (at most 100 actions, cancelled as a whole). Three shapes of the ports do not map directly onto per-record writes:

- A reorder may move all 100 items of a list. With position and item version stored on each item record, that is 100 item updates plus the list version update, which exceeds the 100-action limit.
- Limits (5 households per user, 10 members, 20 lists per household, 5 valid invitations per household) must hold under concurrency, but DynamoDB cannot condition a write on a count of other records.
- Cascading deletes (a household with 20 lists × 100 items) do not fit in one transaction.

## Decision

### Packaging

- Module `adapters/src/dynamodb/` behind the cargo feature `dynamodb`. Default workspace builds do not compile the AWS SDK. DynamoDB types, keys and expressions never leave the module; ports see only domain values and port errors.
- `aws-sdk-dynamodb` without default features, plus `aws-smithy-http-client` with `rustls-ring`. The SDK's default HTTPS client would add aws-lc (C toolchain, extra licenses) and its legacy feature would add hyper 0.14/rustls 0.21. The adapter exposes `dynamodb::https_client()` (rustls 0.23 + ring, both already in the graph) and re-exports the SDK as `dynamodb::sdk` for the composition root (A4). No new license entered `deny.toml`.
- The domain gained `restore` constructors (`ShoppingList`, `ShoppingItem`, `Household`, `Invitation`, `UserProfile`). They reconstitute persisted state without business transitions. `Invitation::restore` rejects a use count above `maxUses`, so corrupted records fail closed.

### Key design

Keys follow `docs/data-model.md`, with these additions: list item index in `LIST#l / META`, tombstones `LIST#l / TOMB#i`, counters `USER#u / HOUSEHOLD_COUNT` and `HOUSEHOLD#h / LIST_COUNT`, the revision record `HOUSEHOLD#h / INVITE_REV`, invitation references `HOUSEHOLD#h / INVITE#<id>` and `USER#<creator> / INVITE#<hash>`. Prefix-queried sort keys end in `#`; singleton sort keys contain no `#`, so a prefix query never matches them. No secondary index is used.

### Item index in the list metadata

Position, item version and the list version of the item's last change (`changedAt`, for sync) are stored per item in a map `items` on `LIST#l / META`. Item records hold only the details (name, quantity, unit, note, checked, creator). Consequences:

- A reorder is one conditional write of the metadata record, whatever the number of moved items.
- Every item mutation already updates the metadata (list version), so the index costs no extra action. Item count, unique positions and the next position are checked on one record.
- The metadata grows with the item count, bounded by 100 entries (about 5–15 KB). Writes and consistent reads of a full list consume a few capacity units instead of one. That is acceptable at the expected scale; revisit if measured cost requires it (D5).

### Optimistic transactions and classification

Every mutation is a bounded loop (at most 40 attempts, jittered exponential back-off up to 100 ms):

1. Consistent reads of every record the decision depends on (`GetItem`, `TransactGetItems` for snapshots of several records).
2. The decision with the domain model, in the error order of the ports (`NotFound` → `Forbidden` → `Conflict` → business rules and limits). No-ops return without writing.
3. One `TransactWriteItems` whose conditions assert that the inputs are unchanged: aggregate version and `ACTIVE` state on list or household metadata, `ownerId` for personal lists and administrative household operations, existence of the principal's membership record for contributor operations (ADR 015), `usedCount`/`revoked` on invitations, bound conditions on counters (`attribute_not_exists(count) OR count < max`), and the invitation revision.

If DynamoDB cancels the transaction with `ConditionalCheckFailed`, `TransactionConflict` or throttling, nothing was written and the next attempt re-reads, so `NotFound`, `Forbidden`, `Conflict` and limit errors are always classified from persisted state, never from client input. Any other cancellation reason (for example a validation error) and attempt exhaustion fail closed with `Unavailable`. Every call carries a fresh idempotency token, which makes the SDK's own retries safe.

Counters and limits:

- Members per household: `memberCount` on the household metadata; every membership change advances the household version, so the version condition covers it.
- Households per user and lists per household: counter records with bound conditions, incremented and decremented in the same transaction as the membership or list.
- Valid invitations per household: validity depends on time, so no counter can track it. Creation reads `INVITE_REV`, then the household's invitations, and writes `INVITE_REV + 1` conditioned on the value read. Concurrent creations therefore serialize. Acceptance, revocation and expiry only ever reduce the number of valid invitations, so they need not touch the revision.

### Consistent reads of list content

Items are read with a partition query between two consistent reads of the list metadata. Every item change advances the list version in the same transaction, so identical metadata before and after the query proves a consistent snapshot. Otherwise the read is retried (bounded). Item pagination and the accessible-list pagination sort bounded in-memory sets (≤ 100 items; the principal's list references), and the opaque cursor holds the last position or list id, hex-encoded and bound to its query and list. It is never used for authorization.

### Sync tombstones

Deleting an item writes `TOMB#i` with the list version of the deletion in the same transaction. Re-adding the id removes the tombstone. Tombstones are kept for the list's lifetime and swept with the list, so changes can always be derived for a version the list had. Pruning would need a "changes available since" floor on the metadata (snapshot below it) and is deferred until tombstone volume is measured.

### Bounded deletion (ADR 017)

- **List:** one transaction checks access and version, marks the metadata `DELETING`, removes the owner or household reference and decrements the household list counter. Then the partition is swept with `BatchWriteItem` (25 per call), metadata last.
- **Household:** one transaction checks owner and version, marks the metadata `DELETING`, and deletes every membership record, every member's household reference and decrements each member's household counter (1 + 3 × 10 actions). Access and limit slots therefore end atomically. Then the household's lists (whole partitions), invitations with both references, counters and finally the metadata are swept.
- `DELETING` records are treated as nonexistent. The sweep is idempotent. If it fails after the mark, the call returns `Unavailable`; the list administrator's or household owner's repeated delete resumes the sweep and returns `NotFound`, like any repeated delete. The id stays reserved (`AlreadyExists`) until the sweep has finished.

### Account data (D2)

`delete_profile` deletes `USER#u / PROFILE` conditioned on the version read. `delete_invitations_created_by` follows the creator references `USER#u / INVITE#<hash>` (written with every invitation) and deletes each invitation with its household reference in one transaction. No scan or index is needed.

### Tests

`make test-dynamodb` runs the adapter's unit tests and `tests/dynamodb_contracts.rs`: every shared contract (list, item, shared access, reorder, pagination, sync, shared query, profile, household, membership lifecycle, invitation, account data), the ADR 017 race tests (concurrent acceptances, same-user acceptance, member limit, revoke against accept, concurrent transfers), concurrent adds, checks and household creations, a full 100-item reverse reorder, and a scan proving that deletions leave no orphaned records. Each test creates its own table. The tests fail instead of skipping when `DYNAMODB_ENDPOINT` is unset. CI runs them in the `backend-dynamodb` job against the `amazon/dynamodb-local:3.3.2` service container.

## Consequences

- DynamoDB Local is a functional emulator: it proves conditions, transactions and cancellation handling, not production latency, throttling or capacity cost. These are measured in dev (D5).
- Hot lists or households serialize writers on one metadata record; concurrent writers retry. That suits shopping-list contention. Sustained contention beyond 40 attempts surfaces as `Unavailable` (fail closed).
- Invitation ids are unique per household (`HOUSEHOLD#h / INVITE#<id>`); token hashes are unique globally (`INVITE#<hash>`). Every invitation operation is household-scoped, so no global id index is kept.
- The Lambda IAM policy (`modules/lambda_api`) gained `dynamodb:BatchWriteItem` for the sweeps; transactions are authorized through the existing per-item actions.
- Table creation stays in Terraform (`modules/dynamodb_table`). Tests create throwaway tables with the same `PK`/`SK` string key schema.
- The composition root (A4) still has to build the SDK client (region, credentials, `https_client()`) and pass the table name.

## Alternatives

- **Position and version on each item record:** self-contained items, but a reorder of more than 98 items cannot fit in one transaction, and a multi-transaction reorder would expose duplicate positions or need locks with recovery. Rejected.
- **Whole items inside the list metadata:** at most one record per list, but up to ~300 KB per record and per write. Rejected.
- **Re-read classification of cancellation reasons per action index:** more precise errors without a full re-read, but brittle (action order) and still racy. The full re-read gives the port's error order for free. Rejected.
- **SDK default HTTPS client (aws-lc) or legacy rustls feature:** extra C toolchain and licenses, or duplicate outdated TLS stacks. Rejected.
- **GSI for invitations by creator:** not needed; a creator reference record serves the only access pattern.
