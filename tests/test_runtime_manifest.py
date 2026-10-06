"""Verify deterministic runtime identities and bounded manifest inputs."""

from __future__ import annotations

import importlib.util
import json
import re
from pathlib import Path
import shutil
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "runtime_manifest", Path(__file__).resolve().parents[1] / "scripts/runtime_manifest.py"
)
assert SPEC and SPEC.loader
MANIFEST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MANIFEST)


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
        for name in MANIFEST.SOURCE_INPUTS:
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

    def test_packager_pin_change_changes_source_identity(self) -> None:
        """A new consumed packager pin cannot retain an older native/browser identity."""
        pin = self.root / "tools/wasm-pack-version.txt"
        before = MANIFEST.source_fingerprint(self.root)

        pin.write_text("0.16.0\n", encoding="ascii")
        after = MANIFEST.source_fingerprint(self.root)

        self.assertNotEqual(before, after)

    def test_rust_and_python_inputs_have_identical_order(self) -> None:
        """Both implementations hash the same authored paths in the same sequence."""
        root = Path(__file__).resolve().parents[1]
        source = (root / "crates/sourcefield-cli/build.rs").read_text(encoding="ascii")
        declaration = source.split("const INPUTS: &[&str] = &[", 1)[1].split("];", 1)[0]

        rust_inputs = tuple(re.findall(r'"([^"]+)"', declaration))

        self.assertEqual(rust_inputs, MANIFEST.SOURCE_INPUTS)
        self.assertIn("tools/wasm-pack-version.txt", rust_inputs)

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
