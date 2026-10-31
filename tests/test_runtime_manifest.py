"""Verify deterministic runtime identities and bounded manifest inputs."""

from __future__ import annotations

import json
import re
from pathlib import Path
import shutil
import tempfile
import unittest

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import runtime_manifest as MANIFEST


class RuntimeManifestTests(unittest.TestCase):
    """Use synthetic source trees; no native binary or real account data is required."""

    def setUp(self) -> None:
        """Create every declared source and browser asset in an isolated root."""
        self.temporary = tempfile.TemporaryDirectory(prefix="sourcefield-manifest-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name in MANIFEST.SOURCE_INPUTS:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"fixture: {name}\n", encoding="ascii")

        helper = self.root / "tools/sourcefield-wasm/src/main.rs"
        helper.parent.mkdir(parents=True)
        helper.write_text("fixture binding adapter\n", encoding="ascii")
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0"\n', encoding="ascii")
        (self.root / "runtime/pkg").mkdir()
        (self.root / "runtime/pkg/sourcefield_wasm.js").write_text("export default function() {}", encoding="ascii")
        (self.root / "runtime/pkg/sourcefield_wasm_bg.wasm").write_bytes(b"\0asm\x01\0\0\0")

    def test_complete_local_manifest_is_deterministic(self) -> None:
        """A local build uses honest unreleased provenance and all required file digests."""
        first = MANIFEST.create_manifest(self.root)
        second = MANIFEST.create_manifest(self.root)

        self.assertEqual(first, second)
        self.assertEqual(first["source_revision"], "unreleased")
        self.assertEqual(first["generator_version"], "0.1.0")
        self.assertEqual(set(first["files"]), set(MANIFEST.RUNTIME_FILES))
        self.assertEqual(len(first["source_fingerprint"]), 64)
        self.assertTrue(json.dumps(first).isascii())

    def test_fingerprint_does_not_depend_on_checkout_path(self) -> None:
        """Relocation preserves source identity across release matrix runners."""
        other = self.root / "other-checkout"
        for name in MANIFEST.source_inputs(self.root):
            destination = other / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.root / name, destination)

        original = MANIFEST.source_fingerprint(self.root)
        relocated = MANIFEST.source_fingerprint(other)

        self.assertEqual(original, relocated)

    def test_source_change_changes_fingerprint(self) -> None:
        """Modified simulation code cannot retain an older runtime source identity."""
        before = MANIFEST.source_fingerprint(self.root)
        source = self.root / "crates/sourcefield-wasm/src/lib.rs"

        source.write_text("changed simulation", encoding="ascii")
        after = MANIFEST.source_fingerprint(self.root)

        self.assertNotEqual(before, after)

    def test_optimizer_manifest_change_changes_source_identity(self) -> None:
        """A changed authenticated optimizer selection invalidates the native/browser pair."""
        pin = self.root / "tools/binaryen.json"
        before = MANIFEST.source_fingerprint(self.root)

        pin.write_text("changed optimizer digest\n", encoding="ascii")
        after = MANIFEST.source_fingerprint(self.root)

        self.assertNotEqual(before, after)

    def test_rust_and_python_inputs_have_identical_order(self) -> None:
        """Both implementations hash the same authored paths in the same sequence."""
        root = Path(__file__).resolve().parents[1]
        source = (root / "crates/sourcefield-cli/build.rs").read_text(encoding="ascii")
        declaration = source.split("const INPUTS: &[&str] = &[", 1)[1].split("];", 1)[0]

        rust_inputs = tuple(re.findall(r'"([^"]+)"', declaration))

        self.assertEqual(rust_inputs, MANIFEST.SOURCE_INPUTS)
        self.assertIn("tools/binaryen.json", rust_inputs)

    def test_new_binding_module_changes_source_identity(self) -> None:
        """New adapter modules are discovered without hand-editing the fingerprint list."""
        module = self.root / "tools/sourcefield-wasm/src/extra.rs"
        before = MANIFEST.source_fingerprint(self.root)

        module.write_text("new binding behavior", encoding="ascii")
        after = MANIFEST.source_fingerprint(self.root)

        self.assertNotEqual(before, after)
        self.assertIn("tools/sourcefield-wasm/src/extra.rs", MANIFEST.source_inputs(self.root))

    def test_binding_build_script_changes_source_identity(self) -> None:
        """Cargo's auto-detected build.rs is a producer input alongside src modules."""
        script = self.root / "tools/sourcefield-wasm/build.rs"
        before = MANIFEST.source_fingerprint(self.root)

        script.write_text("fn main() {}", encoding="ascii")
        after = MANIFEST.source_fingerprint(self.root)

        self.assertNotEqual(before, after)
        self.assertIn("tools/sourcefield-wasm/build.rs", MANIFEST.source_inputs(self.root))

    def test_removed_binding_module_changes_source_identity(self) -> None:
        """Removed helper behavior cannot retain the source identity captured before removal."""
        module = self.root / "tools/sourcefield-wasm/src/extra.rs"
        module.write_text("binding behavior", encoding="ascii")
        before = MANIFEST.source_fingerprint(self.root)

        module.unlink()
        after = MANIFEST.source_fingerprint(self.root)

        self.assertNotEqual(before, after)

    def test_missing_binding_entrypoint_is_rejected(self) -> None:
        """An empty helper directory is not a complete producer inventory."""
        (self.root / "tools/sourcefield-wasm/src/main.rs").unlink()

        with self.assertRaisesRegex(ValueError, "main.rs is required"):
            MANIFEST.source_fingerprint(self.root)

    def test_nested_binding_sources_are_sorted(self) -> None:
        """Filesystem traversal order cannot alter the Rust/Python inventory contract."""
        source = self.root / "tools/sourcefield-wasm/src"
        (source / "nested").mkdir()
        (source / "z.rs").write_text("z", encoding="ascii")
        (source / "nested/a.rs").write_text("a", encoding="ascii")

        names = MANIFEST.source_inputs(self.root)[len(MANIFEST.SOURCE_INPUTS):]

        self.assertEqual(names, (
            "tools/sourcefield-wasm/src/main.rs", "tools/sourcefield-wasm/src/nested/a.rs",
            "tools/sourcefield-wasm/src/z.rs",
        ))

    def test_generated_helper_output_does_not_change_source_identity(self) -> None:
        """Caches within a helper tree are excluded like native generator build outputs."""
        generated = self.root / "tools/sourcefield-wasm/src/target/generated.rs"
        generated.parent.mkdir()
        before = MANIFEST.source_fingerprint(self.root)

        generated.write_text("compiler output", encoding="ascii")
        after = MANIFEST.source_fingerprint(self.root)

        self.assertEqual(before, after)

    @unittest.skipUnless(hasattr(Path, "symlink_to"), "symlink support required")
    def test_symlinked_binding_module_is_rejected(self) -> None:
        """A source link cannot substitute unchecked producer code into a trusted identity."""
        source = self.root / "tools/sourcefield-wasm/src/main.rs"
        original = self.root / "original.rs"
        source.rename(original)
        try:
            source.symlink_to(original)
        except OSError as error:
            self.skipTest(f"symlink creation unavailable: {error}")

        with self.assertRaisesRegex(ValueError, "symlink"):
            MANIFEST.source_fingerprint(self.root)

    def test_full_release_revision_is_preserved(self) -> None:
        """Release manifests retain the explicit full immutable source commit."""
        revision = "a" * 40

        result = MANIFEST.create_manifest(self.root, revision)

        self.assertEqual(result["source_revision"], revision)

    def test_short_revision_is_rejected(self) -> None:
        """Human-friendly abbreviated commits cannot become provenance claims."""
        with self.assertRaisesRegex(ValueError, "full commit SHA"):
            MANIFEST.create_manifest(self.root, "abc123")

    def test_missing_wasm_is_rejected(self) -> None:
        """A manifest cannot describe a partial native/browser release bundle."""
        (self.root / "runtime/pkg/sourcefield_wasm_bg.wasm").unlink()

        with self.assertRaisesRegex(ValueError, "regular file"):
            MANIFEST.create_manifest(self.root)

    def test_invalid_wasm_version_is_rejected(self) -> None:
        """The magic prefix alone does not prove a valid supported WASM header."""
        (self.root / "runtime/pkg/sourcefield_wasm_bg.wasm").write_bytes(b"\0asmgarbage")

        with self.assertRaisesRegex(ValueError, "WebAssembly v1"):
            MANIFEST.create_manifest(self.root)

    def test_traversal_input_is_rejected(self) -> None:
        """Even direct helper calls cannot escape the explicitly selected bundle."""
        with self.assertRaisesRegex(ValueError, "within the bundle"):
            MANIFEST.read_regular(self.root, "../outside")

    def test_oversized_input_is_rejected(self) -> None:
        """Sparse huge files are rejected before their contents are allocated."""
        with (self.root / "runtime/app.js").open("wb") as handle:
            handle.truncate(MANIFEST.MAX_FILE_BYTES + 1)

        with self.assertRaisesRegex(ValueError, "16 MiB"):
            MANIFEST.create_manifest(self.root)

    @unittest.skipUnless(hasattr(Path, "symlink_to"), "symlink support required")
    def test_symlinked_bundle_directory_is_rejected(self) -> None:
        """Reject directory symlinks as well as final-component file links."""
        package = self.root / "runtime/pkg"
        package.rename(self.root / "runtime/actual-pkg")
        try:
            package.symlink_to("actual-pkg", target_is_directory=True)
        except OSError as error:
            self.skipTest(f"symlink creation unavailable: {error}")

        with self.assertRaisesRegex(ValueError, "symlink"):
            MANIFEST.create_manifest(self.root)


if __name__ == "__main__":
    unittest.main()
