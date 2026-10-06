"""Verify release trust, bounded extraction, pin consistency and failure isolation."""

import json
import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
import bootstrap_release as bootstrap
import check_pin
import consumer_candidate
import consumer_publish
import fixture_support
import release_browser_fixtures
import release_manifest
import release_package
import release_publish


def release_lock() -> dict:
    """Return a complete synthetic release identity, never a purported public release."""

    return {
        "schema_version": 1,
        "repository": "kdominic89/sourcefield",
        "source_commit": "a" * 40,
        "release": "v1.2.3",
        "workflow": ".github/workflows/release.yml",
        "assets": {
            target: {"name": f"sourcefield-{target}.zip", "sha256": "b" * 64}
            for target in set(bootstrap.TARGETS.values()) | {"browser"}
        },
    }


class LockTests(unittest.TestCase):
    """Bind all supported platforms and workflow execution to one source identity."""

    def test_complete_lock_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "lock.json"
            path.write_text(json.dumps(release_lock()))

            result = bootstrap.read_lock(path)

            self.assertEqual(result["source_commit"], "a" * 40)

    def test_moving_release_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "lock.json"
            lock = release_lock()
            lock["release"] = "latest"
            path.write_text(json.dumps(lock))

            with self.assertRaisesRegex(ValueError, "exact version"):
                bootstrap.read_lock(path)

    def test_unknown_lock_field_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "lock.json"
            lock = release_lock()
            lock["disable_verification"] = True
            path.write_text(json.dumps(lock))

            with self.assertRaisesRegex(ValueError, "exactly"):
                bootstrap.read_lock(path)

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
    """Verify external trust constraints and ensure failures precede installation."""

    def test_checksum_failure_prevents_external_commands(self):
        with tempfile.TemporaryDirectory() as temporary:
            asset = Path(temporary) / "asset.zip"
            asset.write_bytes(b"tampered")
            lock = release_lock()
            with patch.object(bootstrap.subprocess, "run") as run:
                with self.assertRaisesRegex(ValueError, "checksum"):
                    bootstrap.verify_asset(lock, asset, "browser")

                run.assert_not_called()

    def test_attestation_is_bound_to_workflow_and_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            asset = Path(temporary) / "asset.zip"
            asset.write_bytes(b"asset")
            lock = release_lock()
            lock["assets"]["browser"]["sha256"] = bootstrap.digest_file(asset)
            with patch.object(bootstrap.subprocess, "run") as run:
                bootstrap.verify_asset(lock, asset, "browser")

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

    def test_candidate_rejects_parent_paths(self):
        with self.assertRaisesRegex(ValueError, "consumer-relative"):
            consumer_candidate.relative_path("../README.md")


