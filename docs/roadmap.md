# Einkaufio – Implementation roadmap

## v1 product scope

- Required accounts and sign-in.
- Personal shopping lists with explicit sharing.
- Households with owner/member roles and multiple household-owned lists.
- Item create/update/delete/check/reorder.
- Secure invitation links.
- Version-based polling (15 seconds while list view is visible).
- Responsive Angular PWA.
- Platform-neutral API that supports a future Android client.

### Initial product limits (configurable)
- 10 members per household.
- 20 lists per household.
- 100 active items per list.
- 5 simultaneously valid invitations per resource.
- Invitation lifetime default 7 days, maximum 30 days.
- Invitation maxUses default 5, maximum 10.

These are initial product/abuse-control limits, not permanent technical constraints.

## Milestones and GitHub-ready work packages

### M0 – Repository foundation
- [ ] Add license and contribution guidelines.
- [ ] Add ADR and OpenAPI validation to CI.
- [ ] Define Rust workspace and Angular app structure.
- [ ] Add local development and environment configuration docs.

### M1 – AWS infrastructure
- [ ] Bootstrap Terraform state S3 bucket separately.
- [ ] Create private S3 frontend bucket.
- [ ] Create CloudFront distribution and OAC.
- [ ] Create ARM64 Lambda custom runtime and Function URL.
- [ ] Create DynamoDB PK/SK table.
- [ ] Create Cognito User Pool and public app client.
- [ ] Define least-privilege Lambda IAM.
- [ ] Configure CloudWatch log retention and alarms.
- [ ] Configure AWS Budget notifications.
- [ ] Configure GitHub OIDC deployment role.

### M2 – Authentication/profile
- [ ] Cognito register/sign-in/password reset.
- [ ] Rust JWT verification with JWKS caching and key rotation.
- [ ] Derive principal from verified sub.
- [ ] Create/read/update profile.
- [ ] Angular auth flow and protected routes.
- [ ] Tests for invalid, expired and wrong-use JWTs.

### M3 – Personal lists/items
- [ ] Create/list/get/rename/delete personal lists.
- [ ] Create/update/delete/check/reorder items.
- [ ] Item and list version transactions.
- [ ] Pagination and input validation.
- [ ] Angular personal-list overview and editor.
- [ ] Owner/member/unauthorized and concurrency tests.

### M4 – Households
- [ ] Create/list/get/rename household.
- [ ] Household membership model.
- [ ] Create/list household-owned lists.
- [ ] Owner/member policies.
- [ ] Leave household and owner transfer.
- [ ] Define bounded deletion/cascade strategy and test it.

### M5 – Sharing
- [ ] Cryptographic invitation tokens.
- [ ] Store token hash only.
- [ ] Invite to personal list and household.
- [ ] Authenticated acceptance.
- [ ] Atomic use count, revocation and expiry.
- [ ] Web invitation route and fallback.
- [ ] Stable HTTPS URLs compatible with future Android App Links.

### M6 – Synchronization/PWA
- [ ] Version-based sync endpoint.
- [ ] Poll only visible active list at 15 seconds.
- [ ] Pause background tab; refresh on focus/reconnect.
- [ ] Optimistic UI and 409 conflict handling.
- [ ] PWA installability and service worker.
- [ ] Define offline semantics; do not silently queue conflicting writes.

### M7 – Quality/release
- [ ] Rust fmt, Clippy and unit/integration tests.
- [ ] OpenAPI lint and compatibility check.
- [ ] Angular lint and tests.
- [ ] Playwright E2E tests.
- [ ] Authorization matrix tests.
- [ ] Dependency and secret scanning.
- [ ] Deployed smoke tests.
- [ ] Backup/restore and incident documentation.
- [ ] Cost review for actual AWS account/region.

## Future Android milestone (not v1)
- [ ] Validate product demand and select native stack (Kotlin + Jetpack Compose candidate).
- [ ] Cognito public-client flow with PKCE.
- [ ] Generate/consume API models from the same OpenAPI contract.
- [ ] Keystore-backed secure token storage.
- [ ] Android App Links for invitation URLs.
- [ ] Native navigation and Android share sheet.
- [ ] Reuse same backend authorization and domain rules.
- [ ] Android UI and integration tests.
- [ ] Decide push notifications only if polling is insufficient.

## Definition of Done
- CI checks pass.
- OpenAPI schema validates and breaking changes are reviewed.
- New endpoints have authorization tests for owner/member/non-member.
- Multi-record writes are atomic or have documented consistency semantics.
- No tokens/invite secrets appear in logs.
- Terraform plan is reviewed and cost-affecting resources understood.
- Deployment and rollback are documented.
- AWS pricing, free-tier eligibility and quotas are verified for target account.
