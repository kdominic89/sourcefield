"""Verify verification gates and authoritative toolchain dependency consumers."""

import json
import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
import re

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support


class VerificationGateTests(unittest.TestCase):
    """Missing tools or a missing lockfile must fail rather than silently skip checks."""

    def test_missing_tool_is_a_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            original = Path(__file__).resolve().parents[1] / "scripts/verify.sh"
            (root / "scripts/verify.sh").write_text(original.read_text())
            tools = root / "bin"
            tools.mkdir()
            # dirname is needed to locate the script, before tool checks begin.
            (tools / "dirname").symlink_to("/usr/bin/dirname")

            result = subprocess.run(
                ["/bin/bash", str(root / "scripts/verify.sh")],
                env={**os.environ, "PATH": str(tools)},
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "Required verification tool is missing: python3", result.stderr
            )

    def test_missing_lockfile_is_a_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            original = Path(__file__).resolve().parents[1] / "scripts/verify.sh"
            (root / "scripts/verify.sh").write_text(original.read_text())
            tools = root / "bin"
            tools.mkdir()
            (tools / "dirname").symlink_to("/usr/bin/dirname")
            for name in ("python3", "node", "cargo", "rustc"):
                (tools / name).symlink_to("/usr/bin/true")

            result = subprocess.run(
                ["/bin/bash", str(root / "scripts/verify.sh")],
                env={**os.environ, "PATH": str(tools)},
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("The committed Cargo.lock is required", result.stderr)
            self.assertFalse((root / "Cargo.lock").exists())


class WasmFeatureContractTests(unittest.TestCase):
    """Keep the optimizer's accepted features aligned with compiler output."""

    def test_release_optimizer_accepts_compiler_features(self):
        root = Path(__file__).resolve().parents[1]
        compiler = tomllib.loads((root / ".cargo/config.toml").read_text())
        manifest = tomllib.loads(
            (root / "crates/sourcefield-wasm/Cargo.toml").read_text()
        )

        flags = compiler["target"]["wasm32-unknown-unknown"]["rustflags"]
        release = (
            manifest["package"]
            .get("metadata", {})
            .get("wasm-pack", {})
            .get("profile", {})
            .get("release", {})
        )

        mapping = {
            "bulk-memory": "bulk-memory",
            "multivalue": "multivalue",
            "mutable-globals": "mutable-globals",
            "nontrapping-fptoint": "nontrapping-float-to-int",
            "reference-types": "reference-types",
            "sign-ext": "sign-ext",
        }

        result = subprocess.run(
            ["rustc", "--print", "cfg", "--target", "wasm32-unknown-unknown", *flags],
            check=True,
            capture_output=True,
            text=True,
            cwd=root,
        )

        features = re.findall(r'target_feature="([^"]+)"', result.stdout)
        optimizer = release.get("wasm-opt", ["-O"])

        self.assertIsInstance(
            optimizer, list, "Release optimization must remain enabled"
        )
        self.assertTrue(any(option.startswith("-O") for option in optimizer))
        for feature in features:
            self.assertIn(
                feature,
                mapping,
                "Review new compiler features against optimizer capabilities",
            )
            self.assertIn(f"--enable-{mapping[feature]}", optimizer)


class ToolAuthorityTests(unittest.TestCase):
    """Verify exact authored dependencies and every local/CI pin consumer."""

    def test_browser_manifest_and_lock_agree_on_exact_dependency_closure(self):
        root = Path(__file__).resolve().parents[1]
        manifest = json.loads((root / "tools/browser/package.json").read_text(encoding="ascii"))
        lock = json.loads((root / "tools/browser/package-lock.json").read_text(encoding="ascii"))

        packages = lock["packages"]

        self.assertEqual(set(manifest["devDependencies"]), {"playwright"})
        self.assertRegex(manifest["devDependencies"]["playwright"],
                         r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
        self.assertEqual(packages[""]["devDependencies"], manifest["devDependencies"])
        self.assertEqual(set(packages), {"", "node_modules/playwright", "node_modules/playwright-core"})
        self.assertEqual(lock["lockfileVersion"], 3)
        self.assertEqual(packages["node_modules/playwright"]["dependencies"],
                         {"playwright-core": manifest["devDependencies"]["playwright"]})
        for name in ("playwright", "playwright-core"):
            entry = packages[f"node_modules/{name}"]
            self.assertEqual(entry["version"], manifest["devDependencies"]["playwright"])
            self.assertEqual(entry["resolved"], f"https://registry.npmjs.org/{name}/-/{name}-{entry['version']}.tgz")
            self.assertRegex(entry["integrity"], r"^sha512-[A-Za-z0-9+/]{86}==$" )

    def test_packager_install_workflows_consume_authoritative_pin(self):
        root = Path(__file__).resolve().parents[1]
        paths = [root / ".github/workflows/validate.yml", root / ".github/workflows/release.yml"]

        installs = [path.read_text(encoding="ascii") for path in paths]

        for source in installs:
            self.assertIn(
                'cargo install wasm-pack --version "$(python3 scripts/tool_versions.py --version)" --locked', source
            )
            self.assertNotRegex(source, r"cargo install wasm-pack --version [0-9]")

    def test_freshness_is_scheduled_read_only_and_never_creates_pull_requests(self):
        root = Path(__file__).resolve().parents[1]

        source = (root / ".github/workflows/tool-freshness.yml").read_text(encoding="ascii")

        self.assertIn("schedule:", source)
        self.assertIn("workflow_dispatch:", source)
        self.assertIn("contents: read", source)
        self.assertIn("persist-credentials: false", source)
        self.assertIn("python3 scripts/tool_versions.py --check", source)
        self.assertNotIn("--update", source)
        self.assertNotIn("contents: write", source)
        self.assertNotIn("pull-requests:", source)
        self.assertNotIn("GH_TOKEN", source)

    def test_dependabot_covers_authored_browser_and_rust_toolchain(self):
        root = Path(__file__).resolve().parents[1]

        source = (root / ".github/dependabot.yml").read_text(encoding="ascii")

        self.assertIn("package-ecosystem: npm\n    directory: /tools/browser", source)
        self.assertIn("package-ecosystem: rust-toolchain\n    directory: /", source)

    def test_wasm_script_missing_tool_reports_the_consumed_pin(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            (root / "tools").mkdir()
            (root / "bin").mkdir()
            actual_root = Path(__file__).resolve().parents[1]
            for name in ("build-wasm.sh", "tool_versions.py"):
                (root / "scripts" / name).write_bytes((actual_root / "scripts" / name).read_bytes())

            (root / "tools/wasm-pack-version.txt").write_text("0.16.0\n", encoding="ascii")
            (root / "bin/dirname").symlink_to("/usr/bin/dirname")
            (root / "bin/python3").symlink_to(sys.executable)
            environment = dict(os.environ, PATH=str(root / "bin"))

            result = subprocess.run(["/bin/bash", str(root / "scripts/build-wasm.sh")],
                                    env=environment, capture_output=True, text=True, check=False)

            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stderr, "wasm-pack 0.16.0 is required; install with "
                             "cargo install wasm-pack --version 0.16.0 --locked.\n")
            self.assertEqual(result.stdout, "")

    def test_wasm_script_rejects_mismatched_installed_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "scripts").mkdir()
            (root / "tools").mkdir()
            (root / "bin").mkdir()
            actual_root = Path(__file__).resolve().parents[1]
            for name in ("build-wasm.sh", "tool_versions.py"):
                (root / "scripts" / name).write_bytes((actual_root / "scripts" / name).read_bytes())

            (root / "tools/wasm-pack-version.txt").write_text("0.16.0\n", encoding="ascii")
            (root / "Cargo.lock").write_text("fixture", encoding="ascii")
            tool = root / "bin/wasm-pack"
            tool.write_text("#!/bin/sh\nprintf '%s\\n' 'wasm-pack 0.15.0'\n", encoding="ascii")
            tool.chmod(0o755)
            environment = dict(os.environ, PATH=str(tool.parent) + os.pathsep + os.environ["PATH"])

            result = subprocess.run(["/bin/bash", str(root / "scripts/build-wasm.sh")],
                                    env=environment, capture_output=True, text=True, check=False)

            self.assertEqual(result.returncode, 1)
            self.assertIn("wasm-pack 0.16.0 and the committed Cargo.lock are required", result.stderr)
            self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
