#!/usr/bin/env python3
"""Install an exact, authenticated Sourcefield release without executing unverified code."""

from __future__ import annotations

import argparse
import json
import platform
import shutil
import stat
import subprocess
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

from runtime_manifest import RUNTIME_FILES
from sourcefield_tools.release import (
    MAX_ARCHIVE_BYTES,
    TARGETS,
    ReleaseLock,
    read_lock,
    release_metadata,
    verify_asset,
)

MAX_MEMBERS = 256


def extract_archive(archive: Path, destination: Path) -> None:
    """Extract bounded plain files, rejecting links, aliases and filesystem escapes."""
    if destination.exists() or destination.is_symlink():
        raise ValueError("extraction destination must not exist")

    with zipfile.ZipFile(archive) as bundle:
        members = bundle.infolist()
        if (
            len(members) > MAX_MEMBERS
            or sum(item.file_size for item in members) > MAX_ARCHIVE_BYTES
        ):
            raise ValueError("archive exceeds extraction resource limits")

        seen = set()
        for item in members:
            name = PurePosixPath(item.filename)
            mode = item.external_attr >> 16
            if (
                name.is_absolute()
                or not name.parts
                or any(part in {".", ".."} for part in name.parts)
                or "\\" in item.filename
                or ":" in item.filename
                or "\x00" in item.filename
                or str(name) != item.filename
                or item.is_dir()
                or stat.S_IFMT(mode) not in {0, stat.S_IFREG}
                or item.filename.casefold() in seen
            ):
                raise ValueError(f"unsafe archive member: {item.filename}")

            seen.add(item.filename.casefold())

        # Validate every member before creating anything. Extraction occurs in a private staging tree.
        destination.mkdir(parents=True)
        for item in members:
            target = destination / item.filename
            target.parent.mkdir(parents=True, exist_ok=True)
            with bundle.open(item) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output, length=1024 * 1024)

            target.chmod(0o755 if item.filename == "sourcefield" else 0o644)


def install(lock: ReleaseLock, destination: Path, target: str) -> Path:
    """Verify both assets before installing a complete CLI and browser pair."""
    if target not in set(TARGETS.values()):
        raise ValueError(f"unsupported native target: {target}")

    if destination.exists() or destination.is_symlink():
        raise ValueError(
            "installation destination must not exist; keep previous installations intact"
        )

    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["gh", "release", "verify", lock["release"], "--repo", lock["repository"]],
        check=True,
    )
    with tempfile.TemporaryDirectory(
        prefix=".sourcefield-install-", dir=destination.parent
    ) as temporary:
        staging = Path(temporary)
        for identity in (target, "browser"):
            asset = lock["assets"][identity]
            subprocess.run(
                [
                    "gh",
                    "release",
                    "download",
                    lock["release"],
                    "--repo",
                    lock["repository"],
                    "--pattern",
                    asset["name"],
                    "--dir",
                    str(staging),
                ],
                check=True,
            )
            verify_asset(lock, staging / asset["name"], identity)

        complete = staging / "complete"
        extract_archive(staging / lock["assets"][target]["name"], complete)
        extract_archive(
            staging / lock["assets"]["browser"]["name"], staging / "browser"
        )
        for tree, identity in ((complete, target), (staging / "browser", "browser")):
            metadata = json.loads(
                (tree / "release-metadata.json").read_text(encoding="utf-8")
            )

            expected = release_metadata(lock["source_commit"], lock["release"], identity)

            if metadata != expected:
                raise ValueError(
                    "authenticated bundle metadata does not match the consumer pin"
                )

        executable = complete / (
            "sourcefield.exe" if target.endswith("windows-msvc") else "sourcefield"
        )

        if not executable.is_file():
            raise ValueError("native archive is missing its executable")

        for required in (*RUNTIME_FILES, "runtime-manifest.json"):
            if not (staging / "browser/runtime" / required).is_file():
                raise ValueError(f"incomplete browser archive: {required}")

        version = subprocess.run(
            [str(executable), "--version"], check=True, capture_output=True, text=True
        ).stdout.strip()

        if version != f"sourcefield {lock['release'][1:]}":
            raise ValueError(
                "native executable version differs from the verified release"
            )

        shutil.move(str(staging / "browser/runtime"), complete / "runtime")
        (complete / "sourcefield.lock.json").write_text(
            json.dumps(lock, indent=2) + "\n", encoding="utf-8"
        )
        complete.rename(destination)

    return destination / (
        "sourcefield.exe" if target.endswith("windows-msvc") else "sourcefield"
    )


def main() -> int:
    """Install a pinned release; never resolve a moving latest version."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--target", choices=sorted(set(TARGETS.values())))
    args = parser.parse_args()
    target = args.target or TARGETS.get((platform.system(), platform.machine()))
    if target is None:
        parser.error(
            "unsupported platform; specify a supported --target or build from source"
        )

    executable = install(read_lock(args.lock), args.destination.absolute(), target)
    print(executable)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
