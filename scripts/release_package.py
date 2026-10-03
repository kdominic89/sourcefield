#!/usr/bin/env python3
"""Build deterministic native/browser release ZIPs with explicit source identity."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import zipfile
from pathlib import Path

from package_source import digest_file, zip_info
from runtime_manifest import RUNTIME_FILES


def package(
    root: Path,
    output: Path,
    target: str,
    source_commit: str,
    release: str,
    binary: Path | None = None,
) -> Path:
    """Package a single built target or complete browser runtime in bounded memory."""
    if not re.fullmatch(r"[0-9a-f]{40}", source_commit):
        raise ValueError("release source must be a full commit SHA")

    entries = []
    if target == "browser":
        runtime = root / "runtime"
        for required in (*RUNTIME_FILES, "runtime-manifest.json"):
            if not (runtime / required).is_file():
                raise ValueError(f"incomplete browser runtime: {required}")

        manifest = json.loads(
            (runtime / "runtime-manifest.json").read_text(encoding="utf-8")
        )

        if (
            manifest.get("schema_version") != 1
            or manifest.get("source_revision") != source_commit
            or manifest.get("generator_version") != release[1:]
        ):
            raise ValueError(
                "browser runtime provenance differs from the release identity"
            )

        if manifest.get("files") != {
            name: digest_file(runtime / name) for name in RUNTIME_FILES
        }:
            raise ValueError(
                "browser runtime files differ from their provenance manifest"
            )

        # Package the runtime contract only, never incidental developer or wasm-pack metadata.
        entries = [
            (runtime / name, f"runtime/{name}")
            for name in (*RUNTIME_FILES, "runtime-manifest.json")
        ]

    else:
        if binary is None or not binary.is_file():
            raise ValueError("native release requires its compiled executable")

        entries = [
            (
                binary,
                "sourcefield.exe" if target.endswith("windows-msvc") else "sourcefield",
            )
        ]

    if any(path.is_symlink() for path, _ in entries):
        raise ValueError("release assets must not contain symlinks")

    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"sourcefield-{target}.zip"
    metadata = {
        "schema_version": 1,
        "source_commit": source_commit,
        "release": release,
        "target": target,
    }

    with zipfile.ZipFile(archive, "w") as bundle:
        for path, name in sorted(entries, key=lambda entry: entry[1]):
            with (
                path.open("rb") as source,
                bundle.open(zip_info(name, name == "sourcefield"), "w") as destination,
            ):
                shutil.copyfileobj(source, destination, length=1024 * 1024)

        bundle.writestr(
            zip_info("release-metadata.json"),
            json.dumps(metadata, sort_keys=True) + "\n",
        )

    archive.with_suffix(".zip.sha256").write_text(
        f"{digest_file(archive)}  {archive.name}\n", encoding="utf-8"
    )

    return archive


def main() -> int:
    """Package a prebuilt target; packaging never installs or executes dependencies."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--release", required=True)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path, default=Path("dist"))
    args = parser.parse_args()
    print(
        package(
            Path(__file__).resolve().parents[1],
            args.output,
            args.target,
            args.source_commit,
            args.release,
            args.binary,
        )
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
