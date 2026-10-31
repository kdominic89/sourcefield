#!/usr/bin/env python3
"""Build locked browser bindings with an authenticated, pinned optimizer."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from runtime_manifest import create_manifest, source_fingerprint
from sourcefield_tools.binaryen import ensure_optimizer, read_manifest, snapshot_optimizer

ROOT = Path(__file__).resolve().parents[1]
MAX_CARGO_LINE = 2 * 1024 * 1024
WASM_HEADER = b"\0asm\x01\0\0\0"


def regular_file(path: Path) -> None:
    """Refuse symlink substitution and nonregular build inputs or outputs."""
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"build file must be regular: {path}")


def directory(path: Path) -> None:
    """Create an owned build directory without following a substituted component."""
    for parent in reversed((path, *path.parents)):
        if parent.is_symlink():
            raise ValueError(f"build directory must not contain symlinks: {parent}")

    path.mkdir(parents=True, exist_ok=True)


@contextmanager
def build_lock(root: Path):
    """Serialize publication so concurrent builds cannot combine artifact generations."""
    root = root.resolve()
    directory(root / "build")
    lock = root / "build/.wasm-build.lock"
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(descriptor)
    try:
        yield
    finally:
        lock.unlink()


def host_target(root: Path) -> str:
    """Read rustc's native host rather than inheriting Cargo's configured build target."""
    result = subprocess.run(["rustc", "-vV"], cwd=root, check=True, capture_output=True, text=True)
    hosts = re.findall(r"^host: ([a-z0-9_]+(?:-[a-z0-9_]+){2,4})$", result.stdout, re.MULTILINE)
    if len(hosts) != 1:
        raise ValueError("rustc must report exactly one canonical native host")

    return hosts[0]


def cargo_artifact(root: Path, package: str, target: str, *, offline: bool) -> Path:
    """Stream Cargo's artifact protocol, respecting explicit targets and custom target directories."""
    command = ["cargo", "build", "--locked", "--release", "-p", package,
               "--target", target, "--message-format=json-render-diagnostics"]
    if offline:
        command.append("--offline")

    candidates = []
    process = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE)
    try:
        assert process.stdout is not None
        while line := process.stdout.readline(MAX_CARGO_LINE + 1):
            if len(line) > MAX_CARGO_LINE:
                raise ValueError("Cargo artifact message exceeds 2 MiB")

            message = json.loads(line)
            if message.get("reason") == "compiler-message":
                rendered = message.get("message", {}).get("rendered")
                if rendered:
                    print(rendered, file=sys.stderr, end="")

            if message.get("reason") != "compiler-artifact":
                continue

            artifact = message.get("target", {})
            expected_name = "sourcefield_wasm" if package == "sourcefield-wasm" else "sourcefield-wasm-build"
            if artifact.get("name") != expected_name:
                continue

            if package == "sourcefield-wasm" and "cdylib" in artifact.get("crate_types", []):
                candidates.extend(Path(name) for name in message.get("filenames", []) if name.endswith(".wasm"))
            elif package == "sourcefield-wasm-build" and message.get("executable"):
                candidates.append(Path(message["executable"]))

        if process.wait() != 0:
            raise subprocess.CalledProcessError(process.returncode, command)

    finally:
        if process.poll() is None:
            process.kill()
            process.wait()

        if process.stdout is not None:
            process.stdout.close()

    if len(candidates) != 1:
        raise ValueError(f"Cargo must report exactly one {package} artifact")

    artifact = candidates[0]
    if not artifact.is_absolute():
        artifact = root / artifact

    regular_file(artifact)

    return artifact


def validate_bundle(output: Path) -> None:
    """Require a nonempty ES module and valid WebAssembly header before publication."""
    for name in ("sourcefield_wasm.js", "sourcefield_wasm_bg.wasm"):
        path = output / name
        regular_file(path)
        size = path.stat().st_size
        if not 0 < size <= 16 * 1024 * 1024:
            raise ValueError(f"browser artifact must contain 1 byte to 16 MiB: {name}")

    with (output / "sourcefield_wasm_bg.wasm").open("rb") as stream:
        if stream.read(8) != WASM_HEADER:
            raise ValueError("invalid WebAssembly v1 module header")


def build(root: Path, *, offline: bool = False) -> None:
    """Compile, bind and optimize fresh artifacts, then attest the complete local runtime."""
    root = root.resolve()
    directory(root / "runtime")
    with build_lock(root):
        manifest_path = root / "runtime/runtime-manifest.json"
        if manifest_path.is_symlink():
            raise ValueError("runtime manifest must not be a symlink")

        # A failed attempt must never leave an old bundle presented as a newly verified build.
        manifest_path.unlink(missing_ok=True)
        regular_file(root / "Cargo.lock")
        initial_fingerprint = source_fingerprint(root)
        policy = read_manifest(root)
        host = host_target(root)
        optimizer = ensure_optimizer(root, host, root / "build/binaryen", offline=offline)
        wasm = cargo_artifact(root, "sourcefield-wasm", "wasm32-unknown-unknown", offline=offline)
        bindings = cargo_artifact(root, "sourcefield-wasm-build", host, offline=offline)
        with tempfile.TemporaryDirectory(prefix="wasm-", dir=root / "build") as temporary:
            staging = Path(temporary)
            # Bound worker stacks on many-core hosts; this small module does not need a host-wide pool.
            environment = dict(os.environ, RAYON_NUM_THREADS="1", BINARYEN_CORES="1")
            # Keep loader overrides from replacing the authenticated optimizer or its companion library.
            for name in ("LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES"):
                environment.pop(name, None)

            subprocess.run([str(bindings), str(wasm), str(staging)], cwd=root, env=environment, check=True)
            validate_bundle(staging)
            original = staging / "sourcefield_wasm_bg.wasm"
            optimized = staging / "optimized.wasm"
            executable = snapshot_optimizer(optimizer, policy, host, staging / "optimizer")
            subprocess.run([str(executable), str(original), "-o", str(optimized), *policy["optimizer_flags"]],
                           cwd=root, env=environment, check=True)
            regular_file(optimized)
            optimized.replace(original)
            validate_bundle(staging)
            if source_fingerprint(root) != initial_fingerprint:
                raise ValueError("producer inputs changed during the WASM build; rebuild from stable sources")

            destination = root / "runtime/pkg"
            directory(destination)
            # Publish only authored output names; the manifest is the final generation-validity marker.
            for name in ("sourcefield_wasm.js", "sourcefield_wasm_bg.wasm", "sourcefield_wasm.d.ts",
                         "sourcefield_wasm_bg.wasm.d.ts"):
                regular_file(staging / name)
                if (destination / name).is_symlink():
                    raise ValueError(f"runtime destination must not be a symlink: {name}")

                shutil.copyfile(staging / name, destination / name)

        manifest = create_manifest(root, os.environ.get("SOURCEFIELD_SOURCE_COMMIT", "unreleased"))
        if manifest["source_fingerprint"] != initial_fingerprint:
            raise ValueError("producer inputs changed before runtime publication; rebuild from stable sources")

        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="ascii")
        print(f"Runtime manifest: {manifest_path} ({manifest['source_fingerprint']})")


def main(argv: list[str] | None = None) -> int:
    """Build from the repository root; offline mode prohibits Cargo and tool downloads."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="use only already cached tool and crate inputs")
    args = parser.parse_args(argv)
    try:
        build(ROOT, offline=args.offline)

        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"WASM build failed: {error}", file=sys.stderr)

        return 1


if __name__ == "__main__":
    raise SystemExit(main())
