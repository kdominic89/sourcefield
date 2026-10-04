#!/usr/bin/env python3
"""Render an exact-executable AppArmor user namespace policy for browser CI."""

from __future__ import annotations

import argparse
import os
import stat
import sys
from pathlib import Path


def render_policy(executable: str) -> str:
    """Validate one canonical executable path and render its literal policy attachment."""

    unsafe = "*?[]{}@$\"'\\"
    if any(ord(character) < 32 or ord(character) > 126 or character in unsafe for character in executable):
        raise ValueError("browser executable contains unsafe policy characters")

    path = Path(executable)
    if not path.is_absolute():
        raise ValueError("browser executable path must be absolute")

    try:
        metadata = path.lstat()
    except OSError as error:
        raise ValueError(f"cannot inspect browser executable: {error.strerror}") from error

    if stat.S_ISLNK(metadata.st_mode):
        raise ValueError("browser executable must not be a symlink")

    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError("browser executable must be a regular file")

    try:
        canonical = path.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        raise ValueError("cannot resolve browser executable path") from error

    if str(canonical) != executable:
        raise ValueError("browser executable path must be canonical without symlinks")

    executable_bits = stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
    if not metadata.st_mode & executable_bits or not os.access(path, os.X_OK):
        raise ValueError("browser executable must be executable by the current user")

    # An exact executable attachment prevents wildcard namespace grants to neighboring binaries.
    return (
        "abi <abi/4.0>,\ninclude <tunables/global>\n\n"
        f'profile sourcefield-playwright "{executable}" flags=(unconfined) {{\n'
        "  userns,\n}\n"
    )


def main(argv: list[str] | None = None) -> int:
    """Print only validated policy text, or a concise error without changing host policy."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", help="absolute canonical path to the bundled Chromium executable")
    args = parser.parse_args(argv)
    try:
        policy = render_policy(args.executable)
    except ValueError as error:
        print(f"Browser sandbox policy rejected: {error}", file=sys.stderr)

        return 1

    print(policy, end="")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
