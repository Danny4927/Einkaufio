# ADR 015: List access model

- Status: Accepted
- Date: 2026-11-27
- Amends: ADR 005 (sequencing of explicit list membership; the model itself is unchanged)

## Context

ADR 005 defines two ways to collaborate: personal lists with explicit list members, and households whose active members access every household-owned list. The implemented list and item ports were owner-only (`list_personal_for_owner`, adapters comparing `owner_id`), which cannot express household or member access. ADR 013 required this access-model decision, and the v1/v1.x scope decision on explicit list sharing, before any household code (roadmap C1) and before the DynamoDB adapters are built on the ports.

Supporting both sharing models in v1 roughly doubles the authorization matrix (owner/member/non-member for personal and household lists, plus two invitation targets).

## Decision

### Scope

- **v1:** a principal gains access to a list through personal ownership or household membership.
- **v1.x:** explicit list membership (sharing a personal list with selected users) and personal-list invitations are deferred (roadmap C5). v1 contains no code, routes or records for them. `/api/lists/{listId}/invites` in the OpenAPI contract is marked as v1.x.

### Access sources and effective access

| Source | Effective access | Available |
|---|---|---|
| Owner of a personal list | Administrator | now |
| Owner of the household that owns the list | Administrator | C2 |
| Active member of the household that owns the list | Contributor | C2 |
| Explicit list member of a personal list | Contributor | v1.x |
| Anything else | none | — |

A principal holding access through several sources gets the strongest (`ListAccess` is ordered `Contributor < Administrator`). Access is always resolved from persisted records on every request, never from tokens, request bodies or caches.

### Rule table

The domain owns the single authorization rule table, `ListOperation::required_permission`:

| Operation | Required permission | Contributor | Administrator |
|---|---|---|---|
| List accessible lists | any access | yes | yes |
| Read items (`ReadItems`) | Contribute | yes | yes |
| Add/update/check/delete/reorder items (`EditItems`) | Contribute | yes | yes |
| Rename list (`Rename`) | Administer | no (403) | yes |
| Delete list (`Delete`) | Administer | no (403) | yes |

Creating a personal list needs only an authenticated principal, who becomes its owner. Creating household lists and administering household membership are household policies decided in C2.

### How ports express access

- `ShoppingListRepository::list_accessible(principal)` replaces `list_personal_for_owner`. It returns every list the principal can currently access: owned personal lists now, household lists from C2.
- Operations on an existing list take a `ListAccessRequest` (verified principal plus required `ListPermission`). Use cases build it from the rule table (`ListAccessRequest::for_operation`). Adapters only enforce it: they resolve the principal's effective access from persisted ownership/membership records and check it **in the same atomic operation** as the read or mutation. `rename_personal`/`delete_personal` become `rename_list`/`delete_list`; the use cases are `ListAccessibleLists`, `RenameShoppingList` and `DeleteShoppingList`.
- Error semantics, in this order:
  1. No access (or no such list) → `NotFound`. The two cases cannot be told apart.
  2. Access too weak for the permission → `Forbidden` (HTTP 403). Only principals that can already see the list get this, so it leaks nothing.
  3. Then expected-version checks (`Conflict`), then no-op detection and business limits.
- Households plug in later without changing port signatures. Adapters add the household-membership path to their access resolution, and `ShoppingList` gains an owner scope (personal or household) in C2.

### Contract scenarios

- The list and item contracts use access requests and check access-based listing. Owned lists are always included and foreign lists never are.
- A new `run_shared_access_contract` checks that a contributor can use list content but gets `Forbidden` for rename/delete, even with a stale version. It also checks that an outsider gets `NotFound` everywhere, that adapters enforce the requested permission generically, and that revoking access takes effect on the next request. Lists come from the `SharedListFixture` test-support trait. The in-memory adapter implements it with a direct contributor grant, as a stand-in until households exist. From C2, persistent adapters must implement it through real household membership, never through an access path that production code cannot reach.

### DynamoDB access patterns (sketch for A5, B1, C2)

Keys follow `docs/data-model.md`; no GSI is required.

- **List accessible lists:** Query `PK=USER#p, begins_with(SK, "LIST#")` for owned personal-list references. From C2, Query `PK=USER#p, begins_with(SK, "HOUSEHOLD#")`, then Query `PK=HOUSEHOLD#h, begins_with(SK, "LIST#")` per household. Then BatchGetItem `LIST#id / META` (consistent reads). Drop references whose metadata is missing or no longer grants access (dangling references during bounded deletion). The query is bounded by the per-household list limit (20) plus a per-user household limit to be set in C2. Household list references are **not** copied into members' `USER#` partitions, because membership changes would then fan out (ADR 005).
- **Resolve access to one list:** GetItem `LIST#id / META` (consistent read). For `type=PERSONAL`, `ownerId = p` → Administrator. For `type=HOUSEHOLD` (C2), GetItem `HOUSEHOLD#h / MEMBER#p` (consistent read): role owner → Administrator, active member → Contributor. In v1.x, also GetItem `LIST#id / MEMBER#p`.
- **Enforce atomically on mutations:** TransactWriteItems contains the item/list writes with the expected-version condition on `LIST#id / META`. For personal lists, the same `META` update adds the condition `ownerId = :p`. For household lists, a ConditionCheck on `HOUSEHOLD#h / MEMBER#p` requires the member record to exist, be active and hold a sufficient role. If the transaction is cancelled, the adapter re-reads the records to classify the result (access lost → `NotFound`/`Forbidden`; version moved → `Conflict`). It never classifies from client input.
- **Reads (`list_items`):** resolve access, then Query `PK=LIST#id, begins_with(SK, "ITEM#")`. A read running concurrently with a revocation may still succeed once; every mutation re-checks access atomically.

## Consequences

- The DynamoDB list adapter (A5) implements the ownership path and must pass the list and item contracts. It passes the shared-access contract from C2, when household membership exists.
- HTTP 403 appears only for administrative operations by contributors; all other authorization failures are 404.
- Only the rule table decides who may do what. Adapters cannot loosen permissions because the permission arrives as data.
- Until C2, the `Forbidden` branch is exercised only through the in-memory stand-in grant and unit tests.

## Alternatives

- **Application-side check through an `access_for` port, followed by unscoped mutations:** simpler ports, but a time-of-check/time-of-use gap between authorization and write (a just-removed member could still mutate), and unscoped mutation ports invite misuse. Rejected.
- **Permission implied by each port method (no permission parameter):** every adapter would re-implement the rule table. Rejected.
- **Copy household list references into every member's partition:** one-query listing, but fan-out writes on every membership change, and it duplicates membership (contrary to ADR 005). Rejected.
- **Ship explicit list sharing in v1:** doubles the authorization matrix and invitation surface before households are proven. Deferred to v1.x.
