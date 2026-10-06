"""Verify release publication, retries and immutable remote identities."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import release_publish
from support.releases import make_assets, release_lock
from support.releases import published_state
from support.transports import publication_transport


class ReleasePublicationTests(unittest.TestCase):
    """Publish only complete verified release candidates."""

    def test_publication_requires_confirmed_immutable_setup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = make_assets(root)
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

    def test_manual_release_creates_its_tag_only_after_complete_upload(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = make_assets(root)
            state = {}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"fail_on": ["release", "upload"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"draft": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"draft": True, "target": "c" * 40}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
            ):
                # Act
                with self.assertRaisesRegex(ValueError, "refusing to retarget"):
                    release_publish.publish(
                        root / "assets", lock["repository"], lock["source_commit"], lock["release"]
                    )

                # Assert
                self.assertFalse(state.get("tag", False))
                self.assertFalse(any(call.args[0][1:3] == ["release", "upload"] for call in run.call_args_list))

    def test_published_release_retry_verifies_without_mutation(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            lock = make_assets(root)
            state = published_state(root)

            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"fail_on": ["api", "--paginate"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"tag": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"draft": True, "tag": True, "tag_commit": "c" * 40}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"extra_asset": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"inventory_fault": fault}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"fail_on": ["attestation", "verify"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"fail_on": ["release", "verify"]}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            archive = root / "assets" / lock["assets"]["browser"]["name"]
            archive.write_bytes(archive.read_bytes() + b"tampered")
            state = {}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = published_state(root)
            state["tag_commit"] = "c" * 40
            with patch.object(release_publish.subprocess, "run",
                              side_effect=publication_transport(root, state)) as run:
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
            lock = make_assets(root)
            state = {"tag_after_upload": True}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            lock = make_assets(root)
            state = {"target_after_upload": "c" * 40}
            with (
                patch.dict("os.environ", {"SOURCEFIELD_IMMUTABLE_RELEASES": "true"}),
                patch.object(release_publish.subprocess, "run",
                             side_effect=publication_transport(root, state)) as run,
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
            # Act / Assert
            with self.assertRaisesRegex(ValueError, "invalid release identity returned by GitHub"):
                release_publish.existing_release("kdominic89/sourcefield", "v1.2.3")

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


if __name__ == "__main__":
    unittest.main()
