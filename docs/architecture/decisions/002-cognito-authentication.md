# ADR-002: Cognito for user accounts

- Status: Accepted
- Date: 2026-11-24

## Context
All users must have an account. Web and future Android need one identity system.

## Decision
Use Amazon Cognito User Pool. Cognito `sub` is the canonical user ID. Use public-client authentication flows; never embed client secrets in apps. API validates Cognito access-token signature, issuer, expiry, token use and configured client claims.

## API verification requirements\n- Derive the issuer and JWKS URL only from configured AWS region and user-pool ID.\n- Accept RS256 access tokens only; verify the signature using Cognito JWKS.\n- Validate `iss`, `exp`, optional `nbf`, `token_use=access`, configured `client_id`, and a non-empty `sub`.\n- Cache keys for a short bounded period and refresh on unknown `kid` to support rotation.\n- Missing configuration or JWKS service failures deny requests; do not provide a development bypass in production.\n\n## Consequences
- One account works across clients.
- Password handling is delegated to Cognito.
- Application profile remains in DynamoDB.
- Email/SMS and advanced features may cost extra; check current pricing and quotas.
- Token refresh and secure token storage are client responsibilities.

## Alternatives
Custom password storage was rejected due to security/maintenance burden. Separate identity systems per client were rejected to avoid fragmented accounts.
