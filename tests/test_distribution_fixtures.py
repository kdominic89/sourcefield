"""Verify executable browser fixtures and managed README prerequisites."""

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import fixture_support
import release_browser_fixtures


class ExecutableFixtureTests(unittest.TestCase):
    """Keep authored setup and required producer evidence explicit in executable gates."""

    def test_readme_contains_both_managed_regions_and_fallback_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = root / "profile.toml"
            config.write_text('[profile]\npages_url="https://example.invalid/profile/"\n')
            readme = root / "README.md"

            fixture_support.write_readme(readme, config)

            text = readme.read_text()
            for marker in ("projects:start", "projects:end", "packages:start", "packages:end"):
                self.assertEqual(text.count(f"<!-- sourcefield:{marker} -->"), 1)

            for name in ("dark", "light", "static"):
                self.assertIn(f"assets/sourcefield.{name}.svg", text)

            self.assertIn("https://example.invalid/profile/", text)

    def test_browser_gate_refuses_missing_producer_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with patch.object(release_browser_fixtures.subprocess, "run"):
                with self.assertRaisesRegex(AssertionError, "required history fixture"):
                    release_browser_fixtures.verify_profiles(
                        root, root / "sourcefield", root / "output", "playwright.mjs"
                    )


if __name__ == "__main__":
    unittest.main()
