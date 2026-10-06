"""Verify registry identity, semantic versions and bounded version inventories."""

from __future__ import annotations

import json
import unittest

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
from support.tool_versions import payload
import tool_versions


class RegistryStatusTests(unittest.TestCase):
    """Keep every accepted or rejected registry input independently discoverable."""

    def test_registry_accepts_matching_non_yanked_stable_release(self):
        source = payload()

        result = tool_versions.registry_status(source, "0.15.0")

        self.assertEqual(result, tool_versions.RegistryStatus("0.15.0", False))

    def test_registry_accepts_prereleases_and_yanked_history_without_promoting_them(self):
        document = json.loads(payload())
        document["versions"].extend([
            {"num": "0.16.0", "yanked": True},
            {"num": "0.17.0-rc.1", "yanked": False},
            {"num": "0.14.0+build.01", "yanked": False},
        ])
        source = json.dumps(document).encode("ascii")

        result = tool_versions.registry_status(source, "0.15.0")

        self.assertEqual(result, tool_versions.RegistryStatus("0.15.0", False))

    def test_registry_rejects_latest_below_non_yanked_current_pin(self):
        document = json.loads(payload("0.14.0"))
        document["versions"].append({"num": "0.15.0", "yanked": False})
        source = json.dumps(document).encode("ascii")

        with self.assertRaisesRegex(ValueError, "inconsistent: latest stable"):
            tool_versions.registry_status(source, "0.15.0")

    def test_registry_rejects_mass_body_before_parsing(self):
        source = b" " * (tool_versions.MAX_RESPONSE_BYTES + 1)

        with self.assertRaisesRegex(ValueError, "exceeds 1 MiB"):
            tool_versions.registry_status(source, "0.15.0")

    def assert_history_number_rejected(self, number: object) -> None:
        """Validate one malformed historical number with the current release left otherwise valid."""
        # Arrange
        document = json.loads(payload())
        document["versions"].append({"num": number, "yanked": False})
        source = json.dumps(document).encode("ascii")

        # Act / Assert
        with self.assertRaisesRegex(ValueError, "inconsistent"):
            tool_versions.registry_status(source, "0.15.0")

    def test_registry_rejects_history_null(self):
        """Reject null historical version numbers."""
        self.assert_history_number_rejected(None)

    def test_registry_rejects_history_numeric(self):
        """Reject numeric historical version numbers."""
        self.assert_history_number_rejected(16)

    def test_registry_rejects_history_empty(self):
        """Reject empty historical version numbers."""
        self.assert_history_number_rejected("")

    def test_registry_rejects_history_tag_prefix(self):
        """Reject tag-prefixed historical version numbers."""
        self.assert_history_number_rejected("v0.14.0")

    def test_registry_rejects_history_leading_zero(self):
        """Reject historical versions with leading zeroes."""
        self.assert_history_number_rejected("00.14.0")

    def test_registry_rejects_history_numeric_prerelease_leading_zero(self):
        """Reject a numeric prerelease with a leading zero."""
        self.assert_history_number_rejected("0.14.0-01")

    def test_registry_rejects_history_empty_prerelease_segment(self):
        """Reject an empty prerelease segment."""
        self.assert_history_number_rejected("0.14.0-rc..1")

    def test_registry_rejects_history_empty_build_metadata(self):
        """Reject empty build metadata."""
        self.assert_history_number_rejected("0.14.0+")

    def test_registry_rejects_history_empty_build_segment(self):
        """Reject an empty build-metadata segment."""
        self.assert_history_number_rejected("0.14.0+build..1")

    def test_registry_rejects_history_oversized(self):
        """Bound historical version strings."""
        self.assert_history_number_rejected("0.14.0-" + "x" * 128)

    def assert_registry_document_rejected(self, source: bytes) -> None:
        """Submit one malformed registry response with no fixture or action loop."""
        # Arrange
        pin = "0.15.0"

        # Act / Assert
        with self.assertRaises((ValueError, TypeError)):
            tool_versions.registry_status(source, pin)

    def test_registry_rejects_document_malformed_json(self):
        """Reject a non-JSON registry response."""
        self.assert_registry_document_rejected(b"not JSON")

    def test_registry_rejects_document_array_root(self):
        """Reject an array instead of the registry object."""
        self.assert_registry_document_rejected(b"[]")

    def test_registry_rejects_document_missing_crate(self):
        """Reject missing registry crate metadata."""
        self.assert_registry_document_rejected(b"{}")

    def test_registry_rejects_document_wrong_identity(self):
        """Bind the endpoint response to the requested crate identity."""
        self.assert_registry_document_rejected(payload(id="other"))

    def test_registry_rejects_document_prerelease_latest(self):
        """Require the latest-stable identity to be stable."""
        self.assert_registry_document_rejected(payload("0.16.0-beta.1"))

    def test_registry_rejects_document_build_metadata_latest(self):
        """Reject build metadata in latest-stable identity."""
        self.assert_registry_document_rejected(payload("0.16.0+build"))

    def test_registry_rejects_document_leading_zero_latest(self):
        """Require canonical latest-stable version components."""
        self.assert_registry_document_rejected(payload("00.16.0"))

    def test_registry_rejects_document_unbounded_latest_component(self):
        """Bound latest-stable version components."""
        self.assert_registry_document_rejected(payload("9999999999.0.0"))

    def test_registry_rejects_document_null_latest(self):
        """Reject null latest-stable identity."""
        self.assert_registry_document_rejected(payload(max_stable_version=None))

    def test_registry_rejects_document_numeric_latest(self):
        """Reject numeric latest-stable identity."""
        self.assert_registry_document_rejected(payload(max_stable_version=16))

    def assert_registry_inventory_rejected(self, versions: object) -> None:
        """Validate one invalid inventory while keeping crate metadata valid."""
        # Arrange
        base = json.loads(payload())
        source = json.dumps({**base, "versions": versions}).encode("ascii")

        # Act / Assert
        with self.assertRaises((ValueError, TypeError)):
            tool_versions.registry_status(source, "0.15.0")

    def test_registry_rejects_inventory_null(self):
        """Reject null version inventory."""
        self.assert_registry_inventory_rejected(None)

    def test_registry_rejects_inventory_empty(self):
        """Reject missing latest-stable inventory entries."""
        self.assert_registry_inventory_rejected([])

    def test_registry_rejects_inventory_missing_latest(self):
        """Require latest-stable to appear in inventory."""
        self.assert_registry_inventory_rejected([{"num": "0.14.0", "yanked": False}])

    def test_registry_rejects_inventory_yanked_latest(self):
        """Require latest-stable to remain non-yanked."""
        self.assert_registry_inventory_rejected([{"num": "0.15.0", "yanked": True}])

    def test_registry_rejects_inventory_nonboolean_yanked(self):
        """Reject an integer yanked marker."""
        self.assert_registry_inventory_rejected([{"num": "0.15.0", "yanked": 0}])

    def test_registry_rejects_inventory_duplicate_latest(self):
        """Reject duplicate latest-stable inventory entries."""
        self.assert_registry_inventory_rejected(json.loads(payload())["versions"] * 2)

    def test_registry_rejects_inventory_oversized(self):
        """Bound the number of registry inventory entries."""
        self.assert_registry_inventory_rejected(json.loads(payload())["versions"] * 513)


if __name__ == "__main__":
    unittest.main()
