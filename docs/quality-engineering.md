## Agent workflow additions

- Use the root `AGENTS.md` as the concise entry point; keep detailed policy in this document and the linked architecture/security documents.
- `make verify` is the local CI-equivalent command; `make check` intentionally remains the faster backend/architecture/Terraform-format loop.
- Each repository port contract should have reusable behavioral scenarios. Run the same scenarios against the in-memory fake and every production adapter. Cover authorization-before-existence-disclosure, fail-closed behavior, atomic expected-version comparison (including stale no-op rename), item/list version atomicity, item capacity/position assignment, and repeated-delete semantics.
- Do not introduce a `VerifiedActor` marker until verified Cognito claims are represented at the API trust boundary. Its constructor must not be reachable from request DTOs or arbitrary strings.
- Agent hooks and repository permission files are defense-in-depth only. CI, branch protection, code review and protected environments remain authoritative.
