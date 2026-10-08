"""Preserve explicit private aggregate selection across caller, workflow and native argv."""

import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
import consumer_candidate


class PrivateCountCandidateTests(unittest.TestCase):
    """Require explicit caller intent and credentials before adding the native private flag."""

    def prepare_candidate(
        self, selected: bool | None, token: str, environment_intent: str = "false"
    ) -> tuple[list[list[str]], str, list[str | None]]:
        """Observe native command boundaries using only isolated fixtures and synthetic credentials."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            installation = root / "installation"
            installation.mkdir()
            executable = installation / "sourcefield"
            executable.touch()
            destination = root / "candidate"
            commands = []
            inherited_intents = []
            diagnostics = io.StringIO()

            def clone(source_root: Path, candidate_root: Path) -> None:
                """Create the independent fixture clone without invoking Git."""
                candidate_root.mkdir()

            def execute(command, **kwargs):
                """Admit only the tracked inventory and native generation/validation commands."""
                if command == ["git", "ls-files", "-z"]:
                    return subprocess.CompletedProcess(command, 0, stdout=b"")

                if command[0] == str(executable) and command[1] in ("generate", "validate"):
                    commands.append(command)
                    inherited_intents.append(kwargs.get("env", os.environ).get("SOURCEFIELD_PRIVATE_COUNTS"))
                    return subprocess.CompletedProcess(command, 0)

                raise AssertionError("unmodeled candidate command")

            options = {} if selected is None else {"private_counts": selected}
            environment = {"PROFILE_TOKEN": token, "SOURCEFIELD_PRIVATE_COUNTS": environment_intent}

            with patch.dict(os.environ, environment), patch.object(
                consumer_candidate, "isolated_checkout", side_effect=clone
            ), patch.object(
                consumer_candidate.subprocess, "run", side_effect=execute
            ), contextlib.redirect_stderr(diagnostics), contextlib.redirect_stdout(diagnostics):
                consumer_candidate.candidate(
                    source, destination, installation, "config/profile.toml", [], False, False, **options
                )

            return commands, diagnostics.getvalue(), inherited_intents

    def test_selected_count_with_token_reaches_the_native_cli(self):
        """Forward one explicit private flag without including credentials in argv."""
        # Arrange
        selected, token = True, "synthetic-private-credential"

        # Act
        commands, diagnostic, _ = self.prepare_candidate(selected, token)

        # Assert
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[0].count("--private-counts"), 1)
        self.assertIn("--strict-live", commands[0])
        self.assertNotIn("--private-counts", commands[1])
        self.assertNotIn(token, repr(commands) + diagnostic)
        self.assertEqual(diagnostic, "")

    def test_selected_count_without_token_warns_and_continues_public_generation(self):
        """Preserve the old caller's missing-token behavior without a strict private request."""
        # Arrange
        selected, token = True, ""

        # Act
        commands, diagnostic, _ = self.prepare_candidate(selected, token)

        # Assert
        self.assertEqual(len(commands), 2)
        self.assertNotIn("--private-counts", commands[0])
        self.assertIn("--strict-live", commands[0])
        self.assertIn("::warning::Private aggregate requested", diagnostic)
        self.assertIn("Continuing without it.", diagnostic)

    def test_unselected_count_with_token_remains_public(self):
        """Credentials alone never add private collection intent."""
        # Arrange
        selected, token = False, "synthetic-private-credential"

        # Act
        commands, diagnostic, _ = self.prepare_candidate(selected, token)

        # Assert
        self.assertNotIn("--private-counts", commands[0])
        self.assertNotIn(token, repr(commands) + diagnostic)
        self.assertEqual(diagnostic, "")

    def test_unselected_count_without_token_remains_public(self):
        """Keep the default public path free from private-token warnings."""
        # Arrange
        selected, token = False, ""

        # Act
        commands, diagnostic, _ = self.prepare_candidate(selected, token)

        # Assert
        self.assertNotIn("--private-counts", commands[0])
        self.assertEqual(diagnostic, "")

    def test_whitespace_token_is_treated_as_missing(self):
        """Reject empty credential spellings before constructing private argv."""
        # Arrange
        selected, token = True, " \t\n"

        # Act
        commands, diagnostic, _ = self.prepare_candidate(selected, token)

        # Assert
        self.assertNotIn("--private-counts", commands[0])
        self.assertIn("PROFILE_TOKEN is not configured", diagnostic)

    def test_existing_callers_default_to_no_additional_private_intent(self):
        """Keep the preexisting seven-argument candidate API compatible."""
        # Arrange
        selected, token = None, "synthetic-private-credential"

        # Act
        commands, diagnostic, _ = self.prepare_candidate(selected, token)

        # Assert
        self.assertNotIn("--private-counts", commands[0])
        self.assertEqual(diagnostic, "")

    def test_explicit_native_environment_intent_is_not_overwritten(self):
        """Keep the candidate's new caller selection independent of native environment opt-in."""
        # Arrange
        selected, token, environment_intent = False, "synthetic-private-credential", "true"

        # Act
        commands, diagnostic, intents = self.prepare_candidate(selected, token, environment_intent)

        # Assert
        self.assertEqual(intents, ["true", "true"])
        self.assertNotIn("--private-counts", commands[0])
        self.assertEqual(diagnostic, "")

    def test_cli_private_flag_reaches_the_candidate_api(self):
        """Expose the caller's explicit intent through the public candidate command."""
        # Arrange
        arguments = ["consumer_candidate.py", "--source", "/source", "--destination", "/candidate",
                     "--installation", "/installation", "--private-counts"]

        # Act
        with patch.object(sys, "argv", arguments), patch.object(consumer_candidate, "candidate") as generate:
            status = consumer_candidate.main()

        # Assert
        self.assertEqual(status, 0)
        self.assertTrue(generate.call_args.args[-1])

    def test_cli_defaults_to_no_additional_private_intent(self):
        """Avoid changing existing command-line callers that omit the new option."""
        # Arrange
        arguments = ["consumer_candidate.py", "--source", "/source", "--destination", "/candidate",
                     "--installation", "/installation"]

        # Act
        with patch.object(sys, "argv", arguments), patch.object(consumer_candidate, "candidate") as generate:
            status = consumer_candidate.main()

        # Assert
        self.assertEqual(status, 0)
        self.assertFalse(generate.call_args.args[-1])


