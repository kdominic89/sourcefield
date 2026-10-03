#!/usr/bin/env python3
"""Check or prepare the literal reusable-workflow mirror of a release lock."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

from bootstrap_release import read_lock

REFERENCE = re.compile(
    r"(?m)^(\s*uses:\s*kdominic89/sourcefield/\.github/workflows/generate\.yml@)([^\s#]+)(\s*(?:#.*)?)$"
)


def check_pin(lock_path: Path, workflow: Path, own_commit: str | None = None) -> None:
    """Reject missing, duplicated or divergent workflow/source identities."""
    lock = read_lock(lock_path)
    matches = list(REFERENCE.finditer(workflow.read_text(encoding="utf-8")))
    if len(matches) != 1 or matches[0].group(2) != lock["source_commit"]:
        raise ValueError("exactly one workflow pin must match sourcefield.lock.json")

    if own_commit is not None and own_commit != lock["source_commit"]:
        raise ValueError(
            "executing reusable workflow revision differs from the consumer lock"
        )


def prepare_update(lock_path: Path, workflow: Path, destination: Path) -> None:
    """Prepare both files in a new review directory; do not alter the active consumer."""
    lock = read_lock(lock_path)
    original = workflow.read_text(encoding="utf-8")
    matches = list(REFERENCE.finditer(original))
    if len(matches) != 1 or destination.exists():
        raise ValueError(
            "update requires one existing workflow reference and a new output directory"
        )

    updated = REFERENCE.sub(
        lambda match: match.group(1) + lock["source_commit"] + match.group(3), original
    )

    destination.mkdir(parents=True)
    (destination / "sourcefield.lock.json").write_bytes(lock_path.read_bytes())
    (destination / workflow.name).write_text(updated, encoding="utf-8")
    check_pin(destination / "sourcefield.lock.json", destination / workflow.name)


def main() -> int:
    """Validate pins or produce a reviewable paired update without Git mutations."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, required=True)
    parser.add_argument("--workflow", type=Path, required=True)
    parser.add_argument("--own-commit")
    parser.add_argument("--prepare", type=Path)
    args = parser.parse_args()
    if args.prepare is not None:
        prepare_update(args.lock, args.workflow, args.prepare)
    else:
        check_pin(args.lock, args.workflow, args.own_commit)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
