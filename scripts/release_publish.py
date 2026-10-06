#!/usr/bin/env python3
"""Publish a complete owner-authorized release, creating its version tag last."""

import argparse
import json
import os
import subprocess
from pathlib import Path

from bootstrap_release import digest_file, read_lock, verify_asset, verify_provenance


def existing_release(repository: str, release: str) -> dict | None:
    """Find an authenticated draft or publication without treating API failures as absence."""
    # Draft tags need not exist yet. The published-by-tag endpoint misses these drafts.
    # Project each page inside gh so only matching metadata reaches Python memory.
    response = subprocess.run(
        [
            "gh", "api", "--paginate", f"repos/{repository}/releases?per_page=100",
            "--jq", f".[] | select(.tag_name == {json.dumps(release)}) | "
            "{draft, target_commitish, assets: [.assets[] | {name, size, digest}]}",
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    matches = [json.loads(line) for line in response.stdout.splitlines() if line.strip()]
    if len(matches) > 1:
        raise ValueError("multiple releases claim the requested version")

    if not matches:
        return None

    result = matches[0]
    if (
        not isinstance(result, dict)
        or not isinstance(result.get("draft"), bool)
        or not isinstance(result.get("target_commitish"), str)
    ):
        raise ValueError("invalid release identity returned by GitHub")

    return result


def require_absent_tag(repository: str, release: str) -> None:
    """Refuse a preexisting tag before draft mutation or publication can ignore the target SHA."""
    existing_tag = subprocess.run(
        [
            "gh", "api", f"repos/{repository}/git/matching-refs/tags/{release}",
            "--jq", f".[] | select(.ref == {json.dumps('refs/tags/' + release)}) | .ref",
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    if existing_tag.stdout.strip():
        raise ValueError("release tag already exists before publication; refusing to reuse it")


def verify_inventory(directory: Path, existing: dict) -> None:
    """Require the complete remote asset set and exact uploaded bytes before publication."""
    expected = {
        path.name: (path.stat().st_size, f"sha256:{digest_file(path)}")
        for path in directory.iterdir() if path.is_file()
    }

    assets = existing.get("assets")
    if not isinstance(assets, list):
        raise ValueError("release asset inventory is missing")

    actual = {}
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get("name"), str):
            raise ValueError("invalid release asset inventory")

        name = asset["name"]
        if name in actual:
            raise ValueError("duplicate asset in release inventory")

        actual[name] = (asset.get("size"), asset.get("digest"))

    if actual != expected:
        raise ValueError("uploaded release assets differ from the complete verified candidate")


def verify_published(directory: Path, lock: dict) -> None:
    """Confirm the final immutable release and every uploaded asset without changing it."""
    existing = existing_release(lock["repository"], lock["release"])
    if existing is None or existing["draft"] or existing["target_commitish"] != lock["source_commit"]:
        raise ValueError("published release identity differs from the verified candidate")

    verify_inventory(directory, existing)
    resolved = subprocess.run(
        ["gh", "api", f"repos/{lock['repository']}/commits/tags/{lock['release']}", "--jq", ".sha"],
        check=True,
        capture_output=True,
        text=True,
    )

    if resolved.stdout.strip() != lock["source_commit"]:
        raise ValueError("release tag does not resolve to the attested source commit")

    subprocess.run(
        ["gh", "release", "verify", lock["release"], "--repo", lock["repository"]], check=True
    )
    for target, asset in lock["assets"].items():
        verify_asset(lock, directory / asset["name"], target)


def publish(directory: Path, repository: str, source_commit: str, release: str) -> None:
    """Verify all inputs before creating a draft, then publish its tag at the fixed source SHA."""
    lock = read_lock(directory / "sourcefield.lock.json")
    if (repository, source_commit, release) != (
        lock["repository"], lock["source_commit"], lock["release"],
    ):
        raise ValueError("release publication does not match the validated manifest")

    # Publication stays least-privileged: setup is operator-confirmed, then verified on the result.
    if os.environ.get("SOURCEFIELD_IMMUTABLE_RELEASES") != "true":
        raise ValueError(
            "enable immutable releases and confirm SOURCEFIELD_IMMUTABLE_RELEASES=true before publication"
        )

    for target, asset in lock["assets"].items():
        path = directory / asset["name"]
        if digest_file(path) != asset["sha256"]:
            raise ValueError(f"release archive changed after manifest generation: {target}")

        verify_provenance(lock, path)

    existing = existing_release(repository, release)
    if existing is not None:
        if existing["target_commitish"] != source_commit:
            raise ValueError("existing release targets a different source commit; refusing to retarget it")

        if not existing["draft"]:
            # A retry after publication or a failed final verification must never edit immutable assets.
            verify_published(directory, lock)

            return

    require_absent_tag(repository, release)
    if existing is None:
        # --draft defers tag creation; omit --verify-tag because that pending tag must not exist yet.
        subprocess.run(
            [
                "gh", "release", "create", release, "--repo", repository,
                "--target", source_commit, "--draft", "--title", release, "--generate-notes",
            ],
            check=True,
        )

    files = sorted(path for path in directory.iterdir() if path.is_file())
    subprocess.run(
        ["gh", "release", "upload", release, *map(str, files), "--repo", repository, "--clobber"],
        check=True,
    )
    uploaded = existing_release(repository, release)
    if uploaded is None or not uploaded["draft"] or uploaded["target_commitish"] != source_commit:
        raise ValueError("draft identity changed before publication")

    verify_inventory(directory, uploaded)
    require_absent_tag(repository, release)
    subprocess.run(
        ["gh", "release", "edit", release, "--repo", repository, "--draft=false"], check=True
    )
    verify_published(directory, lock)


def main() -> int:
    """Check an unused tag with read access, or publish a fully verified release candidate."""
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--directory", type=Path)
    mode.add_argument("--check-unpublished", action="store_true")
    parser.add_argument("--source-commit")
    parser.add_argument("--release", required=True)
    parser.add_argument("--repository", required=True)
    args = parser.parse_args()
    if args.check_unpublished:
        if args.source_commit is not None:
            parser.error("--source-commit is not accepted with --check-unpublished")

        # Read access cannot see every draft. The publisher rechecks drafts and tags with write access.
        require_absent_tag(args.repository, args.release)

        return 0

    if args.source_commit is None:
        parser.error("--source-commit is required for release publication")

    publish(args.directory, args.repository, args.source_commit, args.release)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