class PrivateCountWorkflowContractTests(unittest.TestCase):
    """Check the typed input and explicitly scoped caller secret at their actual declarations."""

    def test_reusable_workflow_declares_and_consumes_a_default_false_boolean(self):
        """Forward additional intent as a typed input, never as an inferred secret-presence flag."""
        # Arrange
        source = (ROOT / ".github/workflows/generate.yml").read_text(encoding="ascii")

        # Act
        input_block = source.split("      include_private_count:\n", 1)[1].split("    secrets:\n", 1)[0]

        # Assert
        self.assertIn("        type: boolean\n", input_block)
        self.assertIn("        default: false\n", input_block)
        self.assertIn("REQUEST_PRIVATE_COUNTS: ${{ inputs.include_private_count }}", source)
        self.assertIn("PROFILE_TOKEN: ${{ secrets.PROFILE_TOKEN }}", source)
        self.assertNotIn("SOURCEFIELD_PRIVATE_COUNTS:", source)

    def test_consumer_template_retains_manual_and_variable_selection_and_explicit_secret(self):
        """Preserve the old public default and both existing caller opt-in routes."""
        # Arrange
        source = (ROOT / "docs/consumer-workflow.yml.template").read_text(encoding="ascii")

        # Act
        dispatch = source.split("  workflow_dispatch:\n", 1)[1].split("  schedule:\n", 1)[0]
        generate = source.split("  generate:\n", 1)[1].split("  publish:\n", 1)[0]

        # Assert
        self.assertIn("      include_private_count:\n", dispatch)
        self.assertIn("        type: boolean\n", dispatch)
        self.assertIn("        default: false\n", dispatch)
        self.assertIn("include_private_count: ${{ (github.event_name == 'workflow_dispatch' && "
                      "inputs.include_private_count) || vars.SOURCEFIELD_PRIVATE_COUNTS == 'true' }}", generate)
        self.assertIn("    secrets:\n      PROFILE_TOKEN: ${{ secrets.PROFILE_TOKEN }}", generate)
        self.assertNotIn("secrets: inherit", source)


