#!/usr/bin/env python3
"""Create a deterministic, source-only ZIP using an explicit path/type allowlist."""

from __future__ import annotations

import argparse
import os
import shutil
import tempfile
import zipfile
from pathlib import Path

from sourcefield_tools.artifacts import digest_file, zip_info

ROOT_FILES = {
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rustfmt.toml",
    ".editorconfig",
    ".gitignore",
    ".gitattributes",
    "README.md",
    "ARCHITECTURE.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "LICENSE",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SOURCES.md",
    "MAINTAINING.md",
    ".cargo/config.toml",
    ".github/dependabot.yml",
    ".github/pull_request_template.md",
    ".github/ISSUE_TEMPLATE/bug_report.md",
    ".github/ISSUE_TEMPLATE/feature_request.md",
    ".github/ISSUE_TEMPLATE/config.yml",
    "tools/wasm-pack-version.txt",
    "tools/browser/package.json",
    "tools/browser/package-lock.json",
    "crates/sourcefield-io/README.md",
    "crates/sourcefield-workspace/README.md",
}

TREE_TYPES = {
    "crates": {".rs", ".toml"},
    "config": {".toml", ".json"},
    "scripts": {".py", ".sh", ".mjs"},
    "tests": {".py", ".js", ".mjs", ".json", ".html", ".toml"},
    ".github/workflows": {".yml", ".yaml"},
    "assets": {".svg", ".json"},
    "docs": {
        ".md",
        ".html",
        ".css",
        ".js",
        ".json",
        ".svg",
        ".webmanifest",
        ".template",
    },
    "runtime": {".html", ".css", ".js", ".json", ".svg", ".webmanifest"},
    "examples": {".toml", ".json", ".md"},
}

EXCLUDED = {
    ".git",
    "target",
    "pkg",
    "node_modules",
    "__pycache__",
    ".fastembed_cache",
    "dist",
}


def source_files(root: Path) -> list[Path]:
    """Walk only approved trees, prune generated directories, and reject symlinks."""
    selected = []
    for name in sorted(ROOT_FILES):
        path = root / name
        if path.is_symlink() or any(
            parent.is_symlink() for parent in path.parents if parent != root.parent
        ):
            raise ValueError(f"symlink is not a source artifact: {name}")

        if path.is_file():
            selected.append(path)

    for name, extensions in TREE_TYPES.items():
        base = root / name
        if base.is_symlink():
            raise ValueError(f"symlink is not a source tree: {name}")

        if not base.exists():
            continue

        for directory, names, files in os.walk(base, followlinks=False):
            for child in names:
                if (Path(directory) / child).is_symlink():
                    raise ValueError(f"symlink is not a source directory: {child}")

            names[:] = sorted(
                child
                for child in names
                if child not in EXCLUDED and not child.startswith(".")
            )

            for child in sorted(files):
                # Runtime provenance is built from a particular revision, not authored source.
                if name == "runtime" and child == "runtime-manifest.json":
                    continue

                path = Path(directory) / child
                if path.is_symlink():
                    raise ValueError(
                        f"symlink is not a source file: {path.relative_to(root)}"
                    )

                if not child.startswith(".") and path.suffix in extensions:
                    selected.append(path)

    return sorted(selected, key=lambda path: path.relative_to(root).as_posix())


def package(root: Path, output: Path) -> str:
    """Atomically publish a normalized archive and sidecar after integrity checks."""
    root = root.resolve()
    output = output.absolute()
    if (
        output.is_symlink()
        or output.with_suffix(output.suffix + ".sha256").is_symlink()
    ):
        raise ValueError("archive destinations must not be symlinks")

    paths = source_files(root)
    if output in paths:
        raise ValueError("archive must not overwrite a source file")

    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="sourcefield-package-", dir=output.parent
    ) as temporary:
        archive = Path(temporary) / "source.zip"
        manifest = []
        with zipfile.ZipFile(archive, "w") as handle:
            for path in paths:
                relative = path.relative_to(root).as_posix()
                manifest.append(f"{digest_file(path)}  {relative}\n")
                info = zip_info(f"sourcefield/{relative}", path.suffix == ".sh")
                with path.open("rb") as source, handle.open(info, "w") as destination:
                    shutil.copyfileobj(source, destination, length=1024 * 1024)

            handle.writestr(
                zip_info("sourcefield/SHA256SUMS"), "".join(manifest).encode("utf-8")
            )

        with zipfile.ZipFile(archive) as handle:
            if handle.testzip() is not None:
                raise ValueError("archive integrity check failed")

        checksum = digest_file(archive)
        sidecar = Path(temporary) / "checksum"
        sidecar.write_text(f"{checksum}  {output.name}\n", encoding="utf-8")
        archive.replace(output)
        sidecar.replace(output.with_suffix(output.suffix + ".sha256"))

    return checksum


def main() -> int:
    """Package the current source tree without modifying or cleaning source files."""
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "output", nargs="?", type=Path, default=root / "dist/sourcefield-source.zip"
    )
    args = parser.parse_args()
    checksum = package(root, args.output)
    print(f"Package: {args.output}\nSHA256: {checksum}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
