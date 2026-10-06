"""Verify exact browser policy boundaries and the workflow's privileged handoff."""

import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import SCRIPTS
import browser_sandbox_policy


def executable_file(root: Path, name: str = "chrome") -> Path:
    """Create a real executable fixture under a canonical temporary directory."""

    executable = root / name
    executable.write_text("#!/bin/sh\nexit 0\n", encoding="ascii")
    executable.chmod(0o755)

    return executable


def workflow_run(name: str) -> str:
    """Extract the literal run block of one named validation-workflow step."""

    workflow = (SCRIPTS.parent / ".github/workflows/validate.yml").read_text(encoding="ascii")
    step = workflow.split(f"      - name: {name}\n", 1)[1].split("\n      - ", 1)[0]

    return textwrap.dedent(step.split("        run: |\n", 1)[1])


class BrowserSandboxPolicyTests(unittest.TestCase):
    """Render one literal executable path and reject ambiguous or unsafe inputs."""

    def test_exact_executable_with_spaces_is_quoted(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = executable_file(Path(temporary).resolve(), "chrome with spaces")
            expected = (
                'abi <abi/4.0>,\ninclude <tunables/global>\n\n'
                f'profile sourcefield-playwright "{executable}" flags=(unconfined) {{\n'
                '  userns,\n}\n'
            )

            policy = browser_sandbox_policy.render_policy(str(executable))

            self.assertEqual(policy, expected)

    def test_relative_path_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = executable_file(Path(temporary).resolve())
            relative = executable.name

            with self.assertRaisesRegex(ValueError, "must be absolute"):
                browser_sandbox_policy.render_policy(relative)

    def assert_noncanonical_alias_rejected(self, template: str) -> None:
        """Probe one alternate spelling of a real executable under a canonical temporary root."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root)
            (root / "nested").mkdir()
            alias = template.format(root=root, executable=executable)

            # Act / Assert
            with self.assertRaisesRegex(ValueError, "must be canonical"):
                browser_sandbox_policy.render_policy(alias)

    def test_rejects_noncanonical_alias_parent_traversal(self):
        """Reject a parent-traversal spelling of the real executable."""
        self.assert_noncanonical_alias_rejected('{root}/nested/../chrome')

    def test_rejects_noncanonical_alias_current_directory_segment(self):
        """Reject an explicit current-directory segment in the executable path."""
        self.assert_noncanonical_alias_rejected('{root}/./chrome')

    def test_rejects_noncanonical_alias_duplicate_leading_separator(self):
        """Reject an additional leading separator in the executable path."""
        self.assert_noncanonical_alias_rejected('/{executable}')

    def test_symlink_to_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root)
            alias = root / "chrome-link"
            alias.symlink_to(executable)

            with self.assertRaisesRegex(ValueError, "must not be a symlink"):
                browser_sandbox_policy.render_policy(str(alias))

    def test_parent_directory_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            directory = root / "browser"
            directory.mkdir()
            executable_file(directory)
            alias = root / "browser-link"
            alias.symlink_to(directory, target_is_directory=True)

            with self.assertRaisesRegex(ValueError, "must be canonical"):
                browser_sandbox_policy.render_policy(str(alias / "chrome"))

    def test_directory_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary).resolve()

            with self.assertRaisesRegex(ValueError, "must be a regular file"):
                browser_sandbox_policy.render_policy(str(directory))

    def test_nonexecutable_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = executable_file(Path(temporary).resolve())
            executable.chmod(0o644)

            with self.assertRaisesRegex(ValueError, "must be executable"):
                browser_sandbox_policy.render_policy(str(executable))

    def test_missing_file_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            missing = Path(temporary).resolve() / "missing-browser"

            with self.assertRaisesRegex(ValueError, "cannot inspect browser executable"):
                browser_sandbox_policy.render_policy(str(missing))

    def assert_unsafe_filename_rejected(self, character: str) -> None:
        """Create one real filename containing a disallowed policy character before rendering it."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root, f"chrome{character}")

            # Act / Assert
            with self.assertRaisesRegex(ValueError, "unsafe policy characters"):
                browser_sandbox_policy.render_policy(str(executable))

    def test_rejects_policy_metacharacter_asterisk(self):
        """Reject an asterisk in a real executable filename."""
        self.assert_unsafe_filename_rejected('*')

    def test_rejects_policy_metacharacter_question_mark(self):
        """Reject a question mark in a real executable filename."""
        self.assert_unsafe_filename_rejected('?')

    def test_rejects_policy_metacharacter_opening_bracket(self):
        """Reject an opening bracket in a real executable filename."""
        self.assert_unsafe_filename_rejected('[')

    def test_rejects_policy_metacharacter_closing_bracket(self):
        """Reject a closing bracket in a real executable filename."""
        self.assert_unsafe_filename_rejected(']')

    def test_rejects_policy_metacharacter_opening_brace(self):
        """Reject an opening brace in a real executable filename."""
        self.assert_unsafe_filename_rejected('{')

    def test_rejects_policy_metacharacter_closing_brace(self):
        """Reject a closing brace in a real executable filename."""
        self.assert_unsafe_filename_rejected('}')

    def test_rejects_policy_metacharacter_at_sign(self):
        """Reject an at sign in a real executable filename."""
        self.assert_unsafe_filename_rejected('@')

    def test_rejects_policy_metacharacter_dollar_sign(self):
        """Reject a dollar sign in a real executable filename."""
        self.assert_unsafe_filename_rejected('$')

    def test_rejects_policy_metacharacter_double_quote(self):
        """Reject a double quote in a real executable filename."""
        self.assert_unsafe_filename_rejected('"')

    def test_rejects_policy_metacharacter_single_quote(self):
        """Reject a single quote in a real executable filename."""
        self.assert_unsafe_filename_rejected("'")

    def test_rejects_policy_metacharacter_backslash(self):
        """Reject a backslash in a real executable filename."""
        self.assert_unsafe_filename_rejected('\\')

    def test_rejects_policy_character_newline(self):
        """Reject a newline in a real executable filename."""
        self.assert_unsafe_filename_rejected('\n')

    def test_rejects_policy_character_carriage_return(self):
        """Reject a carriage return in a real executable filename."""
        self.assert_unsafe_filename_rejected('\r')

    def test_rejects_policy_character_tab(self):
        """Reject a tab in a real executable filename."""
        self.assert_unsafe_filename_rejected('\t')

    def test_rejects_policy_character_start_of_heading(self):
        """Reject a control byte in a real executable filename."""
        self.assert_unsafe_filename_rejected('\x01')

    def test_rejects_policy_character_delete(self):
        """Reject the delete character in a real executable filename."""
        self.assert_unsafe_filename_rejected('\x7f')

    def test_rejects_policy_character_non_ascii(self):
        """Reject a non-ASCII character in a real executable filename."""
        self.assert_unsafe_filename_rejected('\xe9')

    def test_null_byte_is_rejected_before_filesystem_access(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = executable_file(Path(temporary).resolve())
            value = str(executable) + "\x00"

            with self.assertRaisesRegex(ValueError, "unsafe policy characters"):
                browser_sandbox_policy.render_policy(value)

    def test_cli_emits_only_the_policy_on_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            executable = executable_file(Path(temporary).resolve(), "chrome with spaces")
            expected = browser_sandbox_policy.render_policy(str(executable))

            result = subprocess.run(
                [sys.executable, str(SCRIPTS / "browser_sandbox_policy.py"), str(executable)],
                capture_output=True,
                text=True,
            )

        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, expected, ""))

    def test_cli_failure_is_concise_without_policy_or_traceback(self):
        with tempfile.TemporaryDirectory() as temporary:
            missing = Path(temporary).resolve() / "missing-browser"

            result = subprocess.run(
                [sys.executable, str(SCRIPTS / "browser_sandbox_policy.py"), str(missing)],
                capture_output=True,
                text=True,
            )

        self.assertEqual((result.returncode, result.stdout), (1, ""))
        self.assertIn("cannot inspect browser executable", result.stderr)
        self.assertEqual(len(result.stderr.splitlines()), 1)
        self.assertNotIn("Traceback", result.stderr)


