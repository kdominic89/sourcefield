#!/usr/bin/env python3
"""Validate an installed Chromium setuid sandbox helper without changing host policy."""

from __future__ import annotations

import argparse
import os
import stat
import sys
from pathlib import Path


def verify_helper(helper: Path) -> None:
    """Reject missing or unsafe privileged helpers before Chromium uses their path."""

    try:
        metadata = helper.lstat()
    except OSError as error:
        raise ValueError(f"cannot inspect sandbox helper {helper}: {error.strerror}") from error

    # The helper runs with root privileges; validate it instead of disabling the browser sandbox.
    if stat.S_ISLNK(metadata.st_mode):
        raise ValueError("sandbox helper must not be a symlink")

    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError("sandbox helper must be a regular file")

    if metadata.st_uid != 0:
        raise ValueError("sandbox helper must be owned by root")

    if not metadata.st_mode & stat.S_ISUID:
        raise ValueError("sandbox helper must have the setuid bit")

    if metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
        raise ValueError("sandbox helper must not be group or world writable")

    executable_bits = stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    if not metadata.st_mode & executable_bits or not os.access(helper, os.X_OK):
        raise ValueError("sandbox helper must be executable by the current user")


def main(argv: list[str] | None = None) -> int:
    """Check one helper and report concise failures without modifying host state."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("helper", type=Path, help="installed Chromium setuid sandbox helper")
    args = parser.parse_args(argv)
    try:
        verify_helper(args.helper)
    except ValueError as error:
        print(f"Browser sandbox helper rejected: {error}", file=sys.stderr)

        return 1

    print(f"Browser sandbox helper verified: {args.helper}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
