"""Verify pinned release trust, extraction and complete installations."""

import json
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import bootstrap_release as bootstrap
import check_pin
from sourcefield_tools import release
from sourcefield_tools.artifacts import digest_file
from sourcefield_tools.release import read_lock
from support.releases import make_assets, release_lock
from support.transports import installation_transport


class LockTests(unittest.TestCase):
    """Bind all supported platforms and workflow execution to one source identity."""

    def test_complete_lock_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "lock.json"
            path.write_text(json.dumps(release_lock()))

            result = read_lock(path)

            self.assertEqual(result["source_commit"], "a" * 40)

    def test_moving_release_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "lock.json"
            lock = release_lock()
            lock["release"] = "latest"
            path.write_text(json.dumps(lock))

            with self.assertRaisesRegex(ValueError, "exact version"):
                read_lock(path)

    def test_unknown_lock_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "lock.json"
            lock = release_lock()
            lock["disable_verification"] = True
            path.write_text(json.dumps(lock))

            with self.assertRaisesRegex(ValueError, "exactly"):
                read_lock(path)

    def test_workflow_mirror_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "lock.json").write_text(json.dumps(release_lock()))
            (root / "workflow.yml").write_text(
                "    uses: kdominic89/sourcefield/.github/workflows/generate.yml@"
                + "a" * 40
                + "\n"
            )

            check_pin.check_pin(root / "lock.json", root / "workflow.yml", "a" * 40)

    def test_own_workflow_revision_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "lock.json").write_text(json.dumps(release_lock()))
            (root / "workflow.yml").write_text(
                "    uses: kdominic89/sourcefield/.github/workflows/generate.yml@"
                + "a" * 40
                + "\n"
            )

            with self.assertRaisesRegex(ValueError, "executing reusable"):
                check_pin.check_pin(root / "lock.json", root / "workflow.yml", "c" * 40)

    def test_update_prepares_both_files_without_changing_workflow(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "lock.json").write_text(json.dumps(release_lock()))
            original = (
                "    uses: kdominic89/sourcefield/.github/workflows/generate.yml@"
                + "c" * 40
                + "\n"
            )

            (root / "workflow.yml").write_text(original)

            check_pin.prepare_update(
                root / "lock.json", root / "workflow.yml", root / "review"
            )

            self.assertEqual((root / "workflow.yml").read_text(), original)
            self.assertIn("a" * 40, (root / "review/workflow.yml").read_text())
            self.assertTrue((root / "review/sourcefield.lock.json").is_file())


class ExtractionTests(unittest.TestCase):
    """Treat authenticated archive paths as untrusted filesystem input too."""

    def test_plain_files_extract(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(root / "asset.zip", "w") as bundle:
                bundle.writestr("runtime/app.js", "synthetic")

            bootstrap.extract_archive(root / "asset.zip", root / "output")

            self.assertEqual((root / "output/runtime/app.js").read_text(), "synthetic")

    def test_traversal_fails_before_creating_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(root / "asset.zip", "w") as bundle:
                bundle.writestr("../escape", "unsafe")

            with self.assertRaisesRegex(ValueError, "unsafe archive"):
                bootstrap.extract_archive(root / "asset.zip", root / "output")

            self.assertFalse((root / "output").exists())
            self.assertFalse((root / "escape").exists())

    def test_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            link = zipfile.ZipInfo("link")
            link.create_system = 3
            link.external_attr = 0o120777 << 16
            with zipfile.ZipFile(root / "asset.zip", "w") as bundle:
                bundle.writestr(link, "/etc/passwd")

            with self.assertRaisesRegex(ValueError, "unsafe archive"):
                bootstrap.extract_archive(root / "asset.zip", root / "output")

    def test_case_alias_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(root / "asset.zip", "w") as bundle:
                bundle.writestr("app.js", "one")
                bundle.writestr("APP.js", "two")

            with self.assertRaisesRegex(ValueError, "unsafe archive"):
                bootstrap.extract_archive(root / "asset.zip", root / "output")

    def test_expansion_limit_is_enforced(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with zipfile.ZipFile(root / "asset.zip", "w") as bundle:
                bundle.writestr("large", b"12345")

            with (
                patch.object(bootstrap, "MAX_ARCHIVE_BYTES", 4),
                self.assertRaisesRegex(ValueError, "resource limits"),
            ):
                bootstrap.extract_archive(root / "asset.zip", root / "output")


class VerificationTests(unittest.TestCase):
    """Verify release trust and ensure failures precede installation."""

    def test_checksum_failure_prevents_external_commands(self):
        with tempfile.TemporaryDirectory() as temporary:
            asset = Path(temporary) / "asset.zip"
            asset.write_bytes(b"tampered")
            lock = release_lock()
            with patch.object(release.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "checksum"):
                    release.verify_asset(lock, asset, "browser")

                run.assert_not_called()

    def test_attestation_is_bound_to_workflow_and_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            asset = Path(temporary) / "asset.zip"
            asset.write_bytes(b"asset")
            lock = release_lock()
            lock["assets"]["browser"]["sha256"] = digest_file(asset)
            with patch.object(release.subprocess, "run") as run:
                release.verify_asset(lock, asset, "browser")

                commands = [call.args[0] for call in run.call_args_list]
                self.assertIn("verify-asset", commands[0])
                self.assertIn("--source-digest", commands[1])
                self.assertIn("--signer-digest", commands[1])
                self.assertIn(
                    "kdominic89/sourcefield/.github/workflows/release.yml", commands[1]
                )

    def test_failed_release_verification_preserves_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "install"
            with patch.object(
                bootstrap.subprocess,
                "run",
                side_effect=subprocess.CalledProcessError(1, "gh"),
            ):
                with self.assertRaises(subprocess.CalledProcessError):
                    bootstrap.install(
                        release_lock(), destination, "x86_64-unknown-linux-gnu"
                    )

                self.assertFalse(destination.exists())


class CompleteInstallationTests(unittest.TestCase):
    """Install matching native and browser assets together."""

    def test_complete_release_installs_matching_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = make_assets(root)
            with patch.object(
                bootstrap.subprocess, "run", side_effect=installation_transport(root)
            ):
                executable = bootstrap.install(
                    lock, root / "installed", "x86_64-unknown-linux-gnu"
                )

                self.assertEqual(executable.read_bytes(), b"synthetic executable")
                self.assertTrue(
                    (root / "installed/runtime/pkg/sourcefield_wasm_bg.wasm").is_file()
                )
                self.assertEqual(
                    json.loads((root / "installed/sourcefield.lock.json").read_text()),
                    lock,
                )

    def test_installation_keeps_previous_directory_intact(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "installed"
            destination.mkdir()
            (destination / "sourcefield").write_text("previous")

            with self.assertRaisesRegex(ValueError, "must not exist"):
                bootstrap.install(
                    release_lock(), destination, "x86_64-unknown-linux-gnu"
                )

            self.assertEqual((destination / "sourcefield").read_text(), "previous")


if __name__ == "__main__":
    unittest.main()
