#!/usr/bin/env python3
"""Fail when Rust workspace crate dependencies violate the hexagonal boundaries."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "backend" / "Cargo.toml"

EXPECTED_INTERNAL_DEPENDENCIES = {
    "einkaufio-domain": set(),
    "einkaufio-application": {"einkaufio-domain"},
    "einkaufio-adapters": {"einkaufio-application", "einkaufio-domain"},
    "einkaufio-api": {"einkaufio-application", "einkaufio-adapters"},
}
PURE_CRATES = {"einkaufio-domain", "einkaufio-application"}


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

    metadata = json.loads(result.stdout)
    packages = {
        package["name"]: package
        for package in metadata["packages"]
        if package["name"] in EXPECTED_INTERNAL_DEPENDENCIES
    }

    missing = set(EXPECTED_INTERNAL_DEPENDENCIES) - set(packages)
    if missing:
        print(f"Missing workspace crates: {', '.join(sorted(missing))}", file=sys.stderr)
        return 1

    errors: list[str] = []
    for name, allowed in EXPECTED_INTERNAL_DEPENDENCIES.items():
        dependencies = packages[name].get("dependencies", [])
        internal = {
            dependency["name"]
            for dependency in dependencies
            if dependency["name"] in EXPECTED_INTERNAL_DEPENDENCIES
        }
        forbidden_internal = internal - allowed
        if forbidden_internal:
            errors.append(
                f"{name} has forbidden internal dependencies: "
                f"{', '.join(sorted(forbidden_internal))}"
            )

        if name in PURE_CRATES:
            external = sorted(
                dependency["name"]
                for dependency in dependencies
                if dependency["name"] not in EXPECTED_INTERNAL_DEPENDENCIES
            )
            if external:
                errors.append(
                    f"{name} must remain framework/technology independent; "
                    f"external dependencies found: {', '.join(external)}"
                )

    if errors:
        print("Architecture boundary check FAILED:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print("Architecture boundary check passed.")
    for name in EXPECTED_INTERNAL_DEPENDENCIES:
        print(f"- {name}: {', '.join(sorted(EXPECTED_INTERNAL_DEPENDENCIES[name])) or '(no internal dependencies)'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
