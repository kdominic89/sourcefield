"""Verify owner dispatch, fixed release inputs and consumer recovery ordering."""

import os
import subprocess
import sys
import tomllib
import unittest

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import SCRIPTS


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


@unittest.skipUnless(os.name == "posix", "The workflow Bash dispatch guard requires a POSIX shell.")
class ReleaseDispatchGuardTests(unittest.TestCase):
    """Execute the workflow's real Bash authorization guard on supported shell platforms."""

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
