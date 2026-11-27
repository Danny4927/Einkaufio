# ADR-002: Cognito for user accounts

- Status: Accepted
- Date: 2026-11-24

## Context
All users must have an account. Web and future Android need one identity system.

## Decision
Use Amazon Cognito User Pool. Cognito `sub` is the canonical user ID. Use public-client authentication flows; never embed client secrets in apps. API validates Cognito access-token signature, issuer, expiry, token use and configured client claims.

## API verification requirements
- Derive issuer and JWKS URL only from configured AWS region and user-pool ID.
- Accept RS256 access tokens only; validate signature, iss, exp, optional nbf, iat, token_use=access, client_id and non-empty sub.
- Never follow jku/x5u from token headers. Serialize concurrent JWKS refreshes and enforce a global 30-second minimum refresh interval, including failed refresh attempts, to limit random-kid amplification.
- Do not use stale keys after the 5-minute cache expires: a JWKS outage fails closed with 503. The refresh cooldown may reject an unknown key ID for up to 30 seconds; this is the deliberate trade-off for outbound-request protection during key rotation.
- Missing configuration or JWKS failures deny requests; no development bypass.
- A JWKS fetch (3-second timeout) runs while holding the refresh lock, so concurrent requests needing a refresh wait for that single fetch instead of issuing their own.
- Clock skew of 30 seconds applies uniformly to exp, nbf and iat. `aud` is not validated; app-client binding uses `client_id`.
- `router()` never reads the environment. The composition root builds state via `ApiState::from_cognito_env` and must abort startup on configuration errors.

## Consequences
- One account works across clients.
- Password handling is delegated to Cognito.
- Application profile remains in DynamoDB.
- Email/SMS and advanced features may cost extra; check current pricing and quotas.
- Token refresh and secure token storage are client responsibilities.

## Alternatives
Custom password storage was rejected due to security/maintenance burden. Separate identity systems per client were rejected to avoid fragmented accounts.
