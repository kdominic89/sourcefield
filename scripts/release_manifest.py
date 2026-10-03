#!/usr/bin/env python3
"""Require the complete release matrix and emit the consumer lock plus checksums."""

import argparse
import json
import tomllib
import zipfile
from pathlib import Path

from bootstrap_release import TARGETS, digest_file, read_lock


def manifest(directory: Path, source_commit: str, release: str) -> Path:
    """Reject mixed-source or incomplete assets before constructing a consumable lock."""
    assets = {}
    for target in sorted(set(TARGETS.values()) | {"browser"}):
        archive = directory / f"sourcefield-{target}.zip"
        with zipfile.ZipFile(archive) as bundle:
            metadata = json.loads(bundle.read("release-metadata.json"))
            if metadata != {
                "schema_version": 1,
                "source_commit": source_commit,
                "release": release,
                "target": target,
            }:
                raise ValueError(f"release metadata mismatch: {target}")

            if bundle.testzip() is not None:
                raise ValueError(f"corrupt release archive: {target}")

        assets[target] = {"name": archive.name, "sha256": digest_file(archive)}

    lock = {
        "schema_version": 1,
        "repository": "kdominic89/sourcefield",
        "source_commit": source_commit,
        "release": release,
        "workflow": ".github/workflows/release.yml",
        "assets": assets,
    }

    destination = directory / "sourcefield.lock.json"
    destination.write_text(
        json.dumps(lock, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    read_lock(destination)
    checksums = [f"{item['sha256']}  {item['name']}\n" for item in assets.values()]
    checksums.append(f"{digest_file(destination)}  {destination.name}\n")
    (directory / "SHA256SUMS").write_text("".join(checksums), encoding="utf-8")

    return destination


def main() -> int:
    """Produce release metadata only after every matrix asset has arrived."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--release", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))[
        "workspace"
    ]["package"]["version"]

    if args.release != f"v{version}":
        parser.error("release tag must match the workspace package version")

    print(manifest(args.directory, args.source_commit, args.release))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
