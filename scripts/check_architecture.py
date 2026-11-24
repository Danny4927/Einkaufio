#!/usr/bin/env python3
"""Fail when Rust workspace crate dependencies violate the hexagonal boundaries."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "backend" / "Cargo.toml"

EXPECTED_INTERNAL_DEPENDENCIES = {
    "einkaufio-domain": set(),
    "einkaufio-application": {"einkaufio-domain"},
    "einkaufio-adapters": {"einkaufio-application", "einkaufio-domain"},
    "einkaufio-api": {"einkaufio-application", "einkaufio-adapters"},
}
PURE_CRATES = {"einkaufio-domain", "einkaufio-application"}


def validate_metadata(metadata: dict[str, Any]) -> list[str]:
    """Return architecture violations for Cargo metadata (also unit-testable)."""
    packages = {package["name"]: package for package in metadata.get("packages", [])}
    actual_names = set(packages)
    expected_names = set(EXPECTED_INTERNAL_DEPENDENCIES)
    errors: list[str] = []

    missing = expected_names - actual_names
    unexpected = actual_names - expected_names
    if missing:
        errors.append(f"Missing workspace crates: {', '.join(sorted(missing))}")
    if unexpected:
        errors.append(f"Unexpected workspace crates: {', '.join(sorted(unexpected))}")
    if errors:
        return errors

    for name, allowed in EXPECTED_INTERNAL_DEPENDENCIES.items():
        dependencies = packages[name].get("dependencies", [])
        dependency_names = {dependency["name"] for dependency in dependencies}
        internal = dependency_names & expected_names
        forbidden_internal = internal - allowed
        if forbidden_internal:
            errors.append(
                f"{name} has forbidden internal dependencies: "
                f"{', '.join(sorted(forbidden_internal))}"
            )

        if name in PURE_CRATES:
            external = sorted(dependency_names - expected_names)
            if external:
                errors.append(
                    f"{name} must remain framework/technology independent; "
                    f"external dependencies found: {', '.join(external)}"
                )

    return errors


def main() -> int:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            str(MANIFEST),
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        print(result.stderr, file=sys.stderr)
        return result.returncode

    errors = validate_metadata(json.loads(result.stdout))
    if errors:
        print("Architecture boundary check FAILED:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print("Architecture boundary check passed.")
    for name, dependencies in EXPECTED_INTERNAL_DEPENDENCIES.items():
        rendered = ", ".join(sorted(dependencies)) or "(no internal dependencies)"
        print(f"- {name}: {rendered}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
