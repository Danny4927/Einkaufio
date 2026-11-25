SHELL := /bin/bash
WEB_DIR := apps/web
TERRAFORM_DIRS := infra/terraform/bootstrap infra/terraform/environments/dev infra/terraform/environments/prod

.PHONY: check verify fmt lint test test-domain test-application test-workspace architecture terraform-fmt terraform-validate web-install web-build web-e2e

# Fast backend feedback loop retained for day-to-day development.
check: fmt lint test-workspace architecture terraform-fmt

# Mirrors every validation job in .github/workflows/ci.yml.
verify: fmt lint test-workspace architecture web-build web-e2e terraform-fmt terraform-validate

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

web-build:
	cd $(WEB_DIR) && npm ci --no-audit --no-fund && npm run build

web-e2e:
	cd $(WEB_DIR) && npm ci --no-audit --no-fund && npx playwright install --with-deps chromium && npm run e2e

terraform-fmt:
	terraform fmt -check -recursive infra/terraform

terraform-validate:
	@set -e; for dir in $(TERRAFORM_DIRS); do \
		terraform -chdir="$$dir" init -backend=false -input=false; \
		terraform -chdir="$$dir" validate; \
	done
