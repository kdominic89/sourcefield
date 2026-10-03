#!/usr/bin/env python3
"""Forward explicit preview paths to the shared generator without implicit README writes."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def main() -> int:
    """Generate a source-build preview inside an explicitly selected consumer root."""
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--config", default="config/profile.toml")
    parser.add_argument("--snapshot", default="config/offline-snapshot.json")
    parser.add_argument("--assets", default="assets")
    parser.add_argument("--docs", default="docs")
    parser.add_argument("--readme", action="append", default=[])
    parser.add_argument("--locked", action="store_true")
    args = parser.parse_args()
    command = [
        "cargo",
        "run",
        "--locked",
        "--manifest-path",
        str(root / "Cargo.toml"),
        "-p",
        "sourcefield-cli",
        "--",
        "generate",
        "--offline",
        "--root",
        str(args.root.resolve()),
        "--config",
        args.config,
        "--fallback-snapshot",
        args.snapshot,
        "--assets",
        args.assets,
        "--docs",
        args.docs,
        "--runtime",
        str(root / "runtime"),
    ]

    for path in args.readme:
        command.extend(["--readme", path])

    if args.locked:
        command.append("--locked")

    return subprocess.run(command, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
