"""Verify reproducible source archives and their source-only filesystem boundary."""

import tempfile
import unittest
import zipfile
from pathlib import Path
import hashlib
import posixpath
import re
from urllib.parse import unquote, urlsplit

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import package_source as packager


class PackageTests(unittest.TestCase):
    """Protect deterministic archives and the source-only filesystem boundary."""

    def test_archive_is_deterministic_and_excludes_generated_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in (
                "Cargo.lock",
                "SOURCES.md",
                "tools/wasm-pack-version.txt",
                "tools/browser/package.json",
                "tools/browser/package-lock.json",
                "tools/browser/node_modules/playwright/cli.js",
                "tools/browser/debug.log",
                "docs/site.webmanifest",
                "crates/core/src/lib.rs",
                "docs/app.js",
                "docs/pkg/generated.js",
                "docs/pkg/compiled.wasm",
                "runtime/runtime-manifest.json",
                "runtime/pkg/sourcefield_wasm_bg.wasm",
                ".fastembed_cache/model.bin",
                "assets/.DS_Store",
                "scripts/__pycache__/tool.pyc",
                "scripts/verify-browser.mjs",
                "tests/integration/field.mjs",
                "target/cache.rs",
            ):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)

            first = packager.package(root, root / "dist/first.zip")
            second = packager.package(root, root / "dist/second.zip")

            self.assertEqual(first, second)
            self.assertTrue((root / ".fastembed_cache/model.bin").is_file())
            with zipfile.ZipFile(root / "dist/first.zip") as archive:
                self.assertEqual(
                    set(archive.namelist()),
                    {
                        "sourcefield/Cargo.lock",
                        "sourcefield/crates/core/src/lib.rs",
                        "sourcefield/docs/app.js",
                        "sourcefield/scripts/verify-browser.mjs",
                        "sourcefield/tests/integration/field.mjs",
                        "sourcefield/SHA256SUMS",
                        "sourcefield/SOURCES.md",
                        "sourcefield/tools/wasm-pack-version.txt",
                        "sourcefield/tools/browser/package.json",
                        "sourcefield/tools/browser/package-lock.json",
                        "sourcefield/docs/site.webmanifest",
                    },
                )
                for line in (
                    archive.read("sourcefield/SHA256SUMS").decode().splitlines()
                ):
                    digest, relative = line.split("  ", 1)
                    self.assertEqual(
                        digest,
                        hashlib.sha256(
                            archive.read(f"sourcefield/{relative}")
                        ).hexdigest(),
                    )

    def test_archive_contains_supported_community_configuration_and_crate_documents(self):
        required = {
            "CODE_OF_CONDUCT.md",
            ".gitattributes",
            ".github/pull_request_template.md",
            ".github/ISSUE_TEMPLATE/bug_report.md",
            ".github/ISSUE_TEMPLATE/feature_request.md",
            ".github/ISSUE_TEMPLATE/config.yml",
            "crates/sourcefield-io/README.md",
            "crates/sourcefield-workspace/README.md",
        }

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in required:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name, encoding="ascii")

            packager.package(root, root / "source.zip")

            with zipfile.ZipFile(root / "source.zip") as archive:
                self.assertEqual(set(archive.namelist()), {f"sourcefield/{name}" for name in required}
                                 | {"sourcefield/SHA256SUMS"})
                for name in required:
                    self.assertEqual(archive.read(f"sourcefield/{name}"), name.encode("ascii"))

    def test_actual_archive_contains_relative_links_from_every_shipped_markdown_document(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "source.zip"

            packager.package(root, output)

            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
                missing = []
                checked = set()
                for name in sorted(names):
                    if not name.endswith(".md"):
                        continue

                    source = archive.read(name).decode("utf-8")
                    links = re.findall(r"\[[^\]]+\]\(([^)]+)\)", source)
                    links.extend(re.findall(r"(?m)^ {0,3}\[[^\]]+\]:\s*(<[^>]+>|\S+)", source))
                    for link in links:
                        url = urlsplit(link.strip("<>"))
                        if url.scheme or url.netloc or not url.path:
                            continue

                        target = posixpath.normpath(posixpath.join(posixpath.dirname(name), unquote(url.path)))
                        checked.add((name, target))
                        if target not in names:
                            missing.append((name, link, target))

                self.assertIn(("sourcefield/README.md", "sourcefield/CODE_OF_CONDUCT.md"), checked)
                self.assertIn(("sourcefield/docs/verification.md", "sourcefield/crates/sourcefield-io/README.md"),
                              checked)
                self.assertEqual(missing, [])
                self.assertIn("sourcefield/crates/sourcefield-workspace/README.md", names)
                self.assertEqual({name for name in packager.ROOT_FILES if not (root / name).is_file()}, set())

    def test_archive_excludes_obsolete_inventory_and_unapproved_neighbors(self):
        excluded = {
            "ACTION-PINS.md", "LICENSE.md", "Makefile", "PACKAGE-INFO.md", "PRIVACY.md",
            "SETUP.md", "VALIDATION-REPORT.md", ".github/CODEOWNERS", "docs/.nojekyll",
            ".github/ISSUE_TEMPLATE/private.md", ".github/local.md", ".github/secrets.yml",
            ".github/ISSUE_TEMPLATE/target/generated.md", "docs/.private/notes.md",
            "crates/core/target/generated.rs", "scripts/node_modules/tool.mjs",
            "crates/sourcefield-io/private.md", "crates/sourcefield-io/target/README.md",
            "crates/sourcefield-workspace/.private/README.md", "crates/unapproved/README.md",
            "tests/dist/output.json", "runtime/pkg/generated.js", "runtime/runtime-manifest.json",
            "tools/browser/node_modules/playwright/cli.js", "docs/preview/unapproved.bin",
        }

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in excluded:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("excluded", encoding="ascii")

            (root / "README.md").write_text("source", encoding="ascii")

            packager.package(root, root / "source.zip")

            with zipfile.ZipFile(root / "source.zip") as archive:
                self.assertEqual(set(archive.namelist()), {"sourcefield/README.md", "sourcefield/SHA256SUMS"})

    def assert_community_symlink_rejected(self, name: str) -> None:
        """Package one linked community file without reading its target or publishing an archive."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "private.txt"
            target.write_text("private", encoding="ascii")
            link = root / name
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(target)
            output = root / "source.zip"

            # Act
            with self.assertRaisesRegex(ValueError, "symlink"):
                packager.package(root, output)

            # Assert
            self.assertFalse(output.exists())
            self.assertEqual(target.read_text(encoding="ascii"), "private")

    def test_archive_rejects_community_symlink_code_of_conduct(self):
        """Reject a linked Code of Conduct without publishing an archive."""
        self.assert_community_symlink_rejected('CODE_OF_CONDUCT.md')

    def test_archive_rejects_community_symlink_git_attributes(self):
        """Reject linked Git attributes without publishing an archive."""
        self.assert_community_symlink_rejected('.gitattributes')

    def test_archive_rejects_community_symlink_pull_request_template(self):
        """Reject a linked pull-request template without publishing an archive."""
        self.assert_community_symlink_rejected('.github/pull_request_template.md')

    def test_archive_rejects_community_symlink_issue_template(self):
        """Reject a linked issue template without publishing an archive."""
        self.assert_community_symlink_rejected('.github/ISSUE_TEMPLATE/bug_report.md')

    def test_archive_rejects_template_parent_symlink_before_publishing(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            outside = root / "private"
            outside.mkdir()
            (outside / "bug_report.md").write_text("private", encoding="ascii")
            (root / ".github").mkdir()
            (root / ".github/ISSUE_TEMPLATE").symlink_to(outside, target_is_directory=True)
            output = root / "source.zip"

            with self.assertRaisesRegex(ValueError, "symlink"):
                packager.package(root, output)

            self.assertFalse(output.exists())

    def test_rejects_symlink_without_reading_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "docs").mkdir()
            (root / "docs/app.js").symlink_to("/etc/passwd")

            with self.assertRaises(ValueError):
                packager.package(root, root / "dist/output.zip")


if __name__ == "__main__":
    unittest.main()
