"""Positive and adversarial coverage for public SVG and source packaging boundaries."""

import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest
import zipfile
from pathlib import Path


def module(name):
    """Load repository tools directly without installing a Python package."""
    spec = importlib.util.spec_from_file_location(
        name, Path(__file__).resolve().parents[1] / "scripts" / f"{name}.py"
    )

    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)

    return loaded


validator = module("validate_artifact")
packager = module("package_source")


class SvgPolicyTests(unittest.TestCase):
    """Ensure public links remain useful without admitting executable/resource payloads."""

    def test_allows_public_anchor_and_local_paint(self):
        source = (
            '<svg xmlns="http://www.w3.org/2000/svg"><a href="https://www.nuget.org/packages/test">'
            '<rect fill="url(#paint)"/></a></svg>'
        )

        validator.validate_svg_payload(source)

    def test_rejects_active_content_and_external_resources(self):
        payloads = [
            '<rect style="fill: image-set(&quot;https://example.com/a&quot;)"/>',
            "<script/>",
            "<foreignObject/>",
            '<rect onload="alert(1)"/>',
            '<use href="https://example.com/resource"/>',
            '<a href="javascript:alert(1)"/>',
            '<rect fill="url(https://example.com/image)"/>',
            '<style>@import "https://example.com";</style>',
            "<style>.a { fill: u\\72l(https://example.com); }</style>",
            '<animate attributeName="href" values="javascript:alert(1)"/>',
            '<a href="https://user:pass@example.com"/>',
            '<rect xml:base="https://example.com"/>',
        ]

        for payload in payloads:
            with self.subTest(payload=payload), self.assertRaises(AssertionError):
                validator.validate_svg_payload(
                    f'<svg xmlns="http://www.w3.org/2000/svg">{payload}</svg>'
                )

    def test_allows_plain_accessibility_text_that_looks_like_css(self):
        source = ('<svg xmlns="http://www.w3.org/2000/svg" '
                  'aria-label="Metadata: image(foo) and data: documentation"/>')

        validator.validate_svg_payload(source)

    def test_rejects_entity_declarations(self):
        source = '<!DOCTYPE svg [<!ENTITY x "payload">]><svg xmlns="http://www.w3.org/2000/svg"/>'

        with self.assertRaises(AssertionError):
            validator.validate_svg_payload(source)


class ImportedAuthorityTests(unittest.TestCase):
    """Supplemental checks consume merged configuration, never infer approval from output nodes."""

    def test_merged_imported_packages_are_authoritative(self):
        authored = {"profile": {"username": "example"}, "imports": [{"id": "other"}]}
        resolved = {"schema_version": 1, "profile": authored["profile"],
                    "publications": [{"packages": [{"id": "Other.Provider"}]}]}
        with tempfile.TemporaryDirectory() as temporary:
            assets = Path(temporary)
            (assets / "resolved-config.json").write_text(json.dumps(resolved))

            result = validator.effective_config(authored, assets)

            self.assertEqual(result["publications"][0]["packages"][0]["id"], "Other.Provider")

    def test_missing_import_capture_is_rejected(self):
        authored = {"imports": [{"id": "other"}]}
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(AssertionError, "require resolved-config"):
                validator.effective_config(authored, Path(temporary))

    def test_changed_captured_identity_is_rejected(self):
        authored = {"profile": {"username": "example"}}
        resolved = {"schema_version": 1, "profile": {"username": "someone-else"}}
        with tempfile.TemporaryDirectory() as temporary:
            assets = Path(temporary)
            (assets / "resolved-config.json").write_text(json.dumps(resolved))

            with self.assertRaisesRegex(AssertionError, "does not match"):
                validator.effective_config(authored, assets)


