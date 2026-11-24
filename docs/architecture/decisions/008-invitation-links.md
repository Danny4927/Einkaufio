# ADR-008: Secure invitation links

- Status: Accepted
- Date: 2026-11-24

## Context
Users share lists/households over messaging apps without needing invitee email addresses.

## Decision
Generate high-entropy random tokens. Store only SHA-256 hash, target, creator, expiry, revocation and usage limit. Invitee must authenticate and explicitly accept. Enforce expiry and use limits in conditional transactions; TTL is cleanup only.

## Consequences
- Links work with WhatsApp, Signal, email and copy/paste.
- Treat URL token as bearer secret; keep it out of logs, analytics and referrers.
- Stable HTTPS invitation URL supports web fallback and future Android App Links.
- Invitation does not grant access until acceptance creates membership.

## Alternatives
Email-bound invitations add delivery complexity. Permanent share URLs create uncontrolled access.