class BrowserSandboxWorkflowTests(unittest.TestCase):
    """Execute the real workflow run block with only external boundaries replaced."""

    def workflow_environment(self, root: Path, executable: Path) -> dict[str, str]:
        """Arrange logging stand-ins for Node path resolution and privileged commands."""

        tools = root / "tools"
        tools.mkdir()
        node = tools / "node"
        node.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "from pathlib import Path\n"
            "root = Path(os.environ['RUNNER_TEMP'])\n"
            "(root / 'node-args.json').write_text(json.dumps(sys.argv[1:]))\n"
            "(root / 'node-input.js').write_text(sys.stdin.read())\n"
            "sys.stdout.write(os.environ['TEST_BROWSER'])\n",
            encoding="ascii",
        )
        node.chmod(0o755)
        sudo = tools / "sudo"
        sudo.write_text(
            f"#!{sys.executable}\n"
            "import json, os, shutil, sys\n"
            "from pathlib import Path\n"
            "root = Path(os.environ['RUNNER_TEMP'])\n"
            "with (root / 'privileged.jsonl').open('a') as output:\n"
            "    output.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            "if sys.argv[1] == 'install':\n"
            "    shutil.copyfile(sys.argv[-2], root / 'installed-policy')\n"
            "elif sys.argv[1] == 'apparmor_parser':\n"
            "    raise SystemExit(int(os.environ.get('TEST_POLICY_FAILURE', '0')))\n"
            "else:\n"
            "    raise SystemExit('Unexpected privileged command')\n",
            encoding="ascii",
        )
        sudo.chmod(0o755)

        return dict(
            os.environ,
            PATH=str(tools) + os.pathsep + os.environ["PATH"],
            RUNNER_TEMP=str(root),
            GITHUB_ENV=str(root / "github-env"),
            TEST_BROWSER=str(executable),
            PYTHONDONTWRITEBYTECODE="1",
        )

    def test_browser_provisioning_consumes_the_authored_lock_and_full_chromium(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root)
            environment = self.workflow_environment(root, executable)
            npm = root / "tools/npm"
            npm.write_text(
                f"#!{sys.executable}\n"
                "import json, os, sys\n"
                "from pathlib import Path\n"
                "root = Path(os.environ['RUNNER_TEMP'])\n"
                "(root / 'npm-args.json').write_text(json.dumps(sys.argv[1:]))\n",
                encoding="ascii",
            )
            npm.chmod(0o755)
            block = workflow_run("Provision the pinned existing browser test tool")

            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", block],
                cwd=SCRIPTS.parent, env=environment, capture_output=True, text=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads((root / "npm-args.json").read_text()),
                             ["ci", "--prefix", str(root / "browser-tools"),
                              "--include=dev", "--ignore-scripts", "--no-audit", "--no-fund"])
            for name in ("package.json", "package-lock.json"):
                self.assertEqual((root / "browser-tools" / name).read_bytes(),
                                 (SCRIPTS.parent / "tools/browser" / name).read_bytes())
            self.assertEqual(json.loads((root / "node-args.json").read_text()),
                             [str(root / "browser-tools/node_modules/playwright/cli.js"),
                              "install", "--with-deps", "--no-shell", "chromium"])

    def test_browser_lock_install_failure_prevents_chromium_installation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root)
            environment = self.workflow_environment(root, executable)
            npm = root / "tools/npm"
            npm.write_text("#!/bin/sh\nexit 7\n", encoding="ascii")
            npm.chmod(0o755)
            block = workflow_run("Provision the pinned existing browser test tool")

            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", block],
                cwd=SCRIPTS.parent, env=environment, capture_output=True, text=True,
            )

            self.assertEqual(result.returncode, 7)
            self.assertFalse((root / "node-args.json").exists())

    def test_workflow_installs_policy_and_publishes_the_same_browser(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root, "chrome with spaces")
            environment = self.workflow_environment(root, executable)
            block = workflow_run("Allow the bundled browser sandbox for its exact executable")
            destination = "/etc/apparmor.d/sourcefield-playwright"
            expected = [
                ["install", "-o", "root", "-g", "root", "-m", "0644",
                 str(root / "sourcefield-playwright.apparmor"), destination],
                ["apparmor_parser", "--replace", destination],
            ]

            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", block],
                cwd=SCRIPTS.parent,
                env=environment,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            actions = [json.loads(line) for line in (root / "privileged.jsonl").read_text().splitlines()]
            self.assertEqual(actions, expected)
            self.assertEqual(
                (root / "installed-policy").read_text(), browser_sandbox_policy.render_policy(str(executable))
            )
            self.assertEqual((root / "github-env").read_text(), f"SOURCEFIELD_BROWSER={executable}\n")
            self.assertEqual(
                json.loads((root / "node-args.json").read_text()),
                ["--input-type=module", "-", str(root / "browser-tools/node_modules/playwright/index.mjs")],
            )
            self.assertIn("chromium.executablePath()", (root / "node-input.js").read_text())

    def test_workflow_rejects_unsafe_path_before_privileged_commands(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root, "chrome*")
            environment = self.workflow_environment(root, executable)
            block = workflow_run("Allow the bundled browser sandbox for its exact executable")

            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", block],
                cwd=SCRIPTS.parent,
                env=environment,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 1)
            self.assertIn("unsafe policy characters", result.stderr)
            self.assertFalse((root / "privileged.jsonl").exists())
            self.assertFalse((root / "github-env").exists())

    def test_workflow_parser_failure_prevents_browser_export(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root, "chrome with spaces")
            environment = self.workflow_environment(root, executable)
            environment["TEST_POLICY_FAILURE"] = "7"
            block = workflow_run("Allow the bundled browser sandbox for its exact executable")

            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", block],
                cwd=SCRIPTS.parent,
                env=environment,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 7)
            actions = [json.loads(line) for line in (root / "privileged.jsonl").read_text().splitlines()]
            self.assertEqual(actions[-1], ["apparmor_parser", "--replace", "/etc/apparmor.d/sourcefield-playwright"])
            self.assertFalse((root / "github-env").exists())

    def test_workflow_fixture_gate_receives_the_exact_browser_and_module(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            executable = executable_file(root, "chrome with spaces")
            environment = self.workflow_environment(root, executable)
            environment["SOURCEFIELD_BROWSER"] = str(executable)
            for name in ("cargo", "python3"):
                command = root / "tools" / name
                command.write_text(
                    f"#!{sys.executable}\n"
                    "import json, os, sys\n"
                    "from pathlib import Path\n"
                    "root = Path(os.environ['RUNNER_TEMP'])\n"
                    "with (root / 'fixture-gate.jsonl').open('a') as output:\n"
                    "    record = {'tool': Path(sys.argv[0]).name, 'args': sys.argv[1:]}\n"
                    "    output.write(json.dumps(record) + '\\n')\n",
                    encoding="ascii",
                )
                command.chmod(0o755)
            block = workflow_run("Verify actual WASM/browser behavior for both variants and multiple organizations")
            expected = [
                {"tool": "cargo", "args": ["build", "--locked", "-p", "sourcefield-cli"]},
                {"tool": "python3", "args": ["scripts/verify_publication.py", "--binary", "target/debug/sourcefield"]},
                {
                    "tool": "python3",
                    "args": [
                        "scripts/release_browser_fixtures.py",
                        "--binary", "target/debug/sourcefield",
                        "--output", str(root / "browser-fixtures"),
                        "--playwright-module", str(root / "browser-tools/node_modules/playwright/index.mjs"),
                        "--browser", str(executable),
                    ],
                },
            ]

            result = subprocess.run(
                ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", block],
                cwd=SCRIPTS.parent,
                env=environment,
                capture_output=True,
                text=True,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            actions = [json.loads(line) for line in (root / "fixture-gate.jsonl").read_text().splitlines()]
            self.assertEqual(actions, expected)


if __name__ == "__main__":
    unittest.main()