class PackageTests(unittest.TestCase):
    """Protect deterministic archives and the source-only filesystem boundary."""

    def test_archive_is_deterministic_and_excludes_generated_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name in (
                "Cargo.lock",
                "SOURCES.md",
                "tools/wasm-pack-version.txt",
                "tools/browser/package.json",
                "tools/browser/package-lock.json",
                "tools/browser/node_modules/playwright/cli.js",
                "tools/browser/debug.log",
                "docs/site.webmanifest",
                "crates/core/src/lib.rs",
                "docs/app.js",
                "docs/pkg/generated.js",
                "docs/pkg/compiled.wasm",
                "runtime/runtime-manifest.json",
                "runtime/pkg/sourcefield_wasm_bg.wasm",
                ".fastembed_cache/model.bin",
                "assets/.DS_Store",
                "scripts/__pycache__/tool.pyc",
                "scripts/verify-browser.mjs",
                "tests/integration/field.mjs",
                "target/cache.rs",
            ):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)

            first = packager.package(root, root / "dist/first.zip")
            second = packager.package(root, root / "dist/second.zip")

            self.assertEqual(first, second)
            self.assertTrue((root / ".fastembed_cache/model.bin").is_file())
            with zipfile.ZipFile(root / "dist/first.zip") as archive:
                self.assertEqual(
                    set(archive.namelist()),
                    {
                        "sourcefield/Cargo.lock",
                        "sourcefield/crates/core/src/lib.rs",
                        "sourcefield/docs/app.js",
                        "sourcefield/scripts/verify-browser.mjs",
                        "sourcefield/tests/integration/field.mjs",
                        "sourcefield/SHA256SUMS",
                        "sourcefield/SOURCES.md",
                        "sourcefield/tools/wasm-pack-version.txt",
                        "sourcefield/tools/browser/package.json",
                        "sourcefield/tools/browser/package-lock.json",
                        "sourcefield/docs/site.webmanifest",
                    },
                )
                for line in (
                    archive.read("sourcefield/SHA256SUMS").decode().splitlines()
                ):
                    digest, relative = line.split("  ", 1)
                    self.assertEqual(
                        digest,
                        hashlib.sha256(
                            archive.read(f"sourcefield/{relative}")
                        ).hexdigest(),
                    )

    def test_rejects_symlink_without_reading_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "docs").mkdir()
            (root / "docs/app.js").symlink_to("/etc/passwd")

            with self.assertRaises(ValueError):
                packager.package(root, root / "dist/output.zip")

    def test_generic_configuration_accepts_another_profile(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "config").mkdir()
            (root / "config/profile.toml").write_text(
                "schema_version = 1\n[render]\nwidth = 1800\nheight = 1680\n"
            )

            config = validator.validate_config(root)

            self.assertEqual(config["render"]["width"], 1800)


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


class StateSchemaTests(unittest.TestCase):
    """Validate the one current schema while refusing legacy and future formats."""

    def write_state(self, root: Path, schema: int) -> dict:
        """Arrange paired synthetic public state copies for an independent case."""
        config = {
            "render": {"width": 1800, "height": 1680},
            "profile": {"username": "example", "organization": "example-org"},
            "projects": [{"id": "example"}],
        }

        state = {
            "schema_version": schema,
            "semantic_hash": "A" * 16,
            "canvas": config["render"],
            "profile": config["profile"],
            "nodes": [
                {
                    "id": "project:example",
                    "kind": "project",
                    "x": 100,
                    "y": 100,
                    "radius": 20,
                }
            ],
            "edges": [{"from": "project:example", "to": "project:example"}],
            "packages": [],
            "stats": {"package_count": 0},
        }

        for directory in ("assets", "docs"):
            (root / directory).mkdir()
            (root / directory / "profile-state.json").write_text(json.dumps(state))

        return config

    def test_current_schema_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 3)

            result = validator.validate_state(root, config)

            self.assertEqual(result["schema_version"], 3)

    def test_authored_packages_without_cached_observations_are_valid(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 3)
            config["publications"] = [{"packages": [{"id": "Example.Package"}]}]
            state = json.loads((root / "assets/profile-state.json").read_text())
            state["nodes"].append({"id": "package:Example.Package", "kind": "package",
                                   "x": 200, "y": 200, "radius": 16})
            state["stats"]["package_count"] = 1
            for name in ("assets", "docs"):
                (root / name / "profile-state.json").write_text(json.dumps(state))

            result = validator.validate_state(root, config)

            self.assertEqual(result["stats"]["package_count"], 1)
            self.assertEqual(result["packages"], [])

    def test_legacy_schema_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 2)

            with self.assertRaisesRegex(
                AssertionError, "state schema_version must be 3"
            ):
                validator.validate_state(root, config)

    def test_future_schema_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 4)

            with self.assertRaisesRegex(
                AssertionError, "state schema_version must be 3"
            ):
                validator.validate_state(root, config)


if __name__ == "__main__":
    unittest.main()