@unittest.skipUnless(os.name == "posix", "The reusable workflow generation step requires a POSIX shell.")
class PrivateCountWorkflowExecutionTests(unittest.TestCase):
    """Execute the actual reusable Bash block with synthetic selected inputs and no network."""

    def invoke_workflow_step(
        self, selected: bool, offline: bool = False, locked: bool = False
    ) -> tuple[list[str], str]:
        """Observe only candidate argv from the real shell block instead of reproducing its branching."""
        source = (ROOT / ".github/workflows/generate.yml").read_text(encoding="ascii")
        step = source.split("      - name: Generate and validate in a separate candidate tree\n", 1)[1]
        block = step.split("        run: |\n", 1)[1].split("      - uses:", 1)[0]
        script = "\n".join(line[10:] for line in block.splitlines())

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            helper = root / "generator/scripts/consumer_candidate.py"
            helper.parent.mkdir(parents=True)
            helper.write_text(
                'import json, sys\nprint(json.dumps(sys.argv[1:]))\n', encoding="ascii"
            )
            environment = dict(os.environ, REQUEST_PRIVATE_COUNTS=str(selected).lower(),
                               OFFLINE=str(offline).lower(), LOCKED=str(locked).lower(),
                               CONSUMER_CONFIG="config/profile.toml", CONSUMER_READMES='["README.md"]',
                               RUNNER_TEMP=str(root), PROFILE_TOKEN="synthetic-private-credential")

            result = subprocess.run(["bash", "-e", "-c", script], cwd=root, env=environment,
                                    check=True, capture_output=True, text=True)

            return json.loads(result.stdout), result.stderr

    def test_selected_workflow_input_reaches_candidate_argv(self):
        """Pass caller intent once without copying the private credential into public argv."""
        # Arrange
        selected = True

        # Act
        arguments, diagnostic = self.invoke_workflow_step(selected)

        # Assert
        self.assertEqual(arguments.count("--private-counts"), 1)
        self.assertNotIn("synthetic-private-credential", repr(arguments) + diagnostic)
        self.assertEqual(diagnostic, "")

    def test_unselected_workflow_input_does_not_follow_token_presence(self):
        """Ignore credential presence when the additional caller intent is false."""
        # Arrange
        selected = False

        # Act
        arguments, diagnostic = self.invoke_workflow_step(selected)

        # Assert
        self.assertNotIn("--private-counts", arguments)
        self.assertEqual(diagnostic, "")

    def test_private_selection_does_not_replace_offline_mode(self):
        """Keep independent execution-mode options when forwarding aggregate intent."""
        # Arrange
        selected, offline = True, True

        # Act
        arguments, _ = self.invoke_workflow_step(selected, offline=offline)

        # Assert
        self.assertIn("--offline", arguments)
        self.assertIn("--private-counts", arguments)
        self.assertNotIn("--locked", arguments)

    def test_private_selection_does_not_replace_locked_replay(self):
        """Retain locked replay while passing the optional aggregate-selection flag."""
        # Arrange
        selected, offline, locked = True, True, True

        # Act
        arguments, _ = self.invoke_workflow_step(selected, offline=offline, locked=locked)

        # Assert
        self.assertIn("--offline", arguments)
        self.assertIn("--locked", arguments)
        self.assertIn("--private-counts", arguments)


if __name__ == "__main__":
    unittest.main()
