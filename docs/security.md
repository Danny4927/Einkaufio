# Einkaufio – Security model

## Trust boundaries

- Angular and Android are untrusted public clients.
- Cognito authenticates users; the Rust API authorizes every resource access.
- DynamoDB is private and accessible only through the Lambda execution role.
- Invitation URLs carry bearer secrets and must be treated as sensitive.

## Authentication

- Require a valid Cognito access token for every application API route except minimal health.
- Derive the issuer and JWKS URL only from trusted AWS region and user-pool configuration.
- Accept RS256 only and verify signatures against Cognito JWKS. Never follow token-provided jku or x5u URLs.
- Validate issuer, expiry, not-before when present, token_use=access, configured app client_id, and non-empty sub.
- Cache JWKS for a bounded period and refresh on an unknown key ID to support rotation.
- Fail closed when configuration or JWKS retrieval is unavailable.
- Derive caller ID from verified sub; never authorize from user IDs in request bodies.
- Do not embed Cognito app-client secrets in browser or Android binaries.

## Authorization

- Centralize authorization in application services.
- Personal list: owner or active explicit list member.
- Household: active household member.
- Household list: active member of owning household.
- Only resource owners administer membership and delete/transfer ownership.
- Re-check persisted membership for every protected request.
- Avoid disclosing inaccessible resource existence.

## Invitation tokens

- Generate at least 256 bits of cryptographically secure randomness.
- Store only SHA-256(token); never store raw token.
- Return raw token once over TLS.
- Enforce expiry, revocation and use count on the server.
- Use conditional DynamoDB transactions for use count plus membership creation.
- TTL is asynchronous cleanup only; always check expiresAt in the API.
- Never log tokens, full invite URLs or token hashes.
- Invitation landing pages should avoid third-party scripts and set a restrictive Referrer-Policy.

## API and infrastructure

- Use TLS for all endpoints.
- Restrict browser CORS to exact configured origins; CORS is not authorization for mobile/non-browser clients.
- Validate payload sizes, string lengths, numeric bounds and enum values.
- Lambda IAM grants only required DynamoDB and log permissions.
- Avoid VPC/NAT unless a documented requirement appears.
- Function URL AuthType NONE is acceptable only because application routes validate Cognito JWTs.
- Add request-level limits and abuse controls.
- Set CloudWatch retention; redact personal data and secrets.
- AWS Budgets/alerts notify but do not enforce a hard spend cap.

## Client security

### Angular PWA

- Use a maintained OIDC/Cognito library.
- Prefer authorization-code flow with PKCE where supported.
- Avoid long-lived secrets in localStorage.
- Use CSP/security headers via CloudFront response headers policy.
- Never send access tokens or invite secrets to analytics.

### Future Android

- Use a supported public-client flow with PKCE.
- Store refresh credentials using Android Keystore-backed secure storage.
- Configure verified Android App Links for invitation URLs.
- Never embed confidential client secrets.
- Reuse the same API authorization model.

## Data lifecycle and recovery

- Define deletion behavior for users, households, lists, memberships and invitations.
- Household owner transfer must precede owner departure.
- Choose backup/PITR before production data becomes important.
- Document restore tests, not only backup configuration.
