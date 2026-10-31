"""Reject inconsistent official Binaryen release and platform inventories."""

from __future__ import annotations

import unittest

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
from support.tool_versions import manifest, release
import tool_versions


class ReleaseMetadataTests(unittest.TestCase):
    """Validate repository identity, stable version and content metadata independently."""

    def test_official_current_release_is_accepted(self):
        document = release()

        version, archives = tool_versions.release_assets(document, 133)

        self.assertEqual(version, 133)
        self.assertEqual(set(archives), set(manifest()["hosts"]))

    def assert_rejected(self, change, message):
        """Change exactly one source field before calling the production metadata validator."""
        document = release()
        change(document)

        with self.assertRaisesRegex(ValueError, message):
            tool_versions.release_assets(document)

    def test_prerelease_is_rejected(self):
        self.assert_rejected(lambda value: value.update(prerelease=True), "stable")

    def test_draft_is_rejected(self):
        self.assert_rejected(lambda value: value.update(draft=True), "published")

    def test_boolean_metadata_is_required(self):
        self.assert_rejected(lambda value: value.update(draft=0), "published")

    def test_wrong_repository_is_rejected(self):
        self.assert_rejected(
            lambda value: value.update(html_url="https://github.com/attacker/binaryen/releases/tag/version_133"),
            "official",
        )

    def test_noncanonical_version_is_rejected(self):
        self.assert_rejected(lambda value: value.update(tag_name="version_0133"), "canonical")

    def test_prerelease_tag_is_rejected(self):
        self.assert_rejected(lambda value: value.update(tag_name="version_134-rc.1"), "canonical")

    def test_missing_host_is_rejected(self):
        self.assert_rejected(lambda value: value["assets"].pop(), "missing")

    def test_duplicate_asset_is_rejected(self):
        self.assert_rejected(lambda value: value["assets"].append(value["assets"][0]), "duplicate")

    def test_unbounded_inventory_is_rejected(self):
        self.assert_rejected(lambda value: value.update(assets=[{}] * 129), "bounded")

    def test_foreign_asset_url_is_rejected(self):
        self.assert_rejected(
            lambda value: value["assets"][0].update(browser_download_url="https://attacker.invalid/wasm-opt"),
            "official",
        )

    def test_missing_asset_digest_is_rejected(self):
        self.assert_rejected(lambda value: value["assets"][0].pop("digest"), "SHA-256")

    def test_boolean_asset_size_is_rejected(self):
        self.assert_rejected(lambda value: value["assets"][0].update(size=True), "integer")

    def test_unuploaded_asset_is_rejected(self):
        self.assert_rejected(lambda value: value["assets"][0].update(state="new"), "uploaded")

    def test_requested_tag_mismatch_is_rejected(self):
        document = release()

        with self.assertRaisesRegex(ValueError, "requested version"):
            tool_versions.release_assets(document, 134)


if __name__ == "__main__":
    unittest.main()
