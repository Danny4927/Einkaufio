SHELL := /bin/bash

.PHONY: check fmt lint test architecture terraform-fmt

check: fmt lint test architecture terraform-fmt

fmt:
	cargo fmt --manifest-path backend/Cargo.toml --all -- --check

lint:
	cargo clippy --manifest-path backend/Cargo.toml --workspace --all-targets -- -D warnings

test:
	cargo test --manifest-path backend/Cargo.toml --workspace

architecture:
	python3 -m unittest discover -s tests/architecture -p 'test_*.py'
	python3 scripts/check_architecture.py

terraform-fmt:
	terraform fmt -check -recursive infra/terraform