class CompleteReleaseTests(unittest.TestCase):
    """Exercise complete multi-artifact install and release assembly with a fake transport."""

    def make_assets(self, root: Path) -> dict:
        """Arrange a complete synthetic native/browser release with real ZIP metadata."""
        binary = root / "native"
        binary.write_bytes(b"synthetic executable")
        runtime = root / "runtime"
        for relative in bootstrap.RUNTIME_FILES:
            path = runtime / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic runtime")

        (runtime / "runtime-manifest.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "source_revision": "a" * 40,
                    "generator_version": "1.2.3",
                    "source_fingerprint": "c" * 64,
                    "files": {
                        name: bootstrap.digest_file(runtime / name)
                        for name in bootstrap.RUNTIME_FILES
                    },
                }
            )
        )

        for target in sorted(set(bootstrap.TARGETS.values()) | {"browser"}):
            release_package.package(
                root, root / "assets", target, "a" * 40, "v1.2.3", binary
            )

        lock_path = release_manifest.manifest(root / "assets", "a" * 40, "v1.2.3")

        return bootstrap.read_lock(lock_path)

    def fake_transport(self, root: Path):
        """Simulate gh downloads without weakening archive/digest/metadata checks."""

        def run(command, **kwargs):
            if command[1:3] == ["release", "download"]:
                name = command[command.index("--pattern") + 1]
                directory = Path(command[command.index("--dir") + 1])
                (directory / name).write_bytes((root / "assets" / name).read_bytes())

            return subprocess.CompletedProcess(
                command,
                0,
                stdout="sourcefield 1.2.3\n" if command[-1] == "--version" else "",
            )

        return run

    def test_complete_release_installs_matching_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            with patch.object(
                bootstrap.subprocess, "run", side_effect=self.fake_transport(root)
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

    def test_mixed_source_release_manifest_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.make_assets(root)
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

    def test_publication_requires_confirmed_immutable_setup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            with (
                patch.dict("os.environ", {}, clear=True),
                patch.object(release_publish.subprocess, "run") as run,
            ):
                with self.assertRaisesRegex(ValueError, "enable immutable releases"):
                    release_publish.publish(
                        root / "assets",
                        lock["repository"],
                        lock["source_commit"],
                        lock["release"],
                    )

                run.assert_not_called()

    def publication_transport(self, root: Path, state: dict):
        """Model pending tags, drafts, uploaded digests and failures without GitHub writes."""
        def run(command, **kwargs):
            if command[1:3] == state.get("fail_on"):
                if kwargs.get("check"):
                    raise subprocess.CalledProcessError(1, command)

                return subprocess.CompletedProcess(command, 1, stdout="", stderr="transport failed")

            output = ""
            if command[1] == "api" and "--paginate" in command:
                if state.get("draft") is not None:
                    assets = [dict(asset) for asset in state.get("assets", {}).values()]
                    fault = state.get("inventory_fault")
                    if assets and fault == "digest":
                        assets[0]["digest"] = "sha256:" + "0" * 64
                    elif assets and fault == "null":
                        assets[0]["digest"] = None
                    elif assets and fault == "size":
                        assets[0]["size"] += 1
                    elif assets and fault == "missing":
                        assets.pop()
                    elif assets and fault == "duplicate":
                        assets.append(dict(assets[0]))

                    if state.get("extra_asset"):
                        assets.append({"name": "unexpected.zip", "size": 1, "digest": "sha256:bad"})

                    output = json.dumps({
                        "draft": state["draft"], "target_commitish": state.get("target", "a" * 40),
                        "assets": assets,
                    })

            elif command[1] == "api" and "/git/matching-refs/" in command[2]:
                output = "refs/tags/v1.2.3" if state.get("tag") else ""
            elif command[1] == "api" and "/commits/" in command[2]:
                output = state.get("tag_commit", "a" * 40)
            elif command[1:3] == ["release", "create"]:
                state["draft"] = True
            elif command[1:3] == ["release", "upload"]:
                state["assets"] = {
                    path.name: {"name": path.name, "size": path.stat().st_size,
                                "digest": f"sha256:{bootstrap.digest_file(path)}"}
                    for path in (root / "assets").iterdir() if path.is_file()
                }

                if "target_after_upload" in state:
                    state["target"] = state["target_after_upload"]

                if state.get("tag_after_upload"):
                    state["tag"] = True

            elif command[1:3] == ["release", "edit"]:
                state["draft"] = False
                state["tag"] = True

            return subprocess.CompletedProcess(command, 0, stdout=output)

        return run

    def test_manual_release_creates_its_tag_only_after_complete_upload(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                release_publish.publish(
                    root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                )

                # Assert
                commands = [call.args[0] for call in run.call_args_list]
                mutations = [command for command in commands if command[1:3] in
                             (["release", "create"], ["release", "upload"], ["release", "edit"])]
                self.assertEqual([command[2] for command in mutations], ["create", "upload", "edit"])
                self.assertIn("--draft", mutations[0])
                self.assertNotIn("--verify-tag", mutations[0])
                self.assertEqual(mutations[0][mutations[0].index("--target") + 1], lock["source_commit"])
                self.assertTrue(state["tag"])
                self.assertFalse(state["draft"])
                self.assertEqual(set(state["assets"]), {path.name for path in (root / "assets").iterdir()})

    def test_failed_upload_leaves_a_draft_without_a_tag(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"fail_on": ["release", "upload"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaises(subprocess.CalledProcessError):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertTrue(state["draft"])
                self.assertFalse(state.get("tag", False))
                self.assertFalse(any(call.args[0][1:3] == ["release", "edit"] for call in run.call_args_list))

    def test_matching_draft_resumes_without_creating_another_release(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"draft": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                release_publish.publish(
                    root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                )

                # Assert
                self.assertTrue(state["tag"])
                self.assertFalse(any(call.args[0][1:3] == ["release", "create"] for call in run.call_args_list))

    def test_draft_for_another_commit_is_never_retargeted(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"draft": True, "target": "c" * 40}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "refusing to retarget"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertFalse(state.get("tag", False))
                self.assertFalse(any(call.args[0][1:3] == ["release", "upload"] for call in run.call_args_list))

    def published_state(self, root: Path) -> dict:
        """Arrange the complete remote inventory of an already-published synthetic release."""

        return {"draft": False, "tag": True, "assets": {
            path.name: {"name": path.name, "size": path.stat().st_size,
                        "digest": f"sha256:{bootstrap.digest_file(path)}"}
            for path in (root / "assets").iterdir()
        }}

    def test_published_release_retry_verifies_without_mutation(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = self.published_state(root)

            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                release_publish.publish(
                    root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                )

                # Assert
                commands = [call.args[0] for call in run.call_args_list]
                self.assertTrue(any(command[1:3] == ["release", "verify"] for command in commands))
                self.assertFalse(any(command[1:3] in (["release", "create"], ["release", "upload"],
                                                    ["release", "edit"]) for command in commands))

    def test_api_failure_is_not_treated_as_an_absent_release(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"fail_on": ["api", "--paginate"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaises(subprocess.CalledProcessError):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertNotIn("draft", state)
                self.assertFalse(any(call.args[0][1:3] == ["release", "create"] for call in run.call_args_list))

    def test_preexisting_tag_without_a_release_is_not_reused(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"tag": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "refusing to reuse"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertNotIn("draft", state)
                self.assertFalse(any(call.args[0][1:3] == ["release", "create"] for call in run.call_args_list))

    def test_resumed_draft_with_a_preexisting_tag_is_never_published(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"draft": True, "tag": True, "tag_commit": "c" * 40}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "refusing to reuse"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertTrue(state["draft"])
                self.assertFalse(any(call.args[0][1:3] in (["release", "upload"], ["release", "edit"])
                                     for call in run.call_args_list))

    def test_extra_remote_asset_prevents_tag_creation(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"extra_asset": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "complete verified candidate"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertTrue(state["draft"])
                self.assertFalse(state.get("tag", False))
                self.assertFalse(any(call.args[0][1:3] == ["release", "edit"] for call in run.call_args_list))

    def assert_inventory_failure_keeps_draft(self, fault: str) -> None:
        """Exercise one remote inventory defect per independent test, with one AAA sequence."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"inventory_fault": fault}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaises(ValueError):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertTrue(state["draft"])
                self.assertFalse(state.get("tag", False))
                self.assertFalse(any(call.args[0][1:3] == ["release", "edit"] for call in run.call_args_list))

    def test_same_name_wrong_digest_keeps_the_draft_unpublished(self):
        self.assert_inventory_failure_keeps_draft("digest")

    def test_missing_remote_digest_keeps_the_draft_unpublished(self):
        self.assert_inventory_failure_keeps_draft("null")

    def test_wrong_remote_size_keeps_the_draft_unpublished(self):
        self.assert_inventory_failure_keeps_draft("size")

    def test_missing_remote_asset_keeps_the_draft_unpublished(self):
        self.assert_inventory_failure_keeps_draft("missing")

    def test_duplicate_remote_name_keeps_the_draft_unpublished(self):
        self.assert_inventory_failure_keeps_draft("duplicate")

    def test_failed_provenance_verification_prevents_draft_creation(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"fail_on": ["attestation", "verify"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaises(subprocess.CalledProcessError):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertNotIn("draft", state)
                self.assertFalse(any(call.args[0][1:3] == ["release", "create"] for call in run.call_args_list))

    def test_final_verification_failure_never_overwrites_the_published_release(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"fail_on": ["release", "verify"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaises(subprocess.CalledProcessError):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertTrue(state["tag"])
                self.assertFalse(state["draft"])
                self.assertEqual(sum(call.args[0][1:3] == ["release", "edit"] for call in run.call_args_list), 1)

    def test_modified_local_archive_prevents_release_creation(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            archive = root / "assets" / lock["assets"]["browser"]["name"]
            archive.write_bytes(archive.read_bytes() + b"tampered")
            state = {}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "archive changed"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertNotIn("draft", state)
                self.assertFalse(any(call.args[0][1:3] == ["release", "create"] for call in run.call_args_list))


    def test_published_tag_must_resolve_to_the_attested_commit(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = self.published_state(root)
            state["tag_commit"] = "c" * 40
            with patch.object(release_publish.subprocess, "run",
                              side_effect=self.publication_transport(root, state)) as run:
                # Act
                with self.assertRaisesRegex(ValueError, "tag does not resolve to the attested source commit"):
                    release_publish.verify_published(root / "assets", lock)

                # Assert
                commands = [call.args[0] for call in run.call_args_list]
                self.assertIn(
                    ["gh", "api", "repos/kdominic89/sourcefield/commits/tags/v1.2.3", "--jq", ".sha"],
                    commands,
                )
                self.assertTrue(all(command[1] == "api" for command in commands))
                self.assertTrue(state["tag"])
                self.assertFalse(state["draft"])

    def test_tag_created_during_upload_prevents_publication(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"tag_after_upload": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "refusing to reuse"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                commands = [call.args[0] for call in run.call_args_list]
                self.assertTrue(any(command[1:3] == ["release", "upload"] for command in commands))
                self.assertEqual(sum("/git/matching-refs/" in command[2] for command in commands), 2)
                self.assertFalse(any(command[1:3] == ["release", "edit"] for command in commands))
                self.assertTrue(state["draft"])

    def test_changed_draft_target_after_upload_prevents_publication(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = self.make_assets(root)
            state = {"target_after_upload": "c" * 40}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=self.publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "draft identity changed before publication"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                commands = [call.args[0] for call in run.call_args_list]
                self.assertTrue(any(command[1:3] == ["release", "upload"] for command in commands))
                self.assertFalse(any(command[1:3] == ["release", "edit"] for command in commands))
                self.assertTrue(state["draft"])
                self.assertFalse(state.get("tag", False))


class ReleaseApiTests(unittest.TestCase):
    """Cover successful absence and fail-closed release identities with read-only transports."""

    def test_multiple_matching_drafts_are_rejected(self):
        # Arrange
        draft = json.dumps({"draft": True, "target_commitish": "a" * 40, "assets": []})
        response = subprocess.CompletedProcess("gh", 0, stdout=f"{draft}\n{draft}\n")
        with patch.object(release_publish.subprocess, "run", return_value=response) as run:
            # Act
            with self.assertRaisesRegex(ValueError, "multiple releases claim the requested version"):
                release_publish.existing_release("kdominic89/sourcefield", "v1.2.3")

            # Assert
            run.assert_called_once()
            self.assertTrue(run.call_args.kwargs["check"])

    def test_invalid_release_identity_is_rejected(self):
        # Arrange
        response = subprocess.CompletedProcess("gh", 0, stdout=json.dumps({"draft": True, "target_commitish": 1}))
        with patch.object(release_publish.subprocess, "run", return_value=response):
            # Act
            with self.assertRaisesRegex(ValueError, "invalid release identity returned by GitHub") as error:
                release_publish.existing_release("kdominic89/sourcefield", "v1.2.3")

            # Assert
            self.assertIn("invalid release identity", str(error.exception))

    def assert_invalid_publication_identity(self, existing: dict | None) -> None:
        """Require final identity validation before any inventory lookup or remote verification."""
        # Arrange
        with (
            patch.object(release_publish, "existing_release", return_value=existing),
            patch.object(release_publish, "verify_inventory") as inventory,
            patch.object(release_publish.subprocess, "run") as run,
        ):
            # Act
            with self.assertRaisesRegex(ValueError, "published release identity differs"):
                release_publish.verify_published(Path("unused"), release_lock())

            # Assert
            inventory.assert_not_called()
            run.assert_not_called()

    def test_missing_published_release_is_rejected(self):
        self.assert_invalid_publication_identity(None)

    def test_unpublished_draft_is_rejected_by_final_verification(self):
        self.assert_invalid_publication_identity({"draft": True, "target_commitish": "a" * 40})

    def test_published_release_for_another_commit_is_rejected(self):
        self.assert_invalid_publication_identity({"draft": False, "target_commitish": "c" * 40})

    def test_unused_tag_preflight_requires_only_read_access(self):
        # Arrange
        arguments = ["release_publish.py", "--check-unpublished", "--repository", "kdominic89/sourcefield",
                     "--release", "v1.2.3"]
        response = subprocess.CompletedProcess("gh", 0, stdout="")
        with (
            patch.object(sys, "argv", arguments),
            patch.dict("os.environ", {}, clear=True),
            patch.object(release_publish.subprocess, "run", return_value=response) as run,
        ):
            # Act
            result = release_publish.main()

            # Assert
            self.assertEqual(result, 0)
            run.assert_called_once()
            command = run.call_args.args[0]
            self.assertEqual(command[:3], ["gh", "api", "repos/kdominic89/sourcefield/git/matching-refs/tags/v1.2.3"])
            self.assertIn('select(.ref == "refs/tags/v1.2.3")', command[-1])
            self.assertTrue(run.call_args.kwargs["check"])

    def test_existing_tag_fails_preflight_without_builds_or_mutation(self):
        # Arrange
        arguments = ["release_publish.py", "--check-unpublished", "--repository", "kdominic89/sourcefield",
                     "--release", "v1.2.3"]
        response = subprocess.CompletedProcess("gh", 0, stdout="refs/tags/v1.2.3\n")
        with (
            patch.object(sys, "argv", arguments),
            patch.object(release_publish.subprocess, "run", return_value=response) as run,
        ):
            # Act
            with self.assertRaisesRegex(ValueError, "refusing to reuse"):
                release_publish.main()

            # Assert
            run.assert_called_once()
            self.assertEqual(run.call_args.args[0][1], "api")

    def test_tag_lookup_failure_is_not_treated_as_an_unused_version(self):
        # Arrange
        arguments = ["release_publish.py", "--check-unpublished", "--repository", "kdominic89/sourcefield",
                     "--release", "v1.2.3"]
        with (
            patch.object(sys, "argv", arguments),
            patch.object(release_publish.subprocess, "run",
                         side_effect=subprocess.CalledProcessError(1, "gh")) as run,
        ):
            # Act
            with self.assertRaises(subprocess.CalledProcessError):
                release_publish.main()

            # Assert
            run.assert_called_once()



class ManualReleaseWorkflowTests(unittest.TestCase):
    """Keep the single owner dispatch, fixed commit and pre-build version check wired together."""

    def test_version_preflight_accepts_the_workspace_version_without_assets(self):
        # Arrange
        version = tomllib.loads((SCRIPTS.parent / "Cargo.toml").read_text(encoding="ascii"))[
            "workspace"]["package"]["version"]
        command = [sys.executable, "-B", str(SCRIPTS / "release_manifest.py"),
                   "--release", f"v{version}", "--check-version"]

        # Act
        result = subprocess.run(command, check=False, capture_output=True, text=True)

        # Assert
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"v{version}\n")

    def test_version_preflight_rejects_an_unmatched_request_before_building(self):
        # Arrange
        command = [sys.executable, "-B", str(SCRIPTS / "release_manifest.py"),
                   "--release", "v9.9.9", "--check-version"]

        # Act
        result = subprocess.run(command, check=False, capture_output=True, text=True)

        # Assert
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must match the workspace package version", result.stderr)
        self.assertEqual(result.stdout, "")

    def assert_rejected_arguments(self, script: str, arguments: list[str], diagnostic: str) -> None:
        """Run an invalid CLI invocation without touching GitHub or creating release files."""
        # Arrange
        version = tomllib.loads((SCRIPTS.parent / "Cargo.toml").read_text(encoding="ascii"))[
            "workspace"]["package"]["version"]
        command = [sys.executable, "-B", str(SCRIPTS / script), "--release", f"v{version}", *arguments]

        # Act
        result = subprocess.run(command, check=False, capture_output=True, text=True)

        # Assert
        self.assertEqual(result.returncode, 2)
        self.assertIn(diagnostic, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_version_check_rejects_an_assembly_directory(self):
        self.assert_rejected_arguments("release_manifest.py", ["--check-version", "--directory", "unused"],
                                       "not allowed with argument")

    def test_version_check_rejects_an_ignored_source_commit(self):
        self.assert_rejected_arguments("release_manifest.py", ["--check-version", "--source-commit", "a" * 40],
                                       "--source-commit is not accepted with --check-version")

    def test_assembly_requires_a_source_commit(self):
        self.assert_rejected_arguments("release_manifest.py", ["--directory", "unused"],
                                       "--source-commit is required for release assembly")

    def test_assembly_requires_a_directory(self):
        self.assert_rejected_arguments("release_manifest.py", ["--source-commit", "a" * 40],
                                       "one of the arguments --directory --check-version is required")

    def test_tag_check_rejects_an_ignored_source_commit(self):
        self.assert_rejected_arguments("release_publish.py", ["--check-unpublished", "--repository", "unused",
                                       "--source-commit", "a" * 40],
                                       "--source-commit is not accepted with --check-unpublished")

    def test_tag_check_rejects_a_publication_directory(self):
        self.assert_rejected_arguments("release_publish.py", ["--check-unpublished", "--repository", "unused",
                                       "--directory", "unused"], "not allowed with argument")

    def test_publication_requires_a_source_commit(self):
        self.assert_rejected_arguments("release_publish.py", ["--directory", "unused", "--repository", "unused"],
                                       "--source-commit is required for release publication")

    def assert_dispatch_guard(self, overrides: dict, succeeds: bool, diagnostic: str = "") -> None:
        """Execute the workflow's actual authorization block with synthetic actor/ref values."""
        # Arrange
        source = (SCRIPTS.parent / ".github/workflows/release.yml").read_text(encoding="ascii")
        prepare = source.split("  prepare:\n", 1)[1].split("  verify:\n", 1)[0]
        block = prepare.split("        run: |\n", 1)[1].split("      - uses:", 1)[0]
        script = "\n".join(line[10:] for line in block.splitlines())
        environment = dict(os.environ, RELEASE_REF="refs/heads/main", RELEASE_ACTOR="owner",
                           RELEASE_TRIGGERING_ACTOR="owner", RELEASE_OWNER="owner")
        environment.update(overrides)

        # Act
        result = subprocess.run(["bash", "-e", "-c", script], env=environment, check=False,
                                capture_output=True, text=True)

        # Assert
        self.assertEqual(result.returncode, 0 if succeeds else 1)
        self.assertEqual(result.stdout, "")
        self.assertIn(diagnostic, result.stderr)

    def test_owner_dispatch_on_main_passes_the_actual_guard(self):
        self.assert_dispatch_guard({}, True)

    def test_wrong_branch_fails_the_actual_guard_visibly(self):
        self.assert_dispatch_guard({"RELEASE_REF": "refs/heads/feature/test"}, False,
                                   "Release dispatch must select main.")

    def test_unauthorized_original_actor_fails_the_actual_guard(self):
        self.assert_dispatch_guard({"RELEASE_ACTOR": "contributor"}, False, "Only the repository owner")

    def test_unauthorized_retry_actor_fails_the_actual_guard(self):
        self.assert_dispatch_guard({"RELEASE_TRIGGERING_ACTOR": "contributor"}, False, "Only the repository owner")

    def test_dispatch_preflight_and_publish_retry_guards_remain_wired(self):
        # Arrange
        source = (SCRIPTS.parent / ".github/workflows/release.yml").read_text(encoding="ascii")

        # Act
        prepare = source.split("  prepare:\n", 1)[1].split("  verify:\n", 1)[0]
        publish = source.split("  publish:\n", 1)[1]
        guard = " ".join(line.strip() for line in publish.splitlines() if line.startswith("      github."))

        # Assert
        self.assertIn("workflow_dispatch:", source)
        self.assertNotIn("  push:", source)
        self.assertNotIn("        default:", source)
        self.assertIn("needs: [prepare, native, browser]", publish)
        self.assertIn("needs: prepare", source)
        self.assertIn('--release "$RELEASE_TAG" --check-version', prepare)
        self.assertIn('--check-unpublished --repository "$RELEASE_REPOSITORY" --release "$RELEASE_TAG"', prepare)
        self.assertIn("GH_TOKEN: ${{ github.token }}", prepare)
        self.assertIn("if: github.repository == 'kdominic89/sourcefield'", prepare)
        self.assertIn("RELEASE_REF: ${{ github.ref }}", prepare)
        self.assertIn("RELEASE_ACTOR: ${{ github.actor }}", prepare)
        self.assertIn("RELEASE_TRIGGERING_ACTOR: ${{ github.triggering_actor }}", prepare)
        self.assertIn("RELEASE_OWNER: ${{ github.repository_owner }}", prepare)
        self.assertEqual(guard, (
            "github.repository == 'kdominic89/sourcefield' && github.ref == 'refs/heads/main' && "
            "github.actor == github.repository_owner && github.triggering_actor == github.repository_owner"
        ))
        self.assertEqual(source.count("ref: ${{ github.sha }}"), 4)
        self.assertNotIn("github.ref_name", source)


class ConsumerPublicationTests(unittest.TestCase):
    """Refuse stale revisions and tampered candidates before mutating a consumer."""

    def git_transport(self, command, **kwargs):
        """Simulate revision checks and staging while keeping filesystem operations real."""
        if command[1] == "rev-parse":
            return subprocess.CompletedProcess(command, 0, stdout="a" * 40)

        if command[1] == "ls-remote":
            return subprocess.CompletedProcess(
                command, 0, stdout="a" * 40 + " refs/heads/main"
            )

        return subprocess.CompletedProcess(
            command, 1 if command[1] == "check-ignore" else 0
        )

    def test_explicit_authored_readme_is_staged_without_becoming_deletion_owned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / "README.md").write_text("authored prose and managed output")
            digest = bootstrap.digest_file(candidate / "README.md")
            (candidate / consumer_publish.MANIFEST).write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "files": {},
                        "authored_files": {"README.md": digest},
                    }
                )
            )
            with patch.object(
                consumer_publish.subprocess, "run", side_effect=self.git_transport
            ):
                staged = consumer_publish.apply_candidate(
                    root, candidate, "a" * 40, "main"
                )

                self.assertIn("README.md", staged)
                self.assertEqual(
                    (root / "README.md").read_text(),
                    "authored prose and managed output",
                )
                self.assertEqual(consumer_publish.read_ownership(root)["files"], {})

    def test_omitted_previous_readme_is_never_deleted_or_staged(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "candidate"
            candidate.mkdir()
            (root / "README.md").write_text("preserve authored text")
            (root / consumer_publish.MANIFEST).write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "files": {},
                        "authored_files": {
                            "README.md": bootstrap.digest_file(root / "README.md")
                        },
                    }
                )
            )
            (candidate / "output.svg").write_text("generated")
            (candidate / consumer_publish.MANIFEST).write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "files": {
                            "output.svg": bootstrap.digest_file(
                                candidate / "output.svg"
                            )
                        },
                        "authored_files": {},
                    }
                )
            )
            with patch.object(
                consumer_publish.subprocess, "run", side_effect=self.git_transport
            ):
                staged = consumer_publish.apply_candidate(
                    root, candidate, "a" * 40, "main"
                )

                self.assertNotIn("README.md", staged)
                self.assertEqual(
                    (root / "README.md").read_text(), "preserve authored text"
                )

    def test_stale_remote_head_prevents_any_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "candidate"
            candidate.mkdir()
            values = [
                subprocess.CompletedProcess("git", 0, stdout="a" * 40),
                subprocess.CompletedProcess(
                    "git", 0, stdout="b" * 40 + " refs/heads/main"
                ),
            ]

            with patch.object(consumer_publish.subprocess, "run", side_effect=values):
                with self.assertRaisesRegex(ValueError, "revision changed"):
                    consumer_publish.apply_candidate(root, candidate, "a" * 40, "main")

                self.assertFalse((root / consumer_publish.MANIFEST).exists())

    def test_changed_candidate_file_is_rejected_before_copy(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / "output.svg").write_text("modified after validation")
            (candidate / consumer_publish.MANIFEST).write_text(
                json.dumps({"schema_version": 1, "files": {"output.svg": "b" * 64}})
            )
            values = [
                subprocess.CompletedProcess("git", 0, stdout="a" * 40),
                subprocess.CompletedProcess(
                    "git", 0, stdout="a" * 40 + " refs/heads/main"
                ),
            ]

            with patch.object(consumer_publish.subprocess, "run", side_effect=values):
                with self.assertRaisesRegex(ValueError, "changed after validation"):
                    consumer_publish.apply_candidate(root, candidate, "a" * 40, "main")

                self.assertFalse((root / "output.svg").exists())


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


class PortablePathTests(unittest.TestCase):
    """Use one slash-based protocol on all native host platforms."""

    def test_accepts_portable_nested_readme(self):
        result = consumer_candidate.relative_path("profile/README.md")

        self.assertEqual(result, "profile/README.md")

    def test_colon_diagnostic_names_offending_path_and_portability_rule(self):
        with self.assertRaisesRegex(ValueError, "docs/example:note.md.*colon.*portable"):
            consumer_candidate.relative_path("docs/example:note.md")

    def test_nonportable_tracked_input_fails_before_candidate_mutation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "candidate"
            tracked = subprocess.CompletedProcess("git", 0, stdout=b"README.md\0docs/example:note.md\0")
            with patch.object(consumer_candidate.subprocess, "run", return_value=tracked) as run:
                with self.assertRaisesRegex(ValueError, "docs/example:note.md.*colon"):
                    consumer_candidate.candidate(root, destination, root / "installed",
                                                 "config/profile.toml", ["README.md"], True, False)

                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args.args[0], ["git", "ls-files", "-z"])
            self.assertFalse(destination.exists())

    def test_rejects_platform_escapes_and_normalization(self):
        values = ["../x", "/x", "C:/x", "C:x", "a\\b", "//host/share", "a//b",
                  "a/./b", "a/../b", ".git/config", "a/.GIT/config", "a/", "", "a\x00b"]

        for value in values:
            with self.subTest(value=value), self.assertRaises(ValueError):
                consumer_candidate.relative_path(value)


