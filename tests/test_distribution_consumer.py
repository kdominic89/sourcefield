"""Verify portable consumer candidates, provenance and scoped publication."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import consumer_candidate
import consumer_publish
from sourcefield_tools.artifacts import digest_file
from sourcefield_tools.consumer import relative_path
from support.transports import consumer_git_transport


class ConsumerPublicationTests(unittest.TestCase):
    """Refuse stale revisions and tampered candidates before mutating a consumer."""

    def test_explicit_authored_readme_is_staged_without_becoming_deletion_owned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / "README.md").write_text("authored prose and managed output")
            digest = digest_file(candidate / "README.md")
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
                consumer_publish.subprocess, "run", side_effect=consumer_git_transport
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
                            "README.md": digest_file(root / "README.md")
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
                            "output.svg": digest_file(
                                candidate / "output.svg"
                            )
                        },
                        "authored_files": {},
                    }
                )
            )
            with patch.object(
                consumer_publish.subprocess, "run", side_effect=consumer_git_transport
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


class CandidateBoundaryTests(unittest.TestCase):
    """Reject candidate paths that would escape the consumer."""

    def test_candidate_rejects_parent_paths(self):
        with self.assertRaisesRegex(ValueError, "consumer-relative"):
            relative_path("../README.md")


class PortablePathTests(unittest.TestCase):
    """Use one slash-based protocol on all native host platforms."""

    def test_accepts_portable_nested_readme(self):
        result = relative_path("profile/README.md")

        self.assertEqual(result, "profile/README.md")

    def test_colon_diagnostic_names_offending_path_and_portability_rule(self):
        with self.assertRaisesRegex(ValueError, "docs/example:note.md.*colon.*portable"):
            relative_path("docs/example:note.md")

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

    def assert_portable_path_rejected(self, value: str, diagnostic: str) -> None:
        """Reject one consumer path without normalizing away its unsafe original spelling."""
        # Arrange
        source = value

        # Act
        with self.assertRaises(ValueError) as error:
            relative_path(source)

        # Assert
        self.assertIn(diagnostic, str(error.exception))
        self.assertIn(repr(source), str(error.exception))

    def test_rejects_portable_path_parent_escape(self):
        """Reject a path beginning with parent traversal."""
        self.assert_portable_path_rejected('../x', 'parent traversal')

    def test_rejects_portable_path_absolute(self):
        """Reject an absolute path."""
        self.assert_portable_path_rejected('/x', 'absolute paths')

    def test_rejects_portable_path_drive_absolute(self):
        """Reject an absolute Windows drive path."""
        self.assert_portable_path_rejected('C:/x', 'colon')

    def test_rejects_portable_path_drive_relative(self):
        """Reject a relative Windows drive path."""
        self.assert_portable_path_rejected('C:x', 'colon')

    def test_rejects_portable_path_backslash_separator(self):
        """Require slash separators on every platform."""
        self.assert_portable_path_rejected('a\\b', 'backslash')

    def test_rejects_portable_path_network_share(self):
        """Reject a network-share path."""
        self.assert_portable_path_rejected('//host/share', 'absolute paths')

    def test_rejects_portable_path_repeated_separator(self):
        """Reject repeated separators instead of normalizing them."""
        self.assert_portable_path_rejected('a//b', 'normalized')

    def test_rejects_portable_path_current_directory_segment(self):
        """Reject an explicit current-directory segment."""
        self.assert_portable_path_rejected('a/./b', 'normalized')

    def test_rejects_portable_path_nested_parent_segment(self):
        """Reject a nested parent-directory segment."""
        self.assert_portable_path_rejected('a/../b', 'parent traversal')

    def test_rejects_portable_path_git_metadata(self):
        """Reject the Git metadata directory."""
        self.assert_portable_path_rejected('.git/config', 'Git metadata')

    def test_rejects_portable_path_case_aliased_git_metadata(self):
        """Reject a case alias of the Git metadata directory."""
        self.assert_portable_path_rejected('a/.GIT/config', 'Git metadata')

    def test_rejects_portable_path_trailing_separator(self):
        """Reject trailing separators."""
        self.assert_portable_path_rejected('a/', 'normalized')

    def test_rejects_portable_path_empty(self):
        """Reject an empty consumer path."""
        self.assert_portable_path_rejected('', 'nonempty')

    def test_rejects_portable_path_null_byte(self):
        """Reject a null byte before interpreting the consumer path."""
        self.assert_portable_path_rejected('a\x00b', 'control characters')


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


if __name__ == "__main__":
    unittest.main()
