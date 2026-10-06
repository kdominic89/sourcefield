"""Validated release identity and authentication, independent of install or publish commands."""

import json
import re
import subprocess
from pathlib import Path
from typing import Literal, TypedDict, cast

from sourcefield_tools.artifacts import digest_file

TARGETS: dict[tuple[str, str], str] = {
    ("Darwin", "arm64"): "aarch64-apple-darwin",
    ("Darwin", "x86_64"): "x86_64-apple-darwin",
    ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
    ("Linux", "aarch64"): "aarch64-unknown-linux-gnu",
    ("Windows", "AMD64"): "x86_64-pc-windows-msvc",
}

MAX_ARCHIVE_BYTES = 256 * 1024 * 1024


class AssetIdentity(TypedDict):
    """Identify an exact named archive through its lowercase SHA-256 digest."""

    name: str
    sha256: str


class ReleaseLock(TypedDict):
    """Describe the complete runtime-validated version 1 consumer release pin."""

    schema_version: Literal[1]
    repository: str
    source_commit: str
    release: str
    workflow: str
    assets: dict[str, AssetIdentity]


class ReleaseMetadata(TypedDict):
    """Bind a packaged target to the source and version declared in the consumer pin."""

    schema_version: Literal[1]
    source_commit: str
    release: str
    target: str


def read_lock(path: Path) -> ReleaseLock:
    """Validate the complete authored release identity before any network access."""
    if path.stat().st_size > 64 * 1024:
        raise ValueError("release lock exceeds 64 KiB")

    value = json.loads(path.read_text(encoding="utf-8"))
    expected = {
        "schema_version",
        "repository",
        "source_commit",
        "release",
        "workflow",
        "assets",
    }

    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError("lock must contain exactly the documented version 1 fields")

    if value["schema_version"] != 1 or isinstance(value["schema_version"], bool):
        raise ValueError("unsupported lock schema_version")

    if value["repository"] != "kdominic89/sourcefield":
        raise ValueError("unexpected Sourcefield repository")

    if not isinstance(value["source_commit"], str) or not re.fullmatch(
        r"[0-9a-f]{40}", value["source_commit"]
    ):
        raise ValueError("source_commit must be a full lowercase Git commit SHA")

    if not isinstance(value["release"], str) or not re.fullmatch(
        r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", value["release"]
    ):
        raise ValueError("release must be an exact version tag")

    if value["workflow"] != ".github/workflows/release.yml":
        raise ValueError("unexpected release signer workflow")

    assets = value["assets"]
    if not isinstance(assets, dict) or set(assets) != set(TARGETS.values()) | {
        "browser"
    }:
        raise ValueError(
            "lock must identify every supported native asset and browser asset"
        )

    for target, asset in assets.items():
        if not isinstance(asset, dict) or set(asset) != {"name", "sha256"}:
            raise ValueError(f"invalid asset entry: {target}")

        if (
            asset["name"] != f"sourcefield-{target}.zip"
            or not isinstance(asset["sha256"], str)
            or not re.fullmatch(r"[0-9a-f]{64}", asset["sha256"])
        ):
            raise ValueError(f"invalid asset identity: {target}")

    # Narrow only after validating decoded JSON; TypedDict itself performs no runtime checks.
    return cast(ReleaseLock, value)


def release_metadata(source_commit: str, release: str, target: str) -> ReleaseMetadata:
    """Construct the shared version 1 bundle identity used by packaging and verification."""

    return {
        "schema_version": 1,
        "source_commit": source_commit,
        "release": release,
        "target": target,
    }


def verify_asset(lock: ReleaseLock, path: Path, target: str) -> None:
    """Bind downloaded bytes to the release, source commit and specific signer workflow."""
    if (
        path.stat().st_size > MAX_ARCHIVE_BYTES
        or digest_file(path) != lock["assets"][target]["sha256"]
    ):
        raise ValueError(f"asset checksum or size mismatch: {target}")

    subprocess.run(
        [
            "gh",
            "release",
            "verify-asset",
            lock["release"],
            str(path),
            "--repo",
            lock["repository"],
        ],
        check=True,
    )
    verify_provenance(lock, path)


def verify_provenance(lock: ReleaseLock, path: Path) -> None:
    """Require build provenance from the pinned release workflow and source commit."""
    subprocess.run(
        [
            "gh",
            "attestation",
            "verify",
            str(path),
            "--repo",
            lock["repository"],
            "--signer-workflow",
            f"{lock['repository']}/{lock['workflow']}",
            "--source-digest",
            lock["source_commit"],
            "--signer-digest",
            lock["source_commit"],
        ],
        check=True,
    )
