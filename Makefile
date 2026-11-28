SHELL := /bin/bash
WEB_DIR := apps/web
TERRAFORM_DIRS := $(shell find infra/terraform -type f -name '*.tf' -exec dirname {} \; | sort -u)
PLAYWRIGHT_INSTALL ?= npx playwright install chromium

.PHONY: check verify ci-backend ci-web ci-terraform fmt lint test test-domain test-application test-workspace test-dynamodb architecture deny terraform-fmt terraform-validate terraform-scan web-install web-lint web-test web-build web-e2e web-e2e-setup agent-config-check yaml-lint openapi-lint ci-contracts

check: fmt lint test-workspace architecture terraform-fmt yaml-lint
verify: ci-backend ci-web ci-terraform ci-contracts agent-config-check
ci-backend: fmt lint test-workspace architecture
ci-web: web-install web-lint web-test
	cd $(WEB_DIR) && npm run build
	cd $(WEB_DIR) && $(PLAYWRIGHT_INSTALL)
	cd $(WEB_DIR) && npm run e2e
ci-terraform: terraform-fmt terraform-validate

fmt:
	cargo fmt --manifest-path backend/Cargo.toml --all -- --check
lint:
	cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings
	cargo clippy --manifest-path backend/Cargo.toml -p einkaufio-adapters --all-targets --features dynamodb,test-support -- -D warnings
test: test-workspace
test-workspace:
	cargo test --manifest-path backend/Cargo.toml --workspace
test-domain:
	cargo test --manifest-path backend/Cargo.toml -p einkaufio-domain
test-application:
	cargo test --manifest-path backend/Cargo.toml -p einkaufio-application
# DynamoDB adapter unit tests plus the shared contract suite and race tests against DynamoDB
# Local (ADR 019). Fails instead of skipping when DYNAMODB_ENDPOINT is unset.
test-dynamodb:
	@test -n "$$DYNAMODB_ENDPOINT" || { echo "DYNAMODB_ENDPOINT is not set; start DynamoDB Local (docs/local-development.md), e.g. DYNAMODB_ENDPOINT=http://localhost:8000" >&2; exit 1; }
	cargo test --manifest-path backend/Cargo.toml -p einkaufio-adapters --features dynamodb,test-support
architecture:
	python3 -m unittest discover -s tests/architecture -p 'test_*.py'
	python3 scripts/check_architecture.py
web-install:
	cd $(WEB_DIR) && npm ci --no-audit --no-fund
web-lint: web-install
	cd $(WEB_DIR) && npm run lint
web-test: web-install
	cd $(WEB_DIR) && npm test
web-build: web-install
	cd $(WEB_DIR) && npm run build
web-e2e-setup:
	cd $(WEB_DIR) && npx playwright install chromium
web-e2e: web-install web-e2e-setup
	cd $(WEB_DIR) && npm run e2e
deny:
	cargo deny --manifest-path backend/Cargo.toml check
terraform-fmt:
	terraform fmt -check -recursive infra/terraform
terraform-validate:
	@set -e; for dir in $(TERRAFORM_DIRS); do \
		terraform -chdir="$$dir" init -backend=false -input=false; \
		terraform -chdir="$$dir" validate; \
	done
CHECKOV ?= checkov
terraform-scan:
	$(CHECKOV) --directory infra/terraform --framework terraform --compact --quiet --download-external-modules false --skip-download
REDOCLY_CLI ?= @redocly/cli@1.34.5
ci-contracts: yaml-lint openapi-lint
yaml-lint:
	yamllint --strict .
openapi-lint:
	npx --yes $(REDOCLY_CLI) lint --config contracts/openapi/redocly.yaml contracts/openapi/openapi.yaml
agent-config-check:
	bash scripts/test_agent_guardrails.sh
