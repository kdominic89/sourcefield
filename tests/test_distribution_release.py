"""Verify deterministic release packaging and complete manifest assembly."""

import json
import tempfile
import unittest
import zipfile
from pathlib import Path

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import release_manifest
import release_package
from support.releases import make_assets


class ReleasePackageTests(unittest.TestCase):
    """Refuse incomplete browser runtimes and retain deterministic native archives."""

    def test_native_package_is_deterministic(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "native"
            binary.write_bytes(b"synthetic executable")

            first = release_package.package(
                root,
                root / "first",
                "x86_64-unknown-linux-gnu",
                "a" * 40,
                "v1.2.3",
                binary,
            )

            second = release_package.package(
                root,
                root / "second",
                "x86_64-unknown-linux-gnu",
                "a" * 40,
                "v1.2.3",
                binary,
            )

            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_incomplete_runtime_cannot_be_packaged(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            with self.assertRaisesRegex(ValueError, "incomplete browser"):
                release_package.package(
                    root, root / "dist", "browser", "a" * 40, "v1.2.3"
                )


class ReleaseAssemblyTests(unittest.TestCase):
    """Reject inconsistent source identities across release assets."""

    def test_mixed_source_release_manifest_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            make_assets(root)
            with zipfile.ZipFile(
                root / "assets/sourcefield-browser.zip", "w"
            ) as bundle:
                bundle.writestr(
                    "release-metadata.json",
                    json.dumps(
                        {
                            "schema_version": 1,
                            "source_commit": "c" * 40,
                            "release": "v1.2.3",
                            "target": "browser",
                        }
                    ),
                )

            with self.assertRaisesRegex(ValueError, "metadata mismatch"):
                release_manifest.manifest(root / "assets", "a" * 40, "v1.2.3")


if __name__ == "__main__":
    unittest.main()
