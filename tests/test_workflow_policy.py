"""Check the current consumer workflow source contract independently of artifact admission."""

from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
from sourcefield_tools.workflow_policy import validate_workflows


COMMIT = "a" * 40


class WorkflowPolicyTests(unittest.TestCase):
    """Exercise the distributed template and fail closed on mutable or ambiguous identities."""

    def setUp(self):
        """Create an isolated consumer with the actual pinned distributed workflow template."""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.workflows = self.root / ".github/workflows"
        self.workflows.mkdir(parents=True)
        self.update = self.workflows / "update-profile.yml"
        self.template = (ROOT / "docs/consumer-workflow.yml.template").read_text(encoding="utf-8")
        self.update.write_text(self.template.replace("SOURCEFIELD_COMMIT", COMMIT), encoding="utf-8")

    def append_step(self, text: str):
        """Add a source-policy fixture step without changing existing template references."""
        with self.update.open("a", encoding="utf-8") as handle:
            handle.write("\n" + text)

    def test_accepts_distributed_template_without_optional_validation_workflow(self):
        # Arrange
        self.assertFalse((self.workflows / "validate.yml").exists())

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertEqual(self.update.read_text(encoding="utf-8"), self.template.replace("SOURCEFIELD_COMMIT", COMMIT))

    def test_rejects_missing_required_update_workflow_with_named_diagnostic(self):
        """Name the absent required source instead of exposing an incidental file-read failure."""
        # Arrange
        self.update.unlink()

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, r"consumer workflow file is missing: .*update-profile\.yml"):
            validate_workflows(self.root)

    def test_preserves_unrelated_workflow_read_errors(self):
        """Permission failures must not be presented as absent source files."""
        # Arrange
        failure = PermissionError("synthetic workflow permissions")

        # Act / Assert
        with patch.object(Path, "read_text", side_effect=failure):
            with self.assertRaisesRegex(PermissionError, "synthetic workflow permissions"):
                validate_workflows(self.root)

    def test_accepts_optional_validation_workflow_with_immutable_action(self):
        # Arrange
        validation = self.workflows / "validate.yml"
        validation.write_text(
            f"jobs:\n  check:\n    steps:\n      - uses: actions/checkout@{COMMIT}\n", encoding="utf-8",
        )

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertTrue(validation.is_file())

    def test_accepts_quoted_literal_references(self):
        # Arrange
        text = self.update.read_text(encoding="utf-8")
        text = text.replace(f"generate.yml@{COMMIT}", f"generate.yml@{COMMIT}\"")
        text = text.replace("uses: kdominic89", "uses: \"kdominic89")
        self.update.write_text(text, encoding="utf-8")
        self.append_step(f"      - uses: 'actions/checkout@{COMMIT}' # pinned fixture\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("'actions/checkout@", self.update.read_text(encoding="utf-8"))

    def test_ignores_reference_text_in_comments(self):
        # Arrange
        self.append_step("      # uses: actions/checkout@main\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("# uses:", self.update.read_text(encoding="utf-8"))

    def test_ignores_reference_text_in_run_block(self):
        # Arrange
        self.append_step("      - run: |\n          uses: actions/checkout@main\n          echo done\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("          uses:", self.update.read_text(encoding="utf-8"))

    def test_accepts_local_action(self):
        # Arrange
        self.append_step("      - uses: ./.github/actions/check\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("uses: ./", self.update.read_text(encoding="utf-8"))

    def test_accepts_digest_pinned_container_action(self):
        # Arrange
        self.append_step(f"      - uses: docker://alpine@sha256:{'b' * 64}\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("@sha256:", self.update.read_text(encoding="utf-8"))

    def test_accepts_pinned_pages_action_and_current_optional_token_forwarding(self):
        """Admit the current explicit optional secret without requiring Pages setup steps."""
        # Arrange
        self.append_step(f"      - uses: actions/configure-pages@{COMMIT}\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("    secrets:\n      PROFILE_TOKEN: ${{ secrets.PROFILE_TOKEN }}",
                      self.update.read_text(encoding="utf-8"))

    def test_accepts_workflow_without_optional_profile_token_forwarding(self):
        """Keep public-only consumers valid without forwarding a private aggregate credential."""
        # Arrange
        mapping = "    secrets:\n      PROFILE_TOKEN: ${{ secrets.PROFILE_TOKEN }}\n"
        source = self.update.read_text(encoding="utf-8").replace(mapping, "")
        self.update.write_text(source, encoding="utf-8")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertNotIn("secrets.PROFILE_TOKEN", self.update.read_text(encoding="utf-8"))

    def test_accepts_quoted_uses_mapping_key(self):
        # Arrange
        self.append_step(f"      - 'uses': 'actions/checkout@{COMMIT}'\n")

        # Act
        validate_workflows(self.root)

        # Assert
        self.assertIn("'uses':", self.update.read_text(encoding="utf-8"))

    def test_rejects_mutable_quoted_uses_mapping_key(self):
        # Arrange
        self.append_step("      - \"uses\": 'actions/checkout@main'\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable full commit"):
            validate_workflows(self.root)

    def test_rejects_alias_instead_of_literal_uses(self):
        # Arrange
        self.append_step("      - uses: *action_reference\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable full commit"):
            validate_workflows(self.root)

    def test_rejects_missing_reusable_generator(self):
        # Arrange
        text = self.update.read_text(encoding="utf-8")
        text = text.replace(f"    uses: kdominic89/sourcefield/.github/workflows/generate.yml@{COMMIT}\n", "")
        self.update.write_text(text, encoding="utf-8")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "exactly one.*Sourcefield"):
            validate_workflows(self.root)

    def test_rejects_duplicate_reusable_generator(self):
        # Arrange
        self.append_step(f"    uses: kdominic89/sourcefield/.github/workflows/generate.yml@{COMMIT}\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "exactly one.*Sourcefield"):
            validate_workflows(self.root)

    def test_rejects_mutable_reusable_generator(self):
        # Arrange
        text = self.update.read_text(encoding="utf-8").replace(f"generate.yml@{COMMIT}", "generate.yml@main")
        self.update.write_text(text, encoding="utf-8")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable full commit"):
            validate_workflows(self.root)

    def test_rejects_mutable_action_in_update_workflow(self):
        # Arrange
        self.append_step("      - uses: actions/checkout@v7\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable full commit"):
            validate_workflows(self.root)

    def test_rejects_mutable_action_in_optional_validation_workflow(self):
        # Arrange
        validation = self.workflows / "validate.yml"
        validation.write_text("jobs:\n  check:\n    steps:\n      - uses: actions/checkout@main\n", encoding="utf-8")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable full commit"):
            validate_workflows(self.root)

    def test_rejects_mutable_reference_after_run_block(self):
        # Arrange
        self.append_step("      - run: |\n          echo done\n      - uses: actions/checkout@main\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable full commit"):
            validate_workflows(self.root)

    def test_rejects_mutable_container_action(self):
        # Arrange
        self.append_step("      - uses: docker://alpine:latest\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "immutable digest"):
            validate_workflows(self.root)

    def test_rejects_empty_uses_value(self):
        # Arrange
        self.append_step("      - uses: # missing literal reference\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "literal uses"):
            validate_workflows(self.root)

    def test_rejects_multiline_uses_value(self):
        # Arrange
        self.append_step(f"      - uses: >\n          actions/checkout@{COMMIT}\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "literal uses"):
            validate_workflows(self.root)

    def test_rejects_generating_cargo_lock_in_update_workflow(self):
        # Arrange
        self.append_step("      - run: cargo generate-lockfile\n")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "committed Cargo.lock"):
            validate_workflows(self.root)

    def test_rejects_generating_cargo_lock_in_optional_validation_workflow(self):
        # Arrange
        validation = self.workflows / "validate.yml"
        validation.write_text("jobs:\n  check:\n    steps:\n      - run: cargo generate-lockfile\n", encoding="utf-8")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "committed Cargo.lock"):
            validate_workflows(self.root)


if __name__ == "__main__":
    unittest.main()
