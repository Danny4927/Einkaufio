# ADR 017: Households, membership lifecycle and invitation token ports

- Status: Accepted
- Date: 2026-11-27
- Builds on: ADR 005 (households), ADR 008 (invitation links), ADR 015 (list access model)

## Context

Roadmap C2–C4 add households, their membership lifecycle and invitations. ADR 015 left several household policies open: who creates household lists, the per-user household limit, and how deletion cascades. ADR 008 and `docs/security.md` require invitation tokens with at least 256 bits of entropy, persisted only as SHA-256 hashes. Secure randomness and SHA-256 need external crates, but the domain and application crates must stay dependency-free (ADR 009, `scripts/check_architecture.py`).

## Decision

### Token generation and hashing behind ports

- The application layer owns three synchronous ports: `InvitationTokenGenerator` (returns `InvitationToken` or `TokenGenerationError`), `InvitationTokenHasher` (`InvitationToken` → `InvitationTokenHash`) and `Clock` (server time for expiry).
- The production adapters are in `adapters/src/invitation_tokens.rs`. `SecureInvitationTokenGenerator` fills 32 bytes (256 bits, checked at compile time) from `ring::rand::SystemRandom`, the operating-system CSPRNG, and encodes them as 43 characters of unpadded base64url. `Sha256InvitationTokenHasher` uses `ring::digest::SHA256`. `SystemClock` reads the system clock. `ring` was already in the dependency graph through rustls/jsonwebtoken, so no new crate or license enters the graph.
- An unsalted, fast hash is enough. With 256-bit tokens the stored hash cannot be brute-forced, and a deterministic hash gives a direct key lookup (`INVITE#<hash>`, docs/data-model.md).
- Use cases hash the token before calling any repository. Invitation repositories only ever receive `InvitationTokenHash`. The raw token appears in exactly two places: the one-time `CreatedInvitation` returned to the creator, and the acceptance command, which hashes and drops it at once.
- `InvitationToken`, `InvitationTokenHash` and `AcceptHouseholdInvitationCommand` implement `Debug` with redaction and no `Display`, so tokens and token hashes cannot reach logs through formatting. Use cases re-validate generated tokens, so a faulty generator cannot weaken them, and they fail closed when the random source fails.

### Household model and policies

- `ShoppingList` has an owner scope `ListOwner::{Personal(UserId), Household(HouseholdId)}`. `ShoppingList::access_for(principal, household_role)` resolves effective access: the personal owner is Administrator; for household lists the household owner is Administrator and members are Contributors. A household role never grants access to a personal list.
- The household rule table is `HouseholdOperation::required_permission`. Any active member may view the household and create household lists. Only the owner may rename, manage invitations, remove members, transfer ownership and delete.
- Limits: 10 members (owner included), 20 lists per household, and **5 households per user** (owned or joined, enforced both at creation and at acceptance). The per-user limit bounds the ADR 015 accessible-list query at 5 × 20 household lists plus personal lists.
- Ports (`application/src/household_ports.rs`) follow ADR 015: a `HouseholdAccessRequest` carries the verified principal and the required permission, and adapters enforce it atomically with the read or write. The error order is `NotFound` (no membership or no household) → `Forbidden` (a member lacking owner rights) → `Conflict` (stale version) → business rules and limits. Every port method fails closed by default with `Unavailable`. Household errors use their own `HouseholdRepositoryError`, so the existing list-port error enum and its exhaustive mappings are unchanged.
- Visible membership changes (join, leave, removal, ownership transfer) and renames advance the household version.

### Membership lifecycle (C3)

- Members leave on their own. The owner gets `OwnerMustTransferOwnership` when trying to leave or when being removed. Transfer requires an active-member target and the expected version; the previous owner stays a member. A sole owner deletes the household instead.
- Losing membership revokes list access with the same atomic write, because access is resolved from membership records on every request.

### Bounded deletion

A full household holds up to 20 lists × 100 items, plus 10 member records, references and invitations. That is far above the 100-item limit of DynamoDB `TransactWriteItems`. Deletion therefore works in two steps:

1. One conditional transaction checks the owner's role and the expected version, then marks `HOUSEHOLD#h / META` as `DELETING`. From then on, access resolution treats the household, its lists and its invitations as nonexistent (`NotFound`, `InvitationUnavailable`). The deleted household no longer counts toward its members' household limit.
2. A cleanup step removes the children in idempotent batches of at most 25 records: list items, list metadata, member and user references, invitations, and finally the household record. It may run in the same request or later, and it is safe to retry.

The port contract (`delete_household`) checks only what callers can observe: after `Ok`, everything is inaccessible and the members' limit slots are free. The in-memory adapter removes everything in one step under its single lock.

### Invitations (C4)

- Acceptance requires authentication and is one atomic operation in this order. The invitation must exist, must not be revoked or expired (expiry is exclusive at `expiresAt`) and must have uses left; otherwise `InvitationUnavailable`. A single variant covers all of these cases, so token holders learn nothing about the reason. Then `AlreadyMember` (no use consumed), `MemberLimitReached` and `HouseholdLimitReached`. Only then do the use-count increment, the membership creation and the household version change commit together.
- Lifetime defaults to 7 days (minimum 1 hour, maximum 30 days). `maxUses` defaults to 5 (maximum 10). At most 5 valid invitations may exist per household; used-up, revoked and expired invitations do not count.

### Test-support access path removed

ADR 015's `SharedListFixture` stand-in (a direct contributor grant) is removed. The shared-access and shared-query contracts, and the API route tests, provision contributors through production ports only: the owner creates a household and an invitation, the contributor accepts it, and a household list is created.

## Consequences

- Contract suites `run_household_contract`, `run_membership_lifecycle_contract` and `run_invitation_contract` apply to every adapter. The DynamoDB adapter must pass them and must implement the conditional transactions above. Race tests (concurrent acceptances, revoke against accept, concurrent transfers) currently cover the in-memory adapter and must be repeated for DynamoDB.
- `ShoppingList::owner_id()` was replaced by `owner()` and `personal_owner()`.
- HTTP routes, the OpenAPI contract, stable invitation URLs (Android App Links) and the web fallback are still open within C4.

## Alternatives

- **Hashing or random generation in the domain or application crates:** would need external crates there and violate the enforced architecture. Rejected.
- **Salted or slow password hashing (Argon2, bcrypt):** protects low-entropy secrets and prevents direct key lookup. It adds nothing for 256-bit random tokens. Rejected.
- **Distinct errors for expired, revoked and used-up invitations:** better UX, but creates an oracle for token holders. Rejected; the UI shows one "invitation no longer valid" message.
- **Single-transaction household deletion:** impossible at full capacity in DynamoDB. Rejected in favour of mark-then-sweep.
- **Owner-only household list creation:** simpler, but blocks everyday collaboration. Members may create lists; only the owner renames or deletes them (ADR 015 rule table).
