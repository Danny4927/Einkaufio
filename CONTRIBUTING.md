# Contributing to Einkaufio

Thank you for considering a contribution. This document describes the workflow. Detailed decisions and policies live in `docs/`; start with `AGENTS.md` for repository conventions.

## Before you start

- Read `docs/roadmap.md`. It is the implementation source of truth; pick work from the current phase and keep its status accurate.
- Read `docs/security.md` and `docs/architecture/arc42.md` before changing authorization, identity, persistence or infrastructure.
- Set up your environment as described in `docs/local-development.md`.

## Branches and pull requests

- Branch from `main`. Use `feat/<short-topic>` for feature work so that CI runs on push (CI triggers on `main` and `feat/**`).
- Keep pull requests focused. One logical change per PR; cross-component contract changes (OpenAPI, backend, web) go in one coordinated PR.
- Use the pull request template in `.github/pull_request_template.md`. List acceptance criteria with evidence, the commands you ran and their results, and anything you could not run.
- Never force-push a shared branch. Do not merge a PR with failing CI.

## Commits

- Make small, meaningful commits with imperative subject lines.
- Do not commit secrets, `.env` files, Terraform state or `*.tfvars` files.

## Local verification

Run the narrowest useful target while iterating, then the full local verification before you ask for review:

| Command | Purpose |
|---|---|
| `make test-domain` / `make test-application` | Focused Rust feedback loops |
| `make fmt` / `make lint` | Rust formatting and Clippy (warnings are errors) |
| `make deny` | Dependency licence, advisory, ban and source policy (`backend/deny.toml`) |
| `make check` | Fast backend checks, architecture checks and Terraform formatting |
| `make verify` | Full local verification through the same targets CI runs |
| `make ci-contracts` | Strict YAML lint and OpenAPI lint |

If a check cannot run in your environment, say so in the PR and explain the impact. Never claim a check passed without observing it.

## Architecture and decisions

- Keep the domain and application crates free of HTTP, AWS SDK, DynamoDB and transport dependencies. The architecture tests enforce this.
- Record architectural changes as an ADR in `docs/architecture/decisions/`, following the existing numbering. Change an accepted decision through a superseding ADR rather than editing it silently.
- Update the OpenAPI contract (`contracts/openapi/`), security notes, and quality evidence when they are affected.

## Roadmap and progress

- Update `docs/roadmap.md` when implementation state changes. Do not create parallel progress trackers.

## Security

- Never log access tokens, invitation secrets, full invite URLs or token hashes.
- Do not trust client-supplied owner or actor IDs; actor identity comes from verified Cognito claims.
- Report suspected vulnerabilities privately to the maintainers rather than in a public issue.

## Licence

By contributing, you agree that your contributions are licensed under the MIT License in `LICENSE`.
