# Einkaufio – Data model and DynamoDB access patterns

One table with string keys `PK` (hash) and `SK` (range), no secondary index (ADR 003). The layout below is implemented by the DynamoDB adapter (`backend/crates/adapters/src/dynamodb/`, feature `dynamodb`, ADR 019); keys are built only in `dynamodb/keys.rs`. Every record carries an `entity` attribute. Identifiers are validated domain values (no control characters, at most 128 characters). Sort-key prefixes used with `begins_with` end in `#`; singleton sort keys (`META`, `PROFILE`, counters) contain no `#`, so prefix queries never match them.

Timestamps (created/updated) are not part of the domain model yet and are not stored.

## Records

### User partition `PK=USER#<cognito-sub>`

| SK | Entity | Attributes | Purpose |
|---|---|---|---|
| `PROFILE` | `PROFILE` | `userId`, `displayName`, `version` | Application profile. Cognito is the identity source; email is never a key. |
| `LIST#<listId>` | `LIST_REF` | `listId` | Owned personal list (overview). |
| `HOUSEHOLD#<householdId>` | `HOUSEHOLD_REF` | `householdId` | Active membership (overview); written and removed with the membership record. |
| `HOUSEHOLD_COUNT` | counter | `count` | Households the user belongs to (owned or joined); bound condition `< 5`. |
| `INVITE#<sha256hex>` | `INVITE_REF` | `householdId`, `invitationId`, `tokenHash` | Invitations the user created (account deletion, D2). |

### List partition `PK=LIST#<listId>`

| SK | Entity | Attributes |
|---|---|---|
| `META` | `LIST` | `listId`, `ownerType` (`PERSONAL`/`HOUSEHOLD`), `ownerId` or `householdId`, `name`, `version`, `state` (`ACTIVE`/`DELETING`), `nextPosition`, `items` |
| `ITEM#<itemId>` | `ITEM` | `itemId`, `name`, optional `quantity`/`unit`/`note`, `checked`, `createdBy` |
| `TOMB#<itemId>` | `TOMBSTONE` | `itemId`, `deletedAt` (list version of the deletion) |
| `MEMBER#<memberSub>` | — | Explicit list member, v1.x only (ADR 015); not implemented. |

`items` is the **item index**: a map `itemId → {p: position, v: item version, c: list version of the last change}`. It is bounded by the 100-active-item limit and lets a reorder of any number of items be one conditional write (ADR 019). Items are individual records for their details, never an unbounded array.

### Household partition `PK=HOUSEHOLD#<householdId>`

| SK | Entity | Attributes |
|---|---|---|
| `META` | `HOUSEHOLD` | `householdId`, `name`, `ownerId`, `version`, `memberCount`, `state` (`ACTIVE`/`DELETING`) |
| `MEMBER#<userSub>` | `MEMBER` | `householdId`, `userId` (the role derives from `ownerId`) |
| `LIST#<listId>` | `LIST_REF` | `listId` (household-owned list) |
| `LIST_COUNT` | counter | `count`; bound condition `< 20` |
| `INVITE#<invitationId>` | `INVITE_REF` | `householdId`, `invitationId`, `tokenHash` |
| `INVITE_REV` | `REVISION` | `rev`; advanced by every invitation creation |

### Invitation `PK=INVITE#<sha256(token) hex>`, `SK=META`

Entity `INVITATION`: `invitationId`, `householdId`, `createdBy`, `expiresAt` (Unix seconds, exclusive), `maxUses`, `usedCount`, `revoked`. The raw token is returned once to its creator and never persisted or logged; the hash appears only in keys and references, which must not be logged either (docs/security.md). Expiry is always checked in code; a TTL attribute, if added later, is cleanup only.

## Access patterns

| Use case | DynamoDB access |
|---|---|
| Get user profile | GetItem `USER#u / PROFILE` (consistent) |
| List all lists accessible to a user (ADR 015) | Query `USER#u` `begins_with(SK, LIST#)` and `begins_with(SK, HOUSEHOLD#)`; per household (≤ 5) Query `HOUSEHOLD#h` `begins_with(SK, LIST#)` (≤ 20); BatchGetItem `LIST#l / META` (consistent); drop deleting or no-longer-matching lists |
| Page accessible lists | Same candidates sorted by list id; BatchGet only up to the page; cursor = last list id |
| Resolve a principal's access to a list | GetItem `LIST#l / META`; household list: TransactGetItems `HOUSEHOLD#h / META` + `HOUSEHOLD#h / MEMBER#u` |
| List/page items, sync | GetItem `LIST#l / META`, Query `LIST#l`, GetItem `LIST#l / META` again; equal metadata proves a consistent snapshot |
| List user's households | Query `USER#u` `begins_with(SK, HOUSEHOLD#)`; BatchGetItem `HOUSEHOLD#h / META` |
| Get household with members | TransactGetItems `META` + own `MEMBER#u`; Query `begins_with(SK, MEMBER#)`; re-read `META` |
| List household invitations | Query `HOUSEHOLD#h` `begins_with(SK, INVITE#)`; BatchGetItem `INVITE#hash / META` |
| Lookup invitation | GetItem `INVITE#hash / META` |
| Invitations created by a user (D2) | Query `USER#u` `begins_with(SK, INVITE#)` |

