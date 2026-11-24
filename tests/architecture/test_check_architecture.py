import unittest

from scripts.check_architecture import (
    EXPECTED_INTERNAL_DEPENDENCIES,
    validate_metadata,
)


def package(name: str, dependencies: list[str] | None = None) -> dict:
    return {
        "name": name,
        "dependencies": [{"name": dependency} for dependency in dependencies or []],
    }


def compliant_metadata() -> dict:
    return {
        "packages": [
            package(name, sorted(dependencies))
            for name, dependencies in EXPECTED_INTERNAL_DEPENDENCIES.items()
        ]
    }


class ArchitectureBoundaryTests(unittest.TestCase):
    def test_accepts_the_intended_dependency_direction(self) -> None:
        self.assertEqual(validate_metadata(compliant_metadata()), [])

    def test_rejects_domain_dependency_on_an_external_framework(self) -> None:
        metadata = compliant_metadata()
        metadata["packages"][0]["dependencies"].append({"name": "axum"})
        self.assertTrue(
            any("einkaufio-domain" in error and "axum" in error
                for error in validate_metadata(metadata))
        )

    def test_rejects_application_dependency_on_an_adapter(self) -> None:
        metadata = compliant_metadata()
        application = next(
            item for item in metadata["packages"]
            if item["name"] == "einkaufio-application"
        )
        application["dependencies"].append({"name": "einkaufio-adapters"})
        self.assertTrue(
            any("einkaufio-application" in error
                for error in validate_metadata(metadata))
        )

    def test_rejects_unregistered_workspace_crates(self) -> None:
        metadata = compliant_metadata()
        metadata["packages"].append(package("einkaufio-unclassified"))
        self.assertTrue(
            any("Unexpected workspace crates" in error
                for error in validate_metadata(metadata))
        )

    def test_rejects_missing_architecture_crates(self) -> None:
        metadata = compliant_metadata()
        metadata["packages"] = [
            item for item in metadata["packages"]
            if item["name"] != "einkaufio-domain"
        ]
        self.assertTrue(
            any("Missing workspace crates" in error
                for error in validate_metadata(metadata))
        )


if __name__ == "__main__":
    unittest.main()
