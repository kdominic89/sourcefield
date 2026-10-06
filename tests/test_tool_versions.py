"""Verify the authored tool pin and one CLI mode per independent test."""

from __future__ import annotations

import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.error import URLError

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
from support.tool_versions import RegistryResponse, payload
import tool_versions


class ToolVersionTests(unittest.TestCase):
    """Isolate pin mutations and execute a single behavior in each Arrange / Act / Assert sequence."""

    def setUp(self) -> None:
        """Create one canonical authored pin in an isolated temporary root."""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.pin = self.root / tool_versions.PIN
        self.pin.parent.mkdir()
        self.pin.write_text("0.15.0\n", encoding="ascii")

    def run_main(self, mode: str, latest: str = "0.15.0") -> tuple[int, str, str]:
        """Execute the public command while replacing only its registry and repository root."""
        document = json.loads(payload(latest))
        current = self.pin.read_text(encoding="ascii").strip()
        if latest != current:
            document["versions"].append({"num": current, "yanked": False})

        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(json.dumps(document).encode("ascii"))

            result = tool_versions.main([mode])

        return result, stdout.getvalue(), stderr.getvalue()

    def test_local_version_is_exact_and_offline(self):
        stdout = io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "fetch_registry_status") as registry,
            contextlib.redirect_stdout(stdout),
        ):
            result = tool_versions.main(["--version"])

        self.assertEqual((result, stdout.getvalue()), (0, "0.15.0\n"))
        registry.assert_not_called()

    def test_pin_rejects_symlink_before_read_or_update(self):
        target = self.root / "external"
        target.write_text("0.15.0\n", encoding="ascii")
        self.pin.unlink()
        self.pin.symlink_to(target)

        with self.assertRaisesRegex(ValueError, "symlink"):
            tool_versions.update_pin(self.root, "0.15.0", "0.16.0")

        self.assertEqual(target.read_text(), "0.15.0\n")

    def test_pin_rejects_parent_symlink(self):
        self.pin.parent.rename(self.root / "actual-tools")
        self.pin.parent.symlink_to(self.root / "actual-tools", target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "symlink"):
            tool_versions.read_pin(self.root)

    def test_pin_rejects_nonregular_file(self):
        self.pin.unlink()
        self.pin.mkdir()

        with self.assertRaisesRegex(ValueError, "regular file"):
            tool_versions.read_pin(self.root)

    def test_current_check_is_read_only(self):
        before = self.pin.read_bytes()

        result, stdout, stderr = self.run_main("--check")

        self.assertEqual((result, stderr), (0, ""))
        self.assertIn("matches crates.io", stdout)
        self.assertEqual(self.pin.read_bytes(), before)

    def test_stale_check_is_actionable_and_read_only(self):
        before = self.pin.read_bytes()

        result, stdout, stderr = self.run_main("--check", "0.16.0")

        self.assertEqual((result, stdout), (1, ""))
        self.assertIn("python3 scripts/tool_versions.py --update", stderr)
        self.assertIn("reviewed pull request", stderr)
        self.assertEqual(self.pin.read_bytes(), before)

    def test_update_changes_only_pin(self):
        sibling = self.pin.parent / "untouched.json"
        sibling.write_text("{}\n", encoding="ascii")

        result, stdout, stderr = self.run_main("--update", "0.16.0")

        self.assertEqual((result, stderr), (0, ""))
        self.assertIn("0.15.0 -> 0.16.0", stdout)
        self.assertEqual(self.pin.read_bytes(), b"0.16.0\n")
        self.assertEqual(sibling.read_bytes(), b"{}\n")
        self.assertEqual(
            sorted(path.name for path in self.pin.parent.iterdir()), ["untouched.json", "wasm-pack-version.txt"]
        )

    def test_update_rejects_downgrade(self):
        before = self.pin.read_bytes()

        with self.assertRaisesRegex(ValueError, "refusing downgrade"):
            tool_versions.update_pin(self.root, "0.15.0", "0.14.0")

        self.assertEqual(self.pin.read_bytes(), before)
        self.assertEqual(list(self.pin.parent.iterdir()), [self.pin])

    def test_update_rejects_concurrent_pin_change_and_cleans_temporary(self):
        self.pin.write_text("0.17.0\n", encoding="ascii")

        with self.assertRaisesRegex(ValueError, "pin changed"):
            tool_versions.update_pin(self.root, "0.15.0", "0.16.0")

        self.assertEqual(self.pin.read_bytes(), b"0.17.0\n")
        self.assertEqual(list(self.pin.parent.iterdir()), [self.pin])

    def assert_yanked_pin_requires_manual_review(self, mode: str) -> None:
        """Exercise one CLI mode without losing the pin or its specific registry diagnostic."""
        # Arrange
        source = json.dumps({"crate": {"id": "wasm-pack", "max_stable_version": "0.14.0"}, "versions": [
            {"num": "0.15.0", "yanked": True}, {"num": "0.14.0", "yanked": False},
        ]}).encode("ascii")

        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(source)

            # Act
            result = tool_versions.main([mode])

        # Assert
        self.assertEqual((result, stdout.getvalue()), (1, ""))
        self.assertIn("pin 0.15.0 is yanked", stderr.getvalue())
        self.assertIn("latest stable is 0.14.0", stderr.getvalue())
        self.assertIn("refusing downgrade", stderr.getvalue())
        self.assertIn("manually edit tools/wasm-pack-version.txt", stderr.getvalue())
        self.assertIn("reviewed pull request", stderr.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")
        factory.return_value.open.assert_called_once()

    def test_yanked_pin_with_lower_latest_check_requires_manual_review(self):
        """Require --check to reject this inventory without mutating the pin."""
        self.assert_yanked_pin_requires_manual_review("--check")

    def test_yanked_pin_with_lower_latest_update_requires_manual_review(self):
        """Require --update to reject this inventory without mutating the pin."""
        self.assert_yanked_pin_requires_manual_review("--update")

    def test_yanked_pin_with_newer_latest_checks_without_mutation(self):
        source = json.dumps({"crate": {"id": "wasm-pack", "max_stable_version": "0.16.0"}, "versions": [
            {"num": "0.15.0", "yanked": True}, {"num": "0.16.0", "yanked": False},
        ]}).encode("ascii")

        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(source)

            result = tool_versions.main(["--check"])

        self.assertEqual((result, stdout.getvalue()), (1, ""))
        self.assertIn("pin 0.15.0 is yanked", stderr.getvalue())
        self.assertIn("latest stable is 0.16.0", stderr.getvalue())
        self.assertIn("python3 scripts/tool_versions.py --update", stderr.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")
        factory.return_value.open.assert_called_once()

    def test_yanked_pin_with_newer_latest_updates_with_warning(self):
        source = json.dumps({"crate": {"id": "wasm-pack", "max_stable_version": "0.16.0"}, "versions": [
            {"num": "0.15.0", "yanked": True}, {"num": "0.16.0", "yanked": False},
        ]}).encode("ascii")

        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(source)

            result = tool_versions.main(["--update"])

        self.assertEqual(result, 0)
        self.assertIn("Warning: wasm-pack pin 0.15.0 is yanked", stderr.getvalue())
        self.assertIn("0.15.0 -> 0.16.0", stdout.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.16.0\n")
        factory.return_value.open.assert_called_once()

    def assert_missing_pin_fails_closed(self, mode: str) -> None:
        """Exercise one CLI mode without losing the pin or its specific registry diagnostic."""
        # Arrange
        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(payload("0.16.0"))

            # Act
            result = tool_versions.main([mode])

        # Assert
        self.assertEqual((result, stdout.getvalue()), (1, ""))
        self.assertIn("pin 0.15.0 is missing from", stderr.getvalue())
        self.assertIn("latest stable is 0.16.0", stderr.getvalue())
        self.assertNotIn("is yanked", stderr.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")

    def test_missing_pin_check_fails_closed_with_distinct_diagnostic(self):
        """Require --check to reject this inventory without mutating the pin."""
        self.assert_missing_pin_fails_closed("--check")

    def test_missing_pin_update_fails_closed_with_distinct_diagnostic(self):
        """Require --update to reject this inventory without mutating the pin."""
        self.assert_missing_pin_fails_closed("--update")

    def test_network_failure_is_concise_and_preserves_pin(self):
        stderr = io.StringIO()
        before = self.pin.read_bytes()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "fetch_registry_status", side_effect=URLError("offline")),
            contextlib.redirect_stderr(stderr),
        ):
            result = tool_versions.main(["--update"])

        self.assertEqual(result, 1)
        self.assertIn("offline", stderr.getvalue())
        self.assertEqual(len(stderr.getvalue().splitlines()), 1)
        self.assertEqual(self.pin.read_bytes(), before)

    def test_cli_emits_only_the_version_offline(self):
        command = [sys.executable, "-B", str(ROOT / "scripts/tool_versions.py"), "--version"]
        expected = (ROOT / tool_versions.PIN).read_text(encoding="ascii")

        result = subprocess.run(command, capture_output=True, text=True, check=False)

        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, expected, ""))

    def assert_pin_rejected(self, value: str) -> None:
        """Read one malformed authored pin and preserve its original bytes on rejection."""
        # Arrange
        source = value.encode("utf-8")
        self.pin.write_bytes(source)

        # Act
        with self.assertRaises(ValueError):
            tool_versions.read_pin(self.root)

        # Assert
        self.assertEqual(self.pin.read_bytes(), source)

    def test_pin_rejects_missing_patch(self):
        """Reject a pin missing the patch component."""
        self.assert_pin_rejected('0.15')

    def test_pin_rejects_tag_prefix(self):
        """Reject a Git-tag prefix in the tool pin."""
        self.assert_pin_rejected('v0.15.0')

    def test_pin_rejects_leading_zero(self):
        """Reject a noncanonical leading zero."""
        self.assert_pin_rejected('00.15.0')

    def test_pin_rejects_prerelease(self):
        """Reject prerelease tool pins."""
        self.assert_pin_rejected('0.15.0-rc.1')

    def test_pin_rejects_build_metadata(self):
        """Reject build metadata in the canonical tool pin."""
        self.assert_pin_rejected('0.15.0+build')

    def test_pin_rejects_leading_whitespace(self):
        """Reject leading whitespace rather than normalizing it."""
        self.assert_pin_rejected(' 0.15.0')

    def test_pin_rejects_extra_newline(self):
        """Reject multiple trailing newlines."""
        self.assert_pin_rejected('0.15.0\n\n')

    def test_pin_rejects_crlf(self):
        """Reject a noncanonical CRLF terminator."""
        self.assert_pin_rejected('0.15.0\r\n')

    def test_pin_rejects_missing_newline(self):
        """Require the canonical single newline terminator."""
        self.assert_pin_rejected('0.15.0')

    def test_pin_rejects_oversized(self):
        """Bound authored pin bytes before parsing."""
        self.assert_pin_rejected("1" * 65)

    def test_pin_rejects_non_ascii(self):
        """Reject non-ASCII pin bytes."""
        self.assert_pin_rejected('\xe9.15.0\n')

    def assert_update_inventory_rejected(self, entries: list) -> None:
        """Execute one update against inconsistent inventory without mutating the authored pin."""
        # Arrange
        document = json.loads(payload("0.16.0"))
        document["versions"].extend(entries)
        stderr = io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(json.dumps(document).encode("ascii"))

            # Act
            result = tool_versions.main(["--update"])

        # Assert
        self.assertEqual(result, 1)
        self.assertIn("inconsistent", stderr.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")

    def test_update_rejects_inventory_duplicate_pin_entries(self):
        """Reject contradictory duplicate entries for the current pin."""
        self.assert_update_inventory_rejected([{"num": "0.15.0", "yanked": False}, {"num": "0.15.0", "yanked": True}])

    def test_update_rejects_inventory_string_yanked(self):
        """Require a Boolean yanked marker."""
        self.assert_update_inventory_rejected([{"num": "0.15.0", "yanked": "false"}])

    def test_update_rejects_inventory_missing_yanked(self):
        """Reject a pin entry missing its yanked marker."""
        self.assert_update_inventory_rejected([{"num": "0.15.0"}])

    def test_update_rejects_inventory_newer_stable_than_latest(self):
        """Reject inventory newer than the endpoint latest-stable identity."""
        self.assert_update_inventory_rejected([{"num": "0.17.0", "yanked": False}])

    def test_update_rejects_inventory_malformed_number(self):
        """Reject malformed inventory versions."""
        self.assert_update_inventory_rejected([{"num": "not-a-version", "yanked": False}])

    def test_update_rejects_inventory_non_object_entry(self):
        """Reject an inventory entry that is not an object."""
        self.assert_update_inventory_rejected([None])


if __name__ == "__main__":
    unittest.main()
