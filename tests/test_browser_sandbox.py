"""Verify browser sandbox helper trust without changing the host security policy."""

import contextlib
import io
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
import verify_browser_sandbox


class BrowserSandboxTests(unittest.TestCase):
    """Reject unsafe helper metadata before the browser receives a privileged path."""

    def test_root_owned_setuid_executable_is_accepted(self):
        metadata = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o4755)

        with patch.object(Path, "lstat", return_value=metadata), patch.object(os, "access", return_value=True):
            result = verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

        self.assertIsNone(result)

    def test_unprivileged_owner_is_rejected(self):
        metadata = SimpleNamespace(st_uid=1000, st_mode=stat.S_IFREG | 0o4755)

        with patch.object(Path, "lstat", return_value=metadata):
            with self.assertRaisesRegex(ValueError, "must be owned by root"):
                verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_missing_setuid_is_rejected(self):
        metadata = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o0755)

        with patch.object(Path, "lstat", return_value=metadata):
            with self.assertRaisesRegex(ValueError, "must have the setuid bit"):
                verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_group_or_world_write_is_rejected(self):
        modes = [0o4775, 0o4757]

        for mode in modes:
            metadata = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | mode)
            with self.subTest(mode=oct(mode)), patch.object(Path, "lstat", return_value=metadata):
                with self.assertRaisesRegex(ValueError, "must not be group or world writable"):
                    verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_nonexecutable_mode_is_rejected(self):
        metadata = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o4644)

        with patch.object(Path, "lstat", return_value=metadata):
            with self.assertRaisesRegex(ValueError, "must be executable"):
                verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_execute_access_denial_is_rejected(self):
        metadata = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o4750)

        with patch.object(Path, "lstat", return_value=metadata), patch.object(os, "access", return_value=False):
            with self.assertRaisesRegex(ValueError, "must be executable"):
                verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_nonregular_file_types_are_rejected(self):
        kinds = [stat.S_IFDIR, stat.S_IFIFO, stat.S_IFSOCK]

        for kind in kinds:
            metadata = SimpleNamespace(st_uid=0, st_mode=kind | 0o4755)
            with self.subTest(kind=kind), patch.object(Path, "lstat", return_value=metadata):
                with self.assertRaisesRegex(ValueError, "must be a regular file"):
                    verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_symlink_to_an_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "real-helper"
            target.touch()
            target.chmod(0o755)
            helper = root / "chrome-sandbox"
            helper.symlink_to(target)

            with self.assertRaisesRegex(ValueError, "must not be a symlink"):
                verify_browser_sandbox.verify_helper(helper)

    def test_missing_helper_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            helper = Path(temporary) / "missing-helper"

            with self.assertRaisesRegex(ValueError, "cannot inspect sandbox helper"):
                verify_browser_sandbox.verify_helper(helper)

    def test_inspection_failure_is_reported(self):
        error = PermissionError(13, "Permission denied")

        with patch.object(Path, "lstat", side_effect=error):
            with self.assertRaisesRegex(ValueError, "cannot inspect sandbox helper.*Permission denied"):
                verify_browser_sandbox.verify_helper(Path("chrome-sandbox"))

    def test_cli_reports_a_validated_helper(self):
        metadata = SimpleNamespace(st_uid=0, st_mode=stat.S_IFREG | 0o4755)
        output = io.StringIO()

        with (
            patch.object(Path, "lstat", return_value=metadata),
            patch.object(os, "access", return_value=True),
            contextlib.redirect_stdout(output),
        ):
            result = verify_browser_sandbox.main(["chrome-sandbox"])

        self.assertEqual((result, output.getvalue()), (0, "Browser sandbox helper verified: chrome-sandbox\n"))

    def test_cli_missing_helper_fails_without_a_traceback(self):
        with tempfile.TemporaryDirectory() as temporary:
            helper = Path(temporary) / "missing-helper"

            result = subprocess.run(
                [sys.executable, str(SCRIPTS / "verify_browser_sandbox.py"), str(helper)],
                capture_output=True,
                text=True,
            )

        self.assertEqual(result.returncode, 1)
        self.assertIn("cannot inspect sandbox helper", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
