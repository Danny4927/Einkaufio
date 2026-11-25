## Summary

<!-- What changed and why? -->

## Acceptance criteria

- [ ] Each acceptance criterion is addressed.

## Verification

- [ ] Focused tests/checks run.
- [ ] make verify run locally, or unavailable checks are explained below.
- [ ] Required GitHub Actions CI checks are green.

## Security and architecture

- [ ] Actor identity is derived only from verified Cognito claims.
- [ ] Authorization happens before resource existence can be disclosed.
- [ ] Repository operations fail closed.
- [ ] Expected-version checks and multi-record mutations are atomic.
- [ ] Domain/application dependency boundaries remain intact.
- [ ] No secrets, tokens, or invitation material are logged.

## Documentation and evidence

- [ ] docs/roadmap.md updated when implementation status changes.
- [ ] ADR/security/OpenAPI/quality docs updated where applicable.
- [ ] UI screenshots/visual diffs included for visual changes.
- [ ] Terraform plan and risk review included for infrastructure changes.

## Limitations

<!-- List skipped checks, known limitations, migration/rollback considerations. -->
