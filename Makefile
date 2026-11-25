SHELL := /bin/bash
WEB_DIR := apps/web
TERRAFORM_DIRS := $(shell find infra/terraform -type f -name '*.tf' -printf '%h\n' | sort -u)
PLAYWRIGHT_INSTALL ?= npx playwright install chromium

.PHONY: check verify ci-backend ci-web ci-terraform fmt lint test test-domain test-application test-workspace architecture terraform-fmt terraform-validate web-install web-build web-e2e web-e2e-setup agent-config-check

check: fmt lint test-workspace architecture terraform-fmt
verify: ci-backend ci-web ci-terraform agent-config-check
ci-backend: fmt lint test-workspace architecture
ci-web: web-install
	cd $(WEB_DIR) && npm run build
	cd $(WEB_DIR) && $(PLAYWRIGHT_INSTALL)
	cd $(WEB_DIR) && npm run e2e
ci-terraform: terraform-fmt terraform-validate

fmt:
	cargo fmt --manifest-path backend/Cargo.toml --all -- --check
lint:
	cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings
test: test-workspace
test-workspace:
	cargo test --manifest-path backend/Cargo.toml --workspace
test-domain:
	cargo test --manifest-path backend/Cargo.toml -p einkaufio-domain
test-application:
	cargo test --manifest-path backend/Cargo.toml -p einkaufio-application
architecture:
	python3 -m unittest discover -s tests/architecture -p 'test_*.py'
	python3 scripts/check_architecture.py
web-install:
	cd $(WEB_DIR) && npm ci --no-audit --no-fund
web-build: web-install
	cd $(WEB_DIR) && npm run build
web-e2e-setup:
	cd $(WEB_DIR) && npx playwright install chromium
web-e2e: web-install web-e2e-setup
	cd $(WEB_DIR) && npm run e2e
terraform-fmt:
	terraform fmt -check -recursive infra/terraform
terraform-validate:
	@set -e; for dir in $(TERRAFORM_DIRS); do \
		terraform -chdir="$$dir" init -backend=false -input=false; \
		terraform -chdir="$$dir" validate; \
	done
agent-config-check:
	bash scripts/test_agent_guardrails.sh
