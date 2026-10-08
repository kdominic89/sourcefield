#!/usr/bin/env python3
"""Generate a validated consumer candidate in an isolated, explicitly selected tree."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

from sourcefield_tools.consumer import relative_path


def isolated_checkout(source: Path, destination: Path) -> None:
    """Preserve commit/blob provenance without copying credentials or sharing Git objects."""
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=source,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()

    origin = subprocess.run(
        ["git", "remote", "get-url", "origin"],
        cwd=source,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()

    # An empty template prevents inherited hooks. A transport clone avoids shared object stores
    # and source config, which may contain credentials unavailable to generated output.
    with tempfile.TemporaryDirectory(prefix="sourcefield-git-template-") as template:
        subprocess.run(
            [
                "git",
                "clone",
                "--quiet",
                "--no-local",
                "--depth=1",
                "--single-branch",
                "--no-checkout",
                f"--template={template}",
                "--",
                str(source),
                str(destination),
            ],
            check=True,
        )

    subprocess.run(
        ["git", "checkout", "--quiet", "--detach", revision],
        cwd=destination,
        check=True,
    )
    subprocess.run(
        ["git", "remote", "set-url", "origin", origin], cwd=destination, check=True
    )


def remove_git_metadata(metadata: Path) -> None:
    """Remove the private clone, including Windows read-only object files."""

    def retry_writable(operation, name, error):
        """Retry only permission failures inside this independently copied object store."""
        if not isinstance(error[1], PermissionError):
            raise error[1]

        Path(name).chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        operation(name)

    shutil.rmtree(metadata, onerror=retry_writable)


def installed_executable(installation: Path) -> Path:
    """Select the single supported native executable without relying on the host suffix."""
    available = [
        path
        for path in (installation / "sourcefield", installation / "sourcefield.exe")
        if path.is_file()
    ]

    if len(available) != 1:
        raise ValueError(
            "installation must contain exactly one native Sourcefield executable"
        )

    return available[0]


def candidate(
    source: Path,
    destination: Path,
    installation: Path,
    config: str,
    readmes: list[str],
    offline: bool,
    locked: bool,
    private_counts: bool = False,
) -> None:
    """Copy public inputs and generate with optional additional private aggregate intent.

    A caller selection needs a nonempty PROFILE_TOKEN; otherwise it warns and continues
    with public collection. Native configuration/environment opt-ins remain independent.
    """
    if destination.exists() or destination.is_symlink():
        raise ValueError("candidate destination must not exist")

    config = relative_path(config)
    readmes = [relative_path(value) for value in readmes]
    if len(set(readmes)) != len(readmes):
        raise ValueError("README destinations must be distinct")

    tracked = subprocess.run(
        ["git", "ls-files", "-z"], cwd=source, check=True, capture_output=True
    ).stdout

    paths = [
        relative_path(value) for value in tracked.decode("utf-8").split("\0") if value
    ]

    for name in paths:
        path = source / name
        if path.is_symlink() or any(
            (source / parent).is_symlink() for parent in Path(name).parents
        ):
            raise ValueError(f"consumer input must not be a symlink: {name}")

    try:
        isolated_checkout(source, destination)
        for name in paths:
            output = destination / name
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / name, output)

        executable = installed_executable(installation)
        # Consumer updates must use their published observations; the CLI's first-author
        # seed can otherwise replace captured packages with an empty offline preview.
        command = [
            str(executable),
            "generate",
            "--root",
            str(destination),
            "--config",
            config,
            "--fallback-snapshot",
            "assets/source-snapshot.json",
            "--assets",
            "assets",
            "--docs",
            "docs",
            "--runtime",
            str(installation / "runtime"),
        ]

        for readme in readmes:
            command.extend(["--readme", readme])

        if offline:
            command.append("--offline")
        else:
            command.append("--strict-live")

        if locked:
            command.append("--locked")

        if private_counts:
            # The old caller treated missing optional credentials as a public-only run.
            # Passing the private flag without them would instead fail strict validation.
            if os.environ.get("PROFILE_TOKEN", "").strip():
                command.append("--private-counts")
            else:
                print(
                    "::warning::Private aggregate requested, but PROFILE_TOKEN is not configured. "
                    "Continuing without it.",
                    file=sys.stderr,
                )

        subprocess.run(command, check=True)
        subprocess.run(
            [
                str(executable),
                "validate",
                "--root",
                str(destination),
                "--config",
                config,
                "--state",
                "assets/profile-state.json",
                "--assets",
                "assets",
            ],
            check=True,
        )
    finally:
        # Provenance is required only during generation, never in the uploaded public artifact.
        metadata = destination / ".git"
        if metadata.is_dir():
            remove_git_metadata(metadata)


def main() -> int:
    """Accept typed arguments rather than interpolating consumer values into shell commands."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--installation", type=Path, required=True)
    parser.add_argument("--config", default="config/profile.toml")
    parser.add_argument("--readmes", default='["README.md"]')
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--locked", action="store_true")
    parser.add_argument(
        "--private-counts",
        action="store_true",
        help="Request an owned-private aggregate when PROFILE_TOKEN is available",
    )
    args = parser.parse_args()
    readmes = json.loads(args.readmes)
    if not isinstance(readmes, list) or not all(
        isinstance(value, str) for value in readmes
    ):
        parser.error("--readmes must be a JSON array of relative file paths")

    candidate(
        args.source.resolve(),
        args.destination.absolute(),
        args.installation.resolve(),
        args.config,
        readmes,
        args.offline,
        args.locked,
        args.private_counts,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
