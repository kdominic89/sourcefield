#!/usr/bin/env python3
"""Apply only generator-owned candidate files after checking consumer revision preconditions."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from pathlib import Path

from sourcefield_tools.artifacts import digest_file
from sourcefield_tools.consumer import relative_path

MANIFEST = ".sourcefield-owned.json"


def read_ownership(root: Path) -> dict[str, dict[str, str]]:
    """Read the explicit generated-file inventory without allowing path escapes."""
    path = root / MANIFEST
    if not path.exists():
        return {"files": {}, "authored_files": {}}

    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema_version") != 1 or not isinstance(value.get("files"), dict):
        raise ValueError("unsupported generated ownership manifest")

    authored = value.get("authored_files", {})
    if not isinstance(authored, dict) or set(authored) & set(value["files"]):
        raise ValueError("authored and generated candidate ownership must be distinct")

    for name, digest in {**value["files"], **authored}.items():
        relative_path(name)
        if not isinstance(digest, str) or len(digest) != 64:
            raise ValueError("invalid generated ownership digest")

    return {"files": value["files"], "authored_files": authored}


def apply_candidate(
    checkout: Path, candidate: Path, expected_commit: str, branch: str
) -> list[str]:
    """Stage a validated candidate, refusing stale HEAD and modified or unsafe artifacts."""
    actual = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=checkout,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()

    remote = subprocess.run(
        ["git", "ls-remote", "origin", f"refs/heads/{branch}"],
        cwd=checkout,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.split()

    if actual != expected_commit or not remote or remote[0] != expected_commit:
        raise ValueError(
            "consumer revision changed after candidate generation; regenerate before publication"
        )

    previous = read_ownership(checkout)["files"]
    inventory = read_ownership(candidate)
    current = {**inventory["files"], **inventory["authored_files"]}
    if not current:
        raise ValueError("candidate has no generated ownership inventory")

    for name, digest in current.items():
        path = candidate / name
        if (
            path.is_symlink()
            or not path.is_file()
            or any((candidate / parent).is_symlink() for parent in Path(name).parents)
        ):
            raise ValueError(f"unsafe or missing candidate file: {name}")

        actual_digest = digest_file(path)
        if actual_digest != digest:
            raise ValueError(f"candidate changed after validation: {name}")

    for name in sorted(set(previous) | set(current)):
        destination = checkout / name
        if destination.is_symlink() or any(
            (checkout / parent).is_symlink() for parent in Path(name).parents
        ):
            raise ValueError(f"unsafe checkout destination: {name}")

    for name in sorted(set(previous) - set(current)):
        (checkout / name).unlink(missing_ok=True)

    for name in sorted(current):
        destination = checkout / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(candidate / name, destination)

    shutil.copyfile(candidate / MANIFEST, checkout / MANIFEST)
    tracked = []
    for name in sorted(set(previous) | set(current) | {MANIFEST}):
        ignored = subprocess.run(
            ["git", "check-ignore", "-q", "--", name], cwd=checkout, check=False
        )

        if ignored.returncode == 1:
            tracked.append(name)
        elif ignored.returncode != 0:
            raise ValueError(f"cannot establish ignored-file policy: {name}")

    subprocess.run(["git", "add", "--all", "--", *tracked], cwd=checkout, check=True)

    return tracked


def main() -> int:
    """Prepare consumer-owned publication; never commit, push or deploy implicitly."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkout", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--expected-commit", required=True)
    parser.add_argument("--branch", required=True)
    args = parser.parse_args()
    paths = apply_candidate(
        args.checkout.resolve(),
        args.candidate.resolve(),
        args.expected_commit,
        args.branch,
    )

    print(f"Staged {len(paths)} generator-owned paths")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
