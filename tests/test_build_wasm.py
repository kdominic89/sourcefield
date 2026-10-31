"""Exercise build publication boundaries without trusting installed packaging tools."""

from contextlib import contextmanager
import io
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import build_wasm


class CargoArtifactTests(unittest.TestCase):
    """Cargo's explicit target and artifact protocol must survive configured defaults."""

    def setUp(self):
        """Create one artifact and a process double with actual protocol bytes."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.wasm = self.root / "custom-target/sourcefield_wasm.wasm"
        self.wasm.parent.mkdir()
        self.wasm.write_bytes(build_wasm.WASM_HEADER)

    def process(self, messages, code=0):
        """Model only the streamed process methods consumed by the build boundary."""
        class Process:
            stdout = io.BytesIO(b"".join(json.dumps(value).encode() + b"\n" for value in messages))
            returncode = code

            def wait(self):
                return self.returncode

            def poll(self):
                return self.returncode

        return Process()

    def message(self):
        """Return a cdylib artifact with the Cargo-selected custom output path."""
        return {"reason": "compiler-artifact", "target": {"name": "sourcefield_wasm", "crate_types": ["cdylib"]},
                "filenames": [str(self.wasm)]}

    def test_explicit_wasm_target_and_locked_release_override_native_environment(self):
        # Arrange
        process = self.process([self.message()])
        with patch.dict(os.environ, {"CARGO_BUILD_TARGET": "aarch64-apple-darwin"}), \
                patch.object(build_wasm.subprocess, "Popen", return_value=process) as spawn:
            # Act
            artifact = build_wasm.cargo_artifact(self.root, "sourcefield-wasm", "wasm32-unknown-unknown", offline=True)

        # Assert
        self.assertEqual(artifact, self.wasm)
        command = spawn.call_args.args[0]
        self.assertEqual(command[command.index("--target") + 1], "wasm32-unknown-unknown")
        self.assertIn("--locked", command)
        self.assertIn("--release", command)
        self.assertIn("--offline", command)

    def test_duplicate_artifacts_are_rejected(self):
        # Arrange
        with patch.object(build_wasm.subprocess, "Popen", return_value=self.process([self.message()] * 2)):
            # Act / Assert
            with self.assertRaisesRegex(ValueError, "exactly one"):
                build_wasm.cargo_artifact(self.root, "sourcefield-wasm", "wasm32-unknown-unknown", offline=False)

    def test_failed_compiler_cannot_reuse_reported_artifact(self):
        # Arrange
        with patch.object(build_wasm.subprocess, "Popen", return_value=self.process([self.message()], code=9)):
            # Act / Assert
            with self.assertRaises(subprocess.CalledProcessError):
                build_wasm.cargo_artifact(self.root, "sourcefield-wasm", "wasm32-unknown-unknown", offline=False)

    def test_native_adapter_uses_the_explicit_host_artifact(self):
        # Arrange
        message = {"reason": "compiler-artifact", "target": {"name": "sourcefield-wasm-build"},
                   "executable": str(self.wasm)}
        with patch.object(build_wasm.subprocess, "Popen", return_value=self.process([message])) as spawn:
            # Act
            result = build_wasm.cargo_artifact(
                self.root, "sourcefield-wasm-build", "aarch64-apple-darwin", offline=False
            )

        # Assert
        self.assertEqual(result, self.wasm)
        command = spawn.call_args.args[0]
        self.assertEqual(command[command.index("--target") + 1], "aarch64-apple-darwin")


class BuildPublicationTests(unittest.TestCase):
    """Every failed producer stage must invalidate stale success provenance."""

    def setUp(self):
        """Prepare old output and deterministic producer doubles, never external tools."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "Cargo.lock").write_text("locked", encoding="ascii")
        (self.root / "runtime/pkg").mkdir(parents=True)
        self.manifest = self.root / "runtime/runtime-manifest.json"
        self.manifest.write_text("old manifest", encoding="ascii")
        (self.root / "runtime/pkg/sourcefield_wasm.js").write_text("old module", encoding="ascii")
        self.calls = []

    def producer(self, command, **kwargs):
        """Write only the same named stage outputs as real binding generation and optimization."""
        self.calls.append(command)
        if command[0] == "/adapter":
            output = Path(command[2])
            for name in ("sourcefield_wasm.js", "sourcefield_wasm.d.ts", "sourcefield_wasm_bg.wasm.d.ts"):
                (output / name).write_text("export {};", encoding="ascii")

            (output / "sourcefield_wasm_bg.wasm").write_bytes(build_wasm.WASM_HEADER)
        else:
            Path(command[command.index("-o") + 1]).write_bytes(build_wasm.WASM_HEADER + b"optimized")

    @contextmanager
    def dependencies(self, producer=None, snapshot=None):
        """Patch external boundaries while retaining real staging, validation and publication."""
        with patch.object(build_wasm, "read_manifest", return_value={"optimizer_flags": ["-O"]}), \
                patch.object(build_wasm, "host_target", return_value="aarch64-apple-darwin"), \
                patch.object(build_wasm, "ensure_optimizer", return_value=Path("/verified/wasm-opt")) as optimizer, \
                patch.object(build_wasm, "cargo_artifact", side_effect=[Path("/input.wasm"), Path("/adapter")]), \
                patch.object(build_wasm.subprocess, "run", side_effect=producer or self.producer), \
                patch.object(build_wasm, "source_fingerprint", return_value="new"), \
                patch.object(build_wasm, "snapshot_optimizer", side_effect=snapshot,
                             return_value=Path("/snapshot/wasm-opt")), \
                patch.object(build_wasm, "create_manifest", return_value={"source_fingerprint": "new"}) as manifest:
            yield optimizer, manifest

    def test_success_publishes_fresh_pair_before_manifest(self):
        # Arrange
        with self.dependencies() as (optimizer, manifest):
            # Act
            build_wasm.build(self.root, offline=True)

        # Assert
        self.assertEqual(json.loads(self.manifest.read_text()), {"source_fingerprint": "new"})
        self.assertEqual((self.root / "runtime/pkg/sourcefield_wasm_bg.wasm").read_bytes(),
                         build_wasm.WASM_HEADER + b"optimized")
        self.assertEqual(self.calls[1][0], "/snapshot/wasm-opt")
        self.assertTrue(optimizer.call_args.kwargs["offline"])
        manifest.assert_called_once()
        self.assertFalse((self.root / "build/.wasm-build.lock").exists())

    def test_binding_failure_removes_old_manifest_and_preserves_old_module(self):
        # Arrange
        error = subprocess.CalledProcessError(1, "adapter")
        with self.dependencies(producer=lambda *args, **kwargs: (_ for _ in ()).throw(error)) as (_, manifest):
            # Act / Assert
            with self.assertRaises(subprocess.CalledProcessError):
                build_wasm.build(self.root)

        self.assertFalse(self.manifest.exists())
        self.assertEqual((self.root / "runtime/pkg/sourcefield_wasm.js").read_text(), "old module")
        manifest.assert_not_called()

    def test_optimizer_failure_cannot_publish_bindings(self):
        # Arrange
        def producer(command, **kwargs):
            if command[0] == "/snapshot/wasm-opt":
                raise subprocess.CalledProcessError(2, command)

            self.producer(command, **kwargs)

        with self.dependencies(producer) as (_, manifest):
            # Act / Assert
            with self.assertRaises(subprocess.CalledProcessError):
                build_wasm.build(self.root)

        self.assertFalse(self.manifest.exists())
        self.assertEqual((self.root / "runtime/pkg/sourcefield_wasm.js").read_text(), "old module")
        manifest.assert_not_called()

    def test_missing_optimizer_output_cannot_publish_stale_wasm(self):
        # Arrange
        def producer(command, **kwargs):
            if command[0] == "/adapter":
                self.producer(command, **kwargs)

        with self.dependencies(producer) as (_, manifest):
            # Act / Assert
            with self.assertRaisesRegex(ValueError, "build file must be regular"):
                build_wasm.build(self.root)

        self.assertFalse(self.manifest.exists())
        manifest.assert_not_called()

    def test_changed_producer_inputs_cannot_receive_fresh_provenance(self):
        # Arrange
        with self.dependencies() as (_, manifest), \
                patch.object(build_wasm, "source_fingerprint", side_effect=["old", "changed"]):
            # Act / Assert
            with self.assertRaisesRegex(ValueError, "producer inputs changed"):
                build_wasm.build(self.root)

        self.assertFalse(self.manifest.exists())
        self.assertEqual((self.root / "runtime/pkg/sourcefield_wasm.js").read_text(), "old module")
        manifest.assert_not_called()

    def test_cache_replaced_during_compilation_cannot_execute(self):
        # Arrange
        cache = self.root.resolve() / "build/cache/bin/wasm-opt"
        cache.parent.mkdir(parents=True)
        approved = b"approved optimizer"
        cache.write_bytes(approved)
        policy = json.loads((support.ROOT / "tools/binaryen.json").read_text())
        host = "x86_64-unknown-linux-gnu"
        policy["hosts"][host]["files"] = [{"path": "bin/wasm-opt", "size": len(approved),
                                         "sha256": hashlib.sha256(approved).hexdigest()}]

        def compile_stage(root, package, target, *, offline):
            if package == "sourcefield-wasm":
                cache.write_bytes(b"replaced optimizer")

            return Path("/input.wasm" if package == "sourcefield-wasm" else "/adapter")

        with self.dependencies(snapshot=build_wasm.snapshot_optimizer) as (_, manifest), \
                patch.object(build_wasm, "read_manifest", return_value=policy), \
                patch.object(build_wasm, "host_target", return_value=host), \
                patch.object(build_wasm, "ensure_optimizer", return_value=cache), \
                patch.object(build_wasm, "cargo_artifact", side_effect=compile_stage):
            # Act / Assert
            with self.assertRaisesRegex(ValueError, "size|SHA-256"):
                build_wasm.build(self.root)

        self.assertEqual([call[0] for call in self.calls], ["/adapter"])
        self.assertFalse(self.manifest.exists())
        manifest.assert_not_called()

    def test_missing_lock_does_not_start_tool_provisioning(self):
        # Arrange
        (self.root / "Cargo.lock").unlink()
        with patch.object(build_wasm, "ensure_optimizer") as optimizer:
            # Act / Assert
            with self.assertRaisesRegex(ValueError, "Cargo.lock"):
                build_wasm.build(self.root)

        self.assertFalse(self.manifest.exists())
        optimizer.assert_not_called()

    def test_concurrent_publication_is_rejected(self):
        # Arrange
        with build_wasm.build_lock(self.root):
            # Act / Assert
            with self.assertRaises(FileExistsError):
                with build_wasm.build_lock(self.root):
                    self.fail("must not enter a concurrent build")


if __name__ == "__main__":
    unittest.main()
