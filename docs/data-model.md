# Einkaufio – Data model and DynamoDB access patterns

## Domain model

### User profile
Cognito is the identity source. Store application-specific profile data:
- `PK=USER#<cognito-sub>`
- `SK=PROFILE`

Attributes: `displayName`, optional normalized `email`, timestamps. Never use email as a primary key; it can change.

### Personal list
- `PK=LIST#<listId>`
- `SK=META`
- Attributes: `type=PERSONAL`, `ownerId`, `name`, `version`, timestamps.
- Owner overview reference: `PK=USER#<ownerSub>`, `SK=LIST#<listId>`.
- Explicit member (v1.x, ADR 015): `PK=LIST#<listId>`, `SK=MEMBER#<memberSub>`.

### Household
- Metadata: `PK=HOUSEHOLD#<householdId>`, `SK=META`.
- Member: `PK=HOUSEHOLD#<householdId>`, `SK=MEMBER#<userSub>`.
- User overview reference: `PK=USER#<userSub>`, `SK=HOUSEHOLD#<householdId>`.
- Household list reference: `PK=HOUSEHOLD#<householdId>`, `SK=LIST#<listId>`.
- Household-owned list metadata: `PK=LIST#<listId>`, `SK=META`, with `type=HOUSEHOLD` and `householdId`.

### Shopping item
- `PK=LIST#<listId>`
- `SK=ITEM#<itemId>`
- Attributes: name, optional quantity/unit/note, checked, position, itemVersion, createdBy, createdAt, updatedAt.
- Items are individual records, not an unbounded array in list metadata.

### Invitation
- `PK=INVITE#<sha256(token)>`
- `SK=META`
- Attributes: inviteId, targetType (LIST or HOUSEHOLD), targetId, createdBy, expiresAt, maxUses, usedCount, revoked, optional TTL attribute.
- Raw token is returned once and never persisted/logged.

## Access patterns

| Use case | DynamoDB access |
|---|---|
| Get user profile | GetItem USER / PROFILE |
| List user's personal lists | Query USER with begins_with(SK, LIST#) |
| List all lists accessible to a user (ADR 015) | Owned list refs above; from C2 also Query USER begins_with(SK, HOUSEHOLD#), Query each HOUSEHOLD#id begins_with(SK, LIST#), then BatchGet LIST META |
| Resolve a principal's access to a list | GetItem LIST#id / META; household list: GetItem HOUSEHOLD#id / MEMBER#user |
| List user's households | Query USER with begins_with(SK, HOUSEHOLD#) |
| Get household metadata/members/lists | Query HOUSEHOLD#id |
| Get list metadata/items/members | Query LIST#id |
| Verify personal-list member (v1.x) | GetItem LIST#id / MEMBER#user |
| Verify household member | GetItem HOUSEHOLD#id / MEMBER#user |
| Lookup invitation | GetItem INVITE#hash / META |

No GSI is needed for the documented initial access patterns.

## Atomic operations

### Create personal list
TransactWrite:
1. Put LIST META, conditional not-exists.
2. Put USER list reference, conditional not-exists.

### Create household
TransactWrite:
1. Put HOUSEHOLD META.
2. Put owner MEMBER record.
3. Put USER household reference.

### Create household list
TransactWrite:
1. Put LIST META with type HOUSEHOLD and householdId.
2. Put HOUSEHOLD list reference.

### Add/update/delete item
Use a transaction:
1. Put/update/delete item with itemVersion condition where applicable.
2. Increment LIST META version with a condition on current list state.
3. Return the committed new version.

An item update with stale expected version returns HTTP 409. List version and item mutation must be atomic.

### Accept invitation
Use a transaction with:
- Conditional update: not revoked, not expired, usedCount below maxUses.
- Conditional creation of membership.
- Creation of user overview reference.
- Idempotent response if the caller is already a member; do not consume an additional use.

## Version semantics

- List version increases once per successful visible mutation.
- Item version starts at 1 and increments on each successful item update.
- Member changes affecting visible membership state should increment list or household version as appropriate.
- Sync must not report a version inconsistent with returned items. Use a consistent read or a safe version-check/retry loop.

## Pagination and limits

Use DynamoDB LastEvaluatedKey wrapped in an opaque, validated API cursor. Never expose raw internal key values as trusted authorization input. Bound result sizes. Initial API list/item page size should be no more than 100.

## Important modeling note

DynamoDB TransactWriteItems has a maximum transaction size and item limits. Household deletion and large cascades cannot assume every child can be deleted in one transaction. Define bounded deletion workflows (mark resource DELETING, deny new access, then batch-delete children) or restrict deletion behavior for v1.
