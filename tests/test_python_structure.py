"""Verify tooling ownership, source distribution and nested syntax admission."""

import ast
import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
import zipfile
from pathlib import Path

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
import package_source


class PythonStructureTests(unittest.TestCase):
    """Exercise the shared package through actual archives and command entrypoints."""

    def test_production_python_does_not_import_test_modules(self):
        """Keep workflow-facing checks independent of test discovery and test-module names."""
        # Arrange
        sources = sorted((ROOT / "scripts").rglob("*.py"))

        # Act
        imports = []
        for path in sources:
            for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
                if isinstance(node, ast.ImportFrom) and node.module:
                    names = [node.module]
                elif isinstance(node, ast.Import):
                    names = [alias.name for alias in node.names]
                else:
                    continue

                imports.extend(
                    (path.relative_to(ROOT).as_posix(), node.lineno, name)
                    for name in names
                    if name.split(".")[0] == "tests" or name.split(".")[0].startswith("test_")
                )

        # Assert
        self.assertEqual(imports, [])

    def test_source_archive_runs_release_command_from_unrelated_directory(self):
        """Distribute shared modules and resolve source-root version data without cwd assumptions."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "source.zip"
            version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"][
                "version"
            ]

            package_source.package(ROOT, archive)
            with zipfile.ZipFile(archive) as bundle:
                names = set(bundle.namelist())
                bundle.extractall(root / "expanded")

            checkout = root / "expanded/sourcefield"
            environment = {key: value for key, value in os.environ.items() if key != "PYTHONPATH"}
            environment["PYTHONDONTWRITEBYTECODE"] = "1"

            # Act
            result = subprocess.run(
                [sys.executable, "-B", str(checkout / "scripts/release_manifest.py"),
                 "--release", f"v{version}", "--check-version"],
                cwd=root, env=environment, capture_output=True, text=True, check=False, timeout=15,
            )

            # Assert
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), f"v{version}")
            for name in ("__init__", "artifacts", "release", "consumer", "presentation", "workflow_policy"):
                self.assertIn(f"sourcefield/scripts/sourcefield_tools/{name}.py", names)

            self.assertIn("sourcefield/tests/support/__init__.py", names)
            self.assertFalse(any("__pycache__" in name or name.endswith(".pyc") for name in names))


@unittest.skipUnless(os.name == "posix", "This fixture exercises the Bash syntax gate on a POSIX host.")
class NestedPythonSyntaxTests(unittest.TestCase):
    """Nested Python syntax must be admitted before unrelated native verification begins."""

    def syntax_fixture(self, root: Path, nested_source: str) -> dict[str, str]:
        """Use the real shell gate with a minimal test scaffold and stubs for unrelated native tools."""
        scripts = root / "scripts"
        scripts.mkdir()
        (root / "tests").mkdir()
        # Current unittest exits on an empty suite, so a real fixture assertion lets the syntax gate run.
        (root / "tests/test_scaffold.py").write_text(
            'import unittest\nfrom pathlib import Path\n'
            'class ScaffoldTests(unittest.TestCase):\n'
            '    def test_nested_probe_exists(self):\n'
            '        self.assertTrue(Path("scripts/nested/probe.py").is_file())\n',
            encoding="utf-8",
        )

        (root / "Cargo.lock").write_text("synthetic lock\n", encoding="utf-8")
        (scripts / "verify.sh").write_bytes((ROOT / "scripts/verify.sh").read_bytes())
        nested = scripts / "nested"
        nested.mkdir()
        (nested / "probe.py").write_text(nested_source, encoding="utf-8")
        tools = root / "bin"
        tools.mkdir()
        (tools / "python3").symlink_to(sys.executable)
        for name in ("node", "cargo", "rustc"):
            (tools / name).symlink_to("/usr/bin/true")

        return {**os.environ, "PATH": f"{tools}:/usr/bin:/bin", "PYTHONDONTWRITEBYTECODE": "1"}

    def test_invalid_nested_python_fails_verification(self):
        """A malformed module inside a tooling package cannot bypass top-level verification."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            environment = self.syntax_fixture(root, "def broken(:\n")

            # Act
            result = subprocess.run(
                ["/bin/bash", str(root / "scripts/verify.sh")],
                cwd=root, env=environment, capture_output=True, text=True, check=False, timeout=15,
            )

            # Assert
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("SyntaxError", result.stderr)
            self.assertIn("nested/probe.py", result.stderr)

    def test_valid_nested_python_is_parsed_without_execution(self):
        """Syntax admission does not execute a nested module's top-level code."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            environment = self.syntax_fixture(root, "raise RuntimeError('must not execute')\n")

            # Act
            result = subprocess.run(
                ["/bin/bash", str(root / "scripts/verify.sh")],
                cwd=root, env=environment, capture_output=True, text=True, check=False, timeout=15,
            )

            # Assert
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("RuntimeError", result.stderr)


if __name__ == "__main__":
    unittest.main()
