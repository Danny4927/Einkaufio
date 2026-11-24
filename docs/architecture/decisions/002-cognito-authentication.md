# ADR-002: Cognito for user accounts

- Status: Accepted
- Date: 2026-11-24

## Context
All users must have an account. Web and future Android need one identity system.

## Decision
Use Amazon Cognito User Pool. Cognito `sub` is the canonical user ID. Use public-client authentication flows; never embed client secrets in apps. API validates Cognito access-token signature, issuer, expiry, token use and configured client claims.

## Consequences
- One account works across clients.
- Password handling is delegated to Cognito.
- Application profile remains in DynamoDB.
- Email/SMS and advanced features may cost extra; check current pricing and quotas.
- Token refresh and secure token storage are client responsibilities.

## Alternatives
Custom password storage was rejected due to security/maintenance burden. Separate identity systems per client were rejected to avoid fragmented accounts.