class CandidateProvenanceTests(unittest.TestCase):
    """Keep real committed source identity available to canonical import verification."""

    def committed_consumer(self, root: Path) -> tuple[Path, str]:
        """Arrange an isolated organization checkout with a genuine committed manifest."""

        source = root / "source"
        source.mkdir()
        (source / "config").mkdir()
        # Git autocrlf normalizes CRLF on commit; canonical provenance requires identical bytes.
        (source / "config/organization.toml").write_text(
            'schema_version = 1\nid = "example-labs"\n',
            encoding="utf-8",
            newline="\n",
        )
        subprocess.run(["git", "init", "--quiet", str(source)], check=True)
        subprocess.run(
            [
                "git",
                "remote",
                "add",
                "origin",
                "https://github.com/example-labs/.github.git",
            ],
            cwd=source,
            check=True,
        )
        subprocess.run(
            ["git", "add", "config/organization.toml"], cwd=source, check=True
        )
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "test: synthetic canonical manifest",
            ],
            cwd=source,
            check=True,
        )
        # Hosted checkout uses detached HEAD; preserve that source identity as well.
        subprocess.run(["git", "checkout", "--quiet", "--detach"], cwd=source, check=True)
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=source,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

        return source, commit

    def test_windows_installation_selects_exe(self):
        with tempfile.TemporaryDirectory() as temporary:
            installation = Path(temporary)
            (installation / "sourcefield.exe").touch()

            executable = consumer_candidate.installed_executable(installation)

            self.assertEqual(executable.name, "sourcefield.exe")

    def test_ambiguous_installation_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            installation = Path(temporary)
            (installation / "sourcefield").touch()
            (installation / "sourcefield.exe").touch()

            with self.assertRaisesRegex(ValueError, "exactly one"):
                consumer_candidate.installed_executable(installation)

    def test_failed_generation_never_leaves_git_metadata_in_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, _ = self.committed_consumer(root)
            destination = root / "candidate"
            installation = root / "installation"
            installation.mkdir()
            (installation / "sourcefield").touch()
            run = subprocess.run

            def execute(command, **kwargs):
                if command[0] == str(installation / "sourcefield"):
                    raise subprocess.CalledProcessError(1, "sourcefield generate")

                return run(command, **kwargs)

            with patch.object(
                consumer_candidate.subprocess, "run", side_effect=execute
            ), self.assertRaises(subprocess.CalledProcessError):
                consumer_candidate.candidate(
                    source,
                    destination,
                    installation,
                    "config/profile.toml",
                    [],
                    False,
                    False,
                )

            self.assertFalse((destination / ".git").exists())
            self.assertTrue((source / ".git").is_dir())

    def test_canonical_manifest_has_authenticated_identity_during_generation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, commit = self.committed_consumer(root)
            destination = root / "candidate"
            installation = root / "installation"
            installation.mkdir()
            (installation / "sourcefield").touch()
            observed = []
            run = subprocess.run

            def execute(command, **kwargs):
                if command[0] == str(installation / "sourcefield"):
                    head = run(
                        ["git", "rev-parse", "HEAD"],
                        cwd=destination,
                        check=True,
                        capture_output=True,
                        text=True,
                    ).stdout.strip()

                    origin = run(
                        ["git", "remote", "get-url", "origin"],
                        cwd=destination,
                        check=True,
                        capture_output=True,
                        text=True,
                    ).stdout.strip()

                    committed = run(
                        ["git", "show", f"{head}:config/organization.toml"],
                        cwd=destination,
                        check=True,
                        capture_output=True,
                    ).stdout

                    observed.append((head, origin, committed))

                    return subprocess.CompletedProcess(command, 0)

                return run(command, **kwargs)

            with patch.object(
                consumer_candidate.subprocess, "run", side_effect=execute
            ):
                consumer_candidate.candidate(
                    source,
                    destination,
                    installation,
                    "config/profile.toml",
                    [],
                    False,
                    False,
                )

            self.assertEqual(len(observed), 2)
            self.assertTrue(all(item[0] == commit for item in observed))
            self.assertTrue(
                all(
                    item[1] == "https://github.com/example-labs/.github.git"
                    for item in observed
                )
            )
            self.assertTrue(
                all(
                    item[2] == (source / "config/organization.toml").read_bytes()
                    for item in observed
                )
            )
            self.assertFalse((destination / ".git").exists())
            self.assertTrue((source / ".git").is_dir())

    def test_canonical_manifest_preserves_bytes_with_windows_autocrlf(self):
        """Keep committed and generated manifest bytes equal under Windows text defaults."""

        with tempfile.TemporaryDirectory() as temporary, patch.dict(
            os.environ,
            {
                "GIT_CONFIG_COUNT": "1",
                "GIT_CONFIG_KEY_0": "core.autocrlf",
                "GIT_CONFIG_VALUE_0": "true",
            },
        ):
            root = Path(temporary)
            manifest = b'schema_version = 1\nid = "example-labs"\n'
            write_text = Path.write_text

            def write_windows_text(path: Path, data: str, **kwargs) -> int:
                """Simulate Windows translation unless a caller selects an explicit newline."""

                if kwargs.get("newline") is None:
                    kwargs["newline"] = "\r\n"

                return write_text(path, data, **kwargs)

            with patch.object(Path, "write_text", new=write_windows_text):
                source, commit = self.committed_consumer(root)

            destination = root / "candidate"
            installation = root / "installation"
            installation.mkdir()
            (installation / "sourcefield").touch()
            observed = []
            run = subprocess.run
            autocrlf = run(
                ["git", "config", "--get", "core.autocrlf"],
                cwd=source,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()

            def execute(command, **kwargs):
                """Read real Git provenance at each native generation or validation boundary."""

                if command[0] == str(installation / "sourcefield"):
                    committed = run(
                        ["git", "show", f"{commit}:config/organization.toml"],
                        cwd=destination,
                        check=True,
                        capture_output=True,
                    ).stdout
                    generated = (destination / "config/organization.toml").read_bytes()
                    observed.append((committed, generated))

                    return subprocess.CompletedProcess(command, 0)

                return run(command, **kwargs)

            with patch.object(
                consumer_candidate.subprocess, "run", side_effect=execute
            ):
                consumer_candidate.candidate(
                    source,
                    destination,
                    installation,
                    "config/profile.toml",
                    [],
                    False,
                    False,
                )

            self.assertEqual(autocrlf, "true")
            self.assertEqual((source / "config/organization.toml").read_bytes(), manifest)
            self.assertEqual(observed, [(manifest, manifest), (manifest, manifest)])
            self.assertFalse((destination / ".git").exists())
            self.assertTrue((source / ".git").is_dir())


class ConsumerWorkflowRecoveryTests(unittest.TestCase):
    """Keep artifact preparation before Git publication and deployment independently retryable."""

    def test_pages_upload_precedes_publication_and_deploy_is_separate(self):
        template = (SCRIPTS.parent / "docs/consumer-workflow.yml.template").read_text()

        upload = template.index("uses: actions/upload-pages-artifact@")
        publication = template.index("- name: Apply only owned files")
        deployment = template.index("  deploy:\n    needs: publish")

        self.assertLess(upload, publication)
        self.assertLess(publication, deployment)
        self.assertNotIn("git push", template[deployment:])
        self.assertNotIn("consumer_publish.py", template[deployment:])
        self.assertIn("artifact_name: ${{ needs.publish.outputs.pages_artifact }}", template[deployment:])
        self.assertIn("pages_artifact: ${{ steps.artifact_name.outputs.name }}", template[:deployment])


if __name__ == "__main__":
    unittest.main()
