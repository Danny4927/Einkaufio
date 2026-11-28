# Einkaufio – Security model

## Trust boundaries

- Angular and Android are untrusted public clients.
- Cognito authenticates users; the Rust API authorizes every resource access.
- DynamoDB is private and accessible only through the Lambda execution role.
- Invitation URLs carry bearer secrets and must be treated as sensitive.

## Authentication

- Require a valid Cognito access token for all application API routes except minimal health.
- Validate RS256 JWT signatures using Cognito JWKS; derive issuer/JWKS URL only from trusted AWS region and user-pool configuration.
- Validate issuer, exp, optional nbf, iat, token_use=access, configured client_id and non-empty sub.
- Never follow token-provided jku/x5u URLs. Bound JWKS refresh frequency, serialize concurrent refreshes and support signing-key rotation.
- Fail closed when configuration or JWKS retrieval is unavailable; document any stale-key grace period explicitly.
- Derive caller ID from verified `sub`; never authorize based on user IDs in request bodies.
- Do not embed Cognito app-client secrets in browser or Android binaries.
- Use supported public-client OIDC flows; use PKCE for native authorization-code flow where applicable.
- Keep token refresh and secure storage platform-specific.

## Authorization

- Centralize authorization in application services.
- Personal list: owner. Explicit list members are deferred to v1.x (ADR 015).
- Household: active household member.
- Household list: active member of owning household.
- Effective list access is Administrator (personal-list owner, owner of the owning household) or Contributor (active household member). Contributors read and edit items; only administrators rename or delete a list. The rule table lives in the domain (`ListOperation::required_permission`, ADR 015).
- No access returns 404, indistinguishable from a missing resource; access that is too weak returns 403. Authorization is checked before expected versions.
- Enforce persisted access atomically with the mutation (conditional writes/transactions), never as a separate earlier check.
- Only resource owners administer membership and delete/transfer ownership.
- Household rule table (`HouseholdOperation::required_permission`, ADR 017): every active member views the household and its members and creates household lists; only the owner renames, deletes, manages invitations, removes members and transfers ownership. Non-members get 404, members lacking owner rights 403 (ADR 018 route table).
- `route_tests/authorization_matrix.rs` checks every documented endpoint for unauthenticated, outsider, member and owner callers, asserts that an outsider's 404 is byte-identical to the answer for a nonexistent id, and fails if the OpenAPI contract lists an operation the matrix does not cover.
- Re-check persisted membership for every protected request.
- Avoid disclosing the existence of inaccessible resource IDs.

## Invitation tokens

- Generate at least 256 bits cryptographically secure randomness.
- Store only SHA-256(token); never store raw token.
- Return raw token once, over TLS.
- Enforce expiry, revocation and use count on server.
- Use conditional DynamoDB transactions for use count plus membership creation.
- TTL is asynchronous cleanup only; API always checks expiresAt.
- Never log tokens, full invite URLs or token hashes.
- The DynamoDB adapter keys invitations by the hex SHA-256 hash (`INVITE#<hash>`), so keys and SDK errors are secret-adjacent: the adapter maps every SDK error to `Unavailable` without request details and never logs keys (ADR 019).
- Invitation landing pages should avoid third-party scripts and set a restrictive Referrer-Policy.
- Invitation URLs are `{PUBLIC_APP_BASE_URL}/invite#token={token}` (ADR 018). The token sits in the URL fragment, which browsers never send to servers or put into `Referer`; the web client removes it from the address bar after reading it.
- The API accepts the token only in the JSON body of `POST /api/invites/accept`, never in a path or query string, so it cannot reach access logs. The request DTO does not implement `Debug`.
- `PUBLIC_APP_BASE_URL` is trusted configuration validated at startup (HTTPS only, no query, fragment or credentials); invitation links are never built from request headers.
- The creation response is the only place the token appears and carries `Cache-Control: no-store`. Invitation listings never contain the token, the URL or the hash.
- Unknown, malformed, revoked, expired and used-up tokens get one identical `410 invitation_unavailable` response that does not echo the token.

## API and infrastructure

- TLS for all endpoints.
- Restrict browser CORS to exact configured origins; CORS is not authorization for mobile or non-browser clients.
- Validate payload size, string length, numeric bounds and enum values.
- Lambda IAM must grant only required DynamoDB operations on the application table and required log permissions.
- The DynamoDB adapter needs the IAM actions `GetItem`, `PutItem`, `UpdateItem`, `DeleteItem`, `ConditionCheckItem`, `Query`, `BatchGetItem` and `BatchWriteItem` on the application table (IAM authorizes `TransactGetItems`/`TransactWriteItems` through the per-item actions). It never needs `Scan`, `CreateTable` or index access (ADR 019).
- Avoid VPC/NAT unless a documented requirement appears.
- Function URL AuthType NONE is acceptable only because application routes validate Cognito JWTs.
- Function URL does not include API Gateway usage plans; add request-level limits and abuse controls.
- Set CloudWatch retention; redact personal data and secrets.
- AWS Budgets/alerts notify but do not enforce a hard spend cap.

## Client security

### Angular PWA
- Use a maintained OIDC/Cognito library.
- Prefer authorization-code flow with PKCE where supported.
- Avoid putting long-lived secrets in localStorage.
- Use CSP/security headers via CloudFront response headers policy.
- Do not send access tokens or invite secrets to analytics.

### Future Android
- Use a supported public-client flow with PKCE.
- Store refresh credentials using Android Keystore-backed secure storage.
- Configure verified Android App Links for invitation URLs.
- Do not embed confidential client secrets.
- Reuse the same API authorization model.

## Data lifecycle and recovery

- Define deletion behavior for user, household, lists, memberships and invitations.
- Household owner transfer must precede owner departure.
- Choose backup/PITR before production data becomes important.
- Document restore tests, not only backup configuration.
- Provide account deletion (including memberships and invitations) and a personal data export before production (GDPR; roadmap Phase D2).
- Account deletion (`DELETE /api/me`, ADR 018) acts only on the verified principal's data: it deletes households the caller owns alone, ends other memberships, deletes personal lists with items, removes invitations the caller created and finally the profile. An owner of a household with other members is refused (`409 owner_must_transfer_ownership`) before anything is deleted, so other members never lose shared lists. Steps are idempotent and repeatable after a conflict or outage. Items the caller added to household lists remain with the household and keep the opaque `createdBy` subject. Deleting the Cognito user is a separate client step (Cognito `DeleteUser`).
- Data export (`GET /api/me/export`) returns the profile, memberships and all lists the caller can currently access with their items, read through the same access-scoped ports as every request; it is sent with `Cache-Control: no-store` and never contains invitation tokens or hashes.
