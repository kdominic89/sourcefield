"""Check explicit Binaryen pin updates without network or repository mutations."""

from __future__ import annotations

import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import URLError

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
from support.tool_versions import future, manifest, release, write_manifest
import tool_versions
from sourcefield_tools import binaryen


class ToolVersionTests(unittest.TestCase):
    """Separate local identity, read-only checks and deliberate atomic updates."""

    def setUp(self):
        """Create one isolated authored manifest for each CLI behavior."""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.current = manifest()
        self.path = write_manifest(self.root, self.current)

    def execute(self, mode, latest=None):
        """Replace only official metadata and the authored repository root."""
        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(binaryen, "fetch_json", side_effect=[latest or release(self.current), release(self.current)]),
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            result = tool_versions.main([mode])

        return result, stdout.getvalue(), stderr.getvalue()

    def test_version_is_offline(self):
        output = io.StringIO()
        with patch.object(tool_versions, "ROOT", self.root), patch.object(binaryen, "fetch_json") as fetch:
            with contextlib.redirect_stdout(output):
                result = tool_versions.main(["--version"])

        self.assertEqual((result, output.getvalue()), (0, "133\n"))
        fetch.assert_not_called()

    def test_current_check_preserves_bytes(self):
        before = self.path.read_bytes()

        result, output, errors = self.execute("--check")

        self.assertEqual((result, errors), (0, ""))
        self.assertIn("all five", output)
        self.assertEqual(self.path.read_bytes(), before)

    def test_stale_check_is_read_only_and_actionable(self):
        before = self.path.read_bytes()

        result, output, errors = self.execute("--check", release(future(self.current)))

        self.assertEqual((result, output), (1, ""))
        self.assertIn("python3 scripts/tool_versions.py --update", errors)
        self.assertEqual(self.path.read_bytes(), before)

    def test_same_version_mutated_asset_requires_manual_review(self):
        document = release(self.current)
        document["assets"][0]["digest"] = "sha256:" + "0" * 64

        result, output, errors = self.execute("--update", document)

        self.assertEqual((result, output), (1, ""))
        self.assertIn("content changed", errors)
        self.assertEqual(binaryen.read_manifest(self.root), self.current)

    def test_missing_pinned_release_stops_update(self):
        errors = io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(binaryen, "fetch_json", side_effect=[release(future(self.current)), URLError("not found")]),
            patch.object(tool_versions, "candidate_manifest") as candidate,
            contextlib.redirect_stderr(errors),
        ):
            result = tool_versions.main(["--update"])

        self.assertEqual(result, 1)
        self.assertIn("not found", errors.getvalue())
        candidate.assert_not_called()
        self.assertEqual(binaryen.read_manifest(self.root), self.current)

    def test_downgrade_is_rejected(self):
        result, output, errors = self.execute("--update", release(future(self.current, 132)))

        self.assertEqual((result, output), (1, ""))
        self.assertIn("refusing downgrade", errors)
        self.assertEqual(binaryen.read_manifest(self.root), self.current)

    def test_explicit_update_changes_only_manifest(self):
        candidate = future(self.current)
        sibling = self.path.parent / "untouched"
        sibling.write_bytes(b"same")
        with patch.object(tool_versions, "candidate_manifest", return_value=candidate):
            result, output, errors = self.execute("--update", release(candidate))

        self.assertEqual((result, errors), (0, ""))
        self.assertIn("133 -> 134", output)
        self.assertEqual(binaryen.read_manifest(self.root), candidate)
        self.assertEqual(sibling.read_bytes(), b"same")
        self.assertEqual(sorted(item.name for item in self.path.parent.iterdir()), ["binaryen.json", "untouched"])

    def test_same_version_update_does_not_redownload(self):
        with patch.object(tool_versions, "candidate_manifest") as candidate:
            result, _, errors = self.execute("--update")

        self.assertEqual((result, errors), (0, ""))
        candidate.assert_not_called()

    def test_atomic_update_rejects_concurrent_pin_change(self):
        changed = future(self.current, 135)
        write_manifest(self.root, changed)

        with self.assertRaisesRegex(ValueError, "pin changed"):
            tool_versions.update_manifest(self.root, self.current, future(self.current))

        self.assertEqual(binaryen.read_manifest(self.root), changed)
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])

    def test_atomic_update_rejects_downgrade(self):
        before = self.path.read_bytes()

        with self.assertRaisesRegex(ValueError, "refusing downgrade"):
            tool_versions.update_manifest(self.root, self.current, future(self.current, 132))

        self.assertEqual(self.path.read_bytes(), before)

    def test_download_failure_preserves_existing_manifest(self):
        before = self.path.read_bytes()
        with patch.object(tool_versions, "candidate_manifest", side_effect=ValueError("archive digest mismatch")):
            result, output, errors = self.execute("--update", release(future(self.current)))

        self.assertEqual((result, output), (1, ""))
        self.assertIn("archive digest mismatch", errors)
        self.assertEqual(self.path.read_bytes(), before)

    def test_candidate_discovers_every_host_before_update(self):
        latest = release(future(self.current))
        def fake_download(version, record, destination):
            destination.write_bytes(b"authenticated at boundary")
        def fake_unpack(archive, version, record, destination, *, discover):
            self.assertTrue(discover)
            return [{"path": item["path"], "size": 123, "sha256": "a" * 64} for item in record["files"]]
        with patch.object(binaryen, "download_archive", side_effect=fake_download) as download:
            with patch.object(binaryen, "unpack_optimizer", side_effect=fake_unpack):
                candidate = tool_versions.candidate_manifest(self.current, latest)

        self.assertEqual(download.call_count, 5)
        self.assertEqual(candidate["version"], 134)
        self.assertEqual(set(candidate["hosts"]), set(binaryen.HOSTS))
        self.assertEqual(len(candidate["hosts"]["aarch64-apple-darwin"]["files"]), 2)


if __name__ == "__main__":
    unittest.main()
