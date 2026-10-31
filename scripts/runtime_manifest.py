#!/usr/bin/env python3
"""Bind a freshly built browser bundle to its native generator source inputs.

This manifest proves local bundle agreement, not publisher authenticity. Release
consumers must additionally verify the release and build attestations.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tomllib


SOURCE_INPUTS = (
    "runtime/index.html",
    "runtime/app.css",
    "runtime/app.js",
    "runtime/simulation-fallback.js",
    "runtime/favicon.svg",
    "runtime/site.webmanifest",
    "crates/sourcefield-wasm/src/lib.rs",
    "crates/sourcefield-core/src/model.rs",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/config.toml",
    "rust-toolchain.toml",
    "crates/sourcefield-wasm/Cargo.toml",
    "scripts/build-wasm.sh",
    "scripts/build_wasm.py",
    "scripts/sourcefield_tools/binaryen.py",
    "tools/binaryen.json",
    "tools/sourcefield-wasm/Cargo.toml",
)
RUNTIME_FILES = (
    "index.html", "app.css", "app.js", "simulation-fallback.js", "favicon.svg",
    "site.webmanifest", "pkg/sourcefield_wasm.js", "pkg/sourcefield_wasm_bg.wasm",
)
MAX_FILE_BYTES = 16 * 1024 * 1024


def read_regular(root: Path, relative: str) -> bytes:
    """Read bounded regular files without allowing symlink substitution in any component."""
    relative_path = Path(relative)
    if relative_path.is_absolute() or ".." in relative_path.parts:
        raise ValueError("runtime input must stay within the bundle")

    path = root
    for component in relative_path.parts:
        path = path / component
        if path.is_symlink():
            raise ValueError(f"symlink in runtime input: {relative}")

    if not path.is_file() or path.stat().st_size > MAX_FILE_BYTES:
        raise ValueError(f"runtime input must be a regular file within 16 MiB: {relative}")

    with path.open("rb") as handle:
        data = handle.read(MAX_FILE_BYTES + 1)

    if len(data) > MAX_FILE_BYTES:
        raise ValueError(f"runtime input grew beyond 16 MiB: {relative}")

    return data


def source_inputs(root: Path) -> tuple[str, ...]:
    """Append sorted authored helper inputs; new Rust modules cannot escape provenance."""
    directory = root / "tools/sourcefield-wasm"
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError("binding helper source must be a real directory")

    def reject_walk_error(error: OSError) -> None:
        """Missing or unreadable source subtrees must fail rather than disappear from identity."""
        raise error

    names = []
    for current, directories, files in os.walk(directory, followlinks=False, onerror=reject_walk_error):
        for name in directories + files:
            path = Path(current) / name
            if path.is_symlink():
                raise ValueError("symlink in binding helper source")

        directories[:] = [name for name in directories if name not in {"target", ".git"}]
        for name in files:
            path = Path(current) / name
            if path.suffix == ".rs" or name == "Cargo.toml":
                if not stat.S_ISREG(path.stat().st_mode):
                    raise ValueError("binding helper source must be a regular file")

                relative = path.relative_to(root).as_posix()
                if not relative.isascii():
                    raise ValueError("source input paths must be ASCII")

                if relative not in SOURCE_INPUTS:
                    names.append(relative)

    if "tools/sourcefield-wasm/src/main.rs" not in names:
        raise ValueError("binding helper main.rs is required")

    return SOURCE_INPUTS + tuple(sorted(names))


def source_fingerprint(root: Path) -> str:
    """Match build.rs using explicit path/byte lengths, never absolute paths or mtimes."""
    digest = hashlib.sha256(b"sourcefield-runtime-source-v1\0")
    for name in source_inputs(root):
        encoded = name.encode("ascii")
        data = read_regular(root, name)
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)

    return digest.hexdigest()


def create_manifest(root: Path, revision: str = "unreleased") -> dict:
    """Describe the complete current bundle; call only after a successful WASM build."""
    if revision != "unreleased" and re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise ValueError("revision must be a lowercase full commit SHA or unreleased")

    version = tomllib.loads(read_regular(root, "Cargo.toml").decode("utf-8"))["workspace"]["package"]["version"]
    files = {}
    for name in RUNTIME_FILES:
        data = read_regular(root, f"runtime/{name}")
        if name.endswith(".wasm") and not data.startswith(b"\0asm\x01\0\0\0"):
            raise ValueError("invalid WebAssembly v1 module header")

        files[name] = hashlib.sha256(data).hexdigest()

    return {
        "schema_version": 1,
        "generator_version": version,
        "source_revision": revision,
        "source_fingerprint": source_fingerprint(root),
        "files": files,
    }


def main() -> int:
    """Write a deterministic manifest with an explicit local or released revision."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--revision", default=os.environ.get("SOURCEFIELD_SOURCE_COMMIT", "unreleased"))
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    destination = args.output or root / "runtime/runtime-manifest.json"
    manifest = create_manifest(root, args.revision)
    if destination.is_symlink():
        raise ValueError("manifest destination must not be a symlink")

    destination.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="ascii")
    print(f"Runtime manifest: {destination} ({manifest['source_fingerprint']})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
