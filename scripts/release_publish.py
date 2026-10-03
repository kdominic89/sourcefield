#!/usr/bin/env python3
"""Publish a complete draft after confirmed setup, then verify release immutability."""

import argparse
import json
import os
import subprocess
from pathlib import Path

from bootstrap_release import digest_file, read_lock, verify_asset, verify_provenance


def publish(directory: Path, repository: str, source_commit: str, release: str) -> None:
    """Fail closed on identity/settings drift and keep failed uploads as a draft."""
    lock = read_lock(directory / "sourcefield.lock.json")
    if (repository, source_commit, release) != (
        lock["repository"],
        lock["source_commit"],
        lock["release"],
    ):
        raise ValueError("release publication does not match the validated manifest")

    # GitHub's settings endpoint needs admin-read access. Keep publication least-privileged:
    # require an operator-confirmed prerequisite and verify the actual release afterward.
    if os.environ.get("SOURCEFIELD_IMMUTABLE_RELEASES") != "true":
        raise ValueError(
            "enable immutable releases and confirm SOURCEFIELD_IMMUTABLE_RELEASES=true before publication"
        )

    resolved = subprocess.run(
        ["gh", "api", f"repos/{repository}/commits/{release}", "--jq", ".sha"],
        check=True,
        capture_output=True,
        text=True,
    )

    if resolved.stdout.strip() != source_commit:
        raise ValueError("release tag does not resolve to the attested source commit")

    for target, asset in lock["assets"].items():
        path = directory / asset["name"]
        if digest_file(path) != asset["sha256"]:
            raise ValueError(
                f"release archive changed after manifest generation: {target}"
            )

        verify_provenance(lock, path)

    existing = subprocess.run(
        [
            "gh",
            "release",
            "view",
            release,
            "--repo",
            repository,
            "--json",
            "isDraft,targetCommitish",
        ],
        check=False,
        capture_output=True,
        text=True,
    )

    if existing.returncode == 0:
        if json.loads(existing.stdout).get("isDraft") is not True:
            raise ValueError("refusing to modify a published release")
    else:
        subprocess.run(
            [
                "gh",
                "release",
                "create",
                release,
                "--repo",
                repository,
                "--verify-tag",
                "--target",
                source_commit,
                "--draft",
                "--title",
                release,
                "--generate-notes",
            ],
            check=True,
        )

    files = sorted(path for path in directory.iterdir() if path.is_file())
    subprocess.run(
        [
            "gh",
            "release",
            "upload",
            release,
            *map(str, files),
            "--repo",
            repository,
            "--clobber",
        ],
        check=True,
    )
    subprocess.run(
        ["gh", "release", "edit", release, "--repo", repository, "--draft=false"],
        check=True,
    )
    subprocess.run(
        ["gh", "release", "verify", release, "--repo", repository], check=True
    )
    for target, asset in lock["assets"].items():
        verify_asset(lock, directory / asset["name"], target)


def main() -> int:
    """Run only in an explicitly authorized release environment with publish permissions."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--release", required=True)
    args = parser.parse_args()
    publish(args.directory, args.repository, args.source_commit, args.release)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
