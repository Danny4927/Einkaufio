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
- Personal list: owner or active explicit list member.
- Household: active household member.
- Household list: active member of owning household.
- Only resource owners administer membership and delete/transfer ownership.
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
- Invitation landing pages should avoid third-party scripts and set a restrictive Referrer-Policy.

## API and infrastructure

- TLS for all endpoints.
- Restrict browser CORS to exact configured origins; CORS is not authorization for mobile or non-browser clients.
- Validate payload size, string length, numeric bounds and enum values.
- Lambda IAM must grant only required DynamoDB operations on the application table and required log permissions.
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
