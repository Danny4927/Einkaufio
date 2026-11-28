# ADR 018: Household HTTP routes, invitation URLs, account deletion and data export

- Status: Accepted
- Date: 2026-11-27
- Builds on: ADR 008 (invitation links), ADR 015 (list access model), ADR 017 (households and invitation ports)

## Context

Roadmap C2–C4 left the HTTP routes, the OpenAPI contract and stable invitation URLs open. Roadmap D2 requires account deletion (including memberships and invitations) and a personal data export before production. Open questions were:

- how the API builds invitation URLs that work for the web client and future Android App Links without putting the bearer token where servers or referrers record it;
- what happens to households a user owns when the account is deleted;
- which data the export contains;
- which port additions this needs while a DynamoDB adapter is being written against the same ports.

## Decision

### Routes

All routes are below `/api`, require a verified Cognito principal and are throttled per principal (ADR 016). Authorization follows ADR 015/017: no membership is `404`, a member lacking owner rights is `403`, then `409` for stale versions and business rules.

| Route | Who |
|---|---|
| `GET/POST /households` | any authenticated user (own households; creator becomes owner) |
| `GET /households/{id}`, `GET /households/{id}/members` | members |
| `PATCH /households/{id}` (body `expectedVersion`), `DELETE /households/{id}?expectedVersion=` | owner |
| `POST /households/{id}/lists` | members |
| `DELETE /households/{id}/members/{userId}?expectedVersion=` | owner (removing the owner is `409 owner_must_transfer_ownership`) |
| `POST /households/{id}/leave` | members except the owner (`409`) |
| `POST /households/{id}/transfer-ownership` (body `newOwnerId`, `expectedVersion`) | owner |
| `GET/POST /households/{id}/invites`, `DELETE /households/{id}/invites/{inviteId}` | owner |
| `POST /invites/accept` (body `token`) | any authenticated user |
| `DELETE /me`, `GET /me/export` | the caller's own data |

- Household deletion answers `204`: when the port returns, everything is inaccessible (ADR 017); a background sweep, if any, is invisible to clients.
- Business-rule conflicts have stable error codes: `household_limit_reached`, `member_limit_reached`, `list_limit_reached`, `invitation_limit_reached`, `already_member`, `owner_must_transfer_ownership`.
- Every unknown, malformed, revoked, expired or used-up invitation token gets the same `410 invitation_unavailable` with the same body (no oracle, ADR 017).
- List responses gain `type` (`PERSONAL`/`HOUSEHOLD`) and `householdId`, so clients can tell household lists apart. The unimplemented `role`, `updatedAt`, `memberCount` and member `displayName` fields were removed from the contract; adding them later is backwards compatible.

### Invitation URLs

- The creation response (`201`, `Cache-Control: no-store`) contains the raw token once and `inviteUrl = {PUBLIC_APP_BASE_URL}/invite#token={token}`.
- The token is in the URL **fragment**. Browsers never send fragments to servers, so the token does not reach CloudFront or Lambda logs and never appears in a `Referer` header. The path `/invite` is stable, so Android App Links can claim it; Android passes the full URI including the fragment to the app.
- The web client reads the fragment, removes it from the address bar (`history.replaceState`) and calls `POST /api/invites/accept` with the token in the JSON body. The API never accepts the token in a path or query string.
- `PUBLIC_APP_BASE_URL` is trusted configuration, validated at startup (`InvitationBaseUrl`: HTTPS only, no query, fragment or credentials); the composition root aborts on error like for Cognito. The API never derives it from request headers (no `Host`-header injection into invitation links).
- Invitation listings never contain the token, the URL or the hash; they show a server-computed `status` (`ACTIVE`, `REVOKED`, `EXPIRED`, `USED_UP`).

### Account deletion policy (`DELETE /api/me`)

1. If the caller owns a household that has **other active members**, the request fails with `409 owner_must_transfer_ownership` before anything is deleted. The owner first transfers ownership or deletes the household explicitly. Other members never lose shared lists as a side effect of someone else's account deletion.
2. Households the caller owns alone are deleted with their lists, items and invitations.
3. The caller leaves every other household.
4. The caller's personal lists are deleted with their items.
5. Invitations the caller created are removed in every household (including households whose ownership the caller transferred earlier).
6. The profile is deleted last.

Each step is one atomic port operation with the current expected version. Steps are idempotent (`NotFound` for an already removed record counts as done). A concurrent change (for example someone joining a household that is being deleted) yields `409 version_conflict`; repeating the request completes the deletion. An outage yields `503`, and the profile still exists, so the incomplete deletion stays visible.

Items the caller added to household lists stay with the household: they are shared content of the remaining members. Their `createdBy` keeps the caller's opaque Cognito subject, which no longer resolves to a profile. Deleting the Cognito user itself is a client step after `DELETE /api/me` (Cognito `DeleteUser` with the access token) until an identity-provider port exists; it is not part of this ADR.

### Data export (`GET /api/me/export`)

A JSON document (`Content-Disposition: attachment`, `Cache-Control: no-store`, `formatVersion: 1`) with the profile (or `null`), the caller's household memberships with role, and every list the caller can currently access (personal and household lists) with all items including `createdBy`. It is assembled through the same access-scoped ports as every other read, so it contains nothing the caller could not read anyway. It is not one consistent snapshot; a list whose access ends between reads is left out. Invitation records are not exported: they contain no data provided by the caller beyond limits and ids, and the token is never stored.

### Port additions

Both have fail-closed defaults (`Unavailable`), so existing adapters compile unchanged and refuse account deletion until they implement them:

- `ProfileRepository::delete_profile(principal)`: `NotFound` if there is none.
- `InvitationRepository::delete_invitations_created_by(principal)`: removes every invitation the principal created, idempotent.

The use cases `DeleteAccount` and `ExportAccountData` orchestrate existing access-scoped ports; there is no global "delete everything for user X" port, so adapters do not re-implement the policy. `run_account_data_contract` covers the two new port methods.

## Consequences

- The DynamoDB adapter must implement `delete_profile` (DeleteItem `USER#p / PROFILE`) and `delete_invitations_created_by`. The latter needs a way to find a user's invitations; the access pattern (for example a `USER#p / INVITE#id` reference written in the invitation-creation transaction) is decided in the adapter work.
- The authorization matrix test (`route_tests/authorization_matrix.rs`) covers every documented operation × {unauthenticated, outsider, member, owner} and fails when the OpenAPI contract and the matrix diverge.
- The composition root (A4) must provide `HouseholdServices` with the production token generator, hasher, clock and `PUBLIC_APP_BASE_URL`; Terraform must set the variable to the CloudFront domain.
- The web route `/invite` (fragment handling, sign-in redirect, accept call) and the Android `assetlinks.json` remain open (roadmap C4/C6, Android milestone).

## Alternatives

- **Token in the URL path or query (`/invite/{token}`):** simpler for clients, but the token would reach CloudFront access logs and could leak through `Referer`. Rejected.
- **Automatic ownership transfer on account deletion (for example to the longest-standing member):** no blocked deletions, but hands administrative rights to someone who did not ask for them. Rejected.
- **Deleting owned households with members on account deletion:** silently destroys other members' shared lists. Rejected.
- **One `delete_account` port method per adapter:** atomic only within limits a DynamoDB transaction cannot meet anyway, and every adapter would re-implement the policy. Rejected in favour of idempotent orchestration in the application layer.
- **Base URL from the request `Host` header:** spoofable, would let an attacker make the API mint links to a foreign host. Rejected.
