# Local development

This guide describes how to prepare a workstation, run the repository checks and configure local environment values. The verification targets are defined in the root `Makefile`; CI runs the same targets (`.github/workflows/ci.yml`).

## Prerequisites

| Tool | Version / notes | Used by |
|---|---|---|
| Git | Recent version | Repository, pre-commit hook |
| Rust | Stable toolchain with `rustfmt` and `clippy` components | `make fmt`, `make lint`, `make test*` |
| cargo-deny | `cargo install cargo-deny --locked`; use the version pinned in CI | `make deny` |
| Python | 3.12 or newer | `make architecture`, `make agent-config-check` |
| yamllint | 1.35.1 (`python -m pip install yamllint==1.35.1`) | `make yaml-lint`, `make ci-contracts` |
| Terraform | 1.10.5 or newer | `make terraform-fmt`, `make terraform-validate` |
| Node.js and npm | Node 24.15.0 (see `apps/web/.nvmrc`); npm pinned to 10.9.2 in CI | `make web-*`, `make openapi-lint` |
| jq, ShellCheck | Optional locally; installed in CI | `make agent-config-check` |
| Checkov | Optional; used by `make terraform-scan` | `make terraform-scan` |

`make web-e2e-setup` installs the Playwright Chromium browser without installing OS packages or requiring root. Playwright's system dependencies on Linux are not installed by that target; CI uses `npx playwright install --with-deps chromium`.

## Commands

Run all commands from the repository root.

### Backend (Rust)

| Command | What it does |
|---|---|
| `make fmt` | Checks Rust formatting (`cargo fmt --check`) |
| `make lint` | Runs Clippy over the workspace with `-D warnings` |
| `make test` / `make test-workspace` | Runs workspace tests |
| `make test-domain` | Runs the domain crate tests |
| `make test-application` | Runs the application crate tests |
| `make architecture` | Runs architecture boundary unit tests and `scripts/check_architecture.py` |
| `make deny` | Runs `cargo deny check` against `backend/Cargo.toml` using `backend/deny.toml` |
| `make ci-backend` | Format, lint, workspace tests and architecture checks |

### Repository checks

| Command | What it does |
|---|---|
| `make check` | Fast backend checks, architecture checks, Terraform formatting and YAML lint |
| `make verify` | Full local verification: backend, web, Terraform, contracts and agent configuration |
| `make terraform-fmt` | Checks Terraform formatting |
| `make terraform-validate` | Initializes each Terraform root without a backend and validates it |
| `make terraform-scan` | Runs Checkov over `infra/terraform` |
| `make yaml-lint` | Runs strict yamllint over the repository |
| `make openapi-lint` | Lints `contracts/openapi/openapi.yaml` with Redocly |
| `make ci-contracts` | YAML lint plus OpenAPI lint |
| `make agent-config-check` | Validates Claude settings and hook behaviour |

### Web (Angular PWA)

Web dependencies are locked in `apps/web/package-lock.json`. Install them with `npm ci` from `apps/web`, or use the targets below.

| Command | What it does |
|---|---|
| `make web-install` | `npm ci --no-audit --no-fund` in `apps/web` |
| `make web-lint` | Runs `npm run lint` |
| `make web-test` | Runs unit tests (`npm test`) |
| `make web-build` | Builds the production bundle |
| `make web-e2e-setup` | Installs the Playwright Chromium browser |
| `make web-e2e` | Runs Playwright end-to-end tests |

## Environment configuration

The backend reads its Cognito configuration from environment variables when it builds the token verifier (`backend/crates/api/src/auth.rs`, `CognitoConfig::from_env`):

| Variable | Required | Purpose |
|---|---|---|
| `AWS_REGION` | Yes | AWS region of the user pool, for example `eu-central-1` |
| `COGNITO_USER_POOL_ID` | Yes | User pool ID; must start with the region, for example `eu-central-1_EXAMPLE` |
| `COGNITO_APP_CLIENT_ID` | Yes | Cognito app client ID that issues the access tokens |

If any variable is missing, empty or inconsistent with the region, the backend fails closed with an unavailable authentication configuration. It does not fall back to permissive defaults.

Rules for local configuration:

- Use placeholder or development-pool values only. Never commit real secrets, `.env` files or `*.tfvars` files; `.gitignore` excludes them.
- Do not embed Cognito app-client secrets in browser or Android code (see `docs/security.md`).
- Do not put access tokens, invitation secrets or full invite URLs into logs or shell history that you share.

## Troubleshooting

- A check that cannot run locally (for example `make deny` without network access to the advisory database) must be reported in the pull request with the reason and impact. Do not claim it passed.
- Terraform commands need the Terraform version listed above; newer or older versions may format or validate differently.