## Atomic operations

All writes are `TransactWriteItems` conditioned on the records the decision was based on; a cancelled transaction is re-read and classified (ADR 019). "Guard" means the access re-check of ADR 015: `ownerId = principal` on personal-list metadata, a ConditionCheck that `HOUSEHOLD#h / MEMBER#principal` exists for contributor operations, or a ConditionCheck `state = ACTIVE AND ownerId = principal` on `HOUSEHOLD#h / META` for administrative operations on household lists.

| Operation | Transaction |
|---|---|
| Create personal list | Put `LIST META` (not exists), Put `USER` list reference |
| Create household | Put `HOUSEHOLD META` (not exists, `memberCount 1`), Put owner `MEMBER` (not exists), Put `USER` household reference, ADD `USER HOUSEHOLD_COUNT` (bound `< 5`) |
| Create household list | Put `LIST META` (not exists, `type HOUSEHOLD`), Put household list reference, ADD `LIST_COUNT` (bound `< 20`), guard |
| Rename list / household, transfer ownership | Put `META` conditioned on version, `ACTIVE` and owner; transfer also checks the target `MEMBER` exists |
| Add item | Put `LIST META` (version, `ACTIVE`, new index entry, `nextPosition + 1`), Put `ITEM` (not exists), Delete `TOMB`, guard |
| Check/update item | Put `LIST META` (version; entry `v`, `c`), Put `ITEM` (exists), guard |
| Delete item | Put `LIST META` (version; entry removed), Delete `ITEM` (exists), Put `TOMB`, guard |
| Reorder items | Put `LIST META` (version; positions and versions of moved items), guard — one record, whatever the number of moved items |
| Accept invitation | Put `INVITATION` (`usedCount = read`, `revoked = false`), Put `HOUSEHOLD META` (version, `ACTIVE`, `memberCount + 1`), Put `MEMBER` (not exists), Put `USER` reference, ADD `HOUSEHOLD_COUNT` (bound `< 5`) |
| Leave / remove member | Put `HOUSEHOLD META` (version, `memberCount − 1`), Delete `MEMBER` (exists), Delete `USER` reference, ADD `HOUSEHOLD_COUNT −1` |
| Create invitation | Put `INVITATION` (not exists), Put household `INVITE#id` (not exists), Put creator `USER` reference, Put `INVITE_REV` (= read + 1), guard (owner) |
| Revoke invitation | Put `INVITATION` (`usedCount = read`, `revoked = false`), guard (owner) |

Invalid states are refused before the write in the documented error order; the conditions guarantee that the state read is still the state written over.

## Version semantics

- List version increases once per successful visible mutation (rename, item add/update/check/delete, effective reorder). No-ops do not write.
- Item version starts at 1 and increments on each item change, including being moved by a reorder.
- Household version increases on rename, ownership transfer and every membership change (join, leave, removal).
- Sync reports a version consistent with the returned items (metadata read before and after the query).

## Pagination and limits

Cursors are opaque, hex-encoded, kind-prefixed and bound to their list; they hold the last list id or item position and are never authorization input. Pages hold at most 100 entries. Items (≤ 100 per list) are paged from one consistent read; accessible lists are paged over the principal's sorted list references.

## Bounded deletion

TransactWriteItems allows at most 100 actions, so cascades are two-step (ADR 017, ADR 019):

1. **Mark:** one transaction checks access and version and sets `state = DELETING`. For a list it also removes the owner or household reference and decrements `LIST_COUNT`; for a household it also deletes every `MEMBER` record and member reference and decrements each member's `HOUSEHOLD_COUNT` (≤ 31 actions). From now on the resource is `NotFound`, its invitations are unavailable and the limit slots are free.
2. **Sweep:** BatchWriteItem (25 per call) deletes the children — items and tombstones; for households every list partition, invitation records with both references, counters and the revision — and finally the metadata. The sweep is idempotent. If it fails, the call reports `Unavailable`; the administrator's repeated delete resumes it and reports `NotFound`. The id stays reserved until the sweep completes.

Tombstones are kept for the list's lifetime. Pruning them would need a "changes available since" floor on the list metadata and is deferred until volume is measured.
