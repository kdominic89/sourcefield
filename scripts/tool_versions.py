#!/usr/bin/env python3
"""Check or explicitly update the content-pinned official Binaryen release."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import sys
import tempfile
from urllib.error import URLError

from sourcefield_tools import binaryen

ROOT = Path(__file__).resolve().parents[1]
PIN = binaryen.PIN


def release_assets(document: dict, expected_version: int | None = None) -> tuple[int, dict]:
    """Validate official stable release identity and all required platform archive metadata."""
    tag = document.get("tag_name")
    if not isinstance(tag, str) or re.fullmatch(r"version_[1-9][0-9]{0,5}", tag) is None:
        raise ValueError("Binaryen release tag must be a canonical stable version_N")

    version = int(tag.removeprefix("version_"))
    if expected_version is not None and version != expected_version:
        raise ValueError("Binaryen release identity does not match the requested version")

    if document.get("prerelease") is not False or document.get("draft") is not False:
        raise ValueError("Binaryen release must be published and stable")

    if document.get("html_url") != f"https://github.com/WebAssembly/binaryen/releases/tag/{tag}":
        raise ValueError("Binaryen release URL does not identify the official repository")

    entries = document.get("assets")
    if not isinstance(entries, list) or not 1 <= len(entries) <= 128:
        raise ValueError("Binaryen release must have a bounded asset inventory")

    inventory = {}
    for item in entries:
        if not isinstance(item, dict) or not isinstance(item.get("name"), str):
            raise ValueError("Binaryen release assets must be named objects")

        name = item["name"]
        if name in inventory:
            raise ValueError("Binaryen release contains duplicate asset names")

        inventory[name] = item

    hosts = {}
    for host in binaryen.HOSTS:
        name = binaryen.archive_name(version, host)
        item = inventory.get(name)
        if not isinstance(item, dict) or item.get("state") != "uploaded":
            raise ValueError(f"Binaryen release is missing an uploaded archive: {name}")

        size = binaryen.positive_integer(item.get("size"), binaryen.MAX_ARCHIVE_BYTES, "Binaryen archive size")
        digest = item.get("digest")
        if not isinstance(digest, str) or not digest.startswith("sha256:"):
            raise ValueError("Binaryen release asset must provide a SHA-256 digest")

        digest = binaryen.digest_value(digest.removeprefix("sha256:"))
        if item.get("browser_download_url") != f"{binaryen.DOWNLOAD}/{tag}/{name}":
            raise ValueError("Binaryen release asset URL does not match the official version and host")

        hosts[host] = {"name": name, "size": size, "sha256": digest}

    return version, hosts


def check_current_assets(current: dict, released: dict) -> None:
    """Detect removed or replaced content even when the release tag remains unchanged."""
    _, assets = release_assets(released, current["version"])
    for host, record in current["hosts"].items():
        if assets[host] != record["archive"]:
            raise ValueError("the pinned Binaryen release content changed; manual security review is required")


def candidate_manifest(current: dict, latest: dict) -> dict:
    """Authenticate every host archive and derive only its required file identities."""
    version, archives = release_assets(latest)
    if version <= current["version"]:
        raise ValueError("Binaryen updates require a newer release; refusing downgrade or replacement")

    candidate = {
        "schema_version": 1, "version": version,
        "optimizer_flags": list(current["optimizer_flags"]), "hosts": {},
    }

    with tempfile.TemporaryDirectory(prefix="sourcefield-binaryen-update-") as temporary:
        directory = Path(temporary)
        for host, archive in archives.items():
            record = {
                "archive": archive,
                "files": [{"path": name, "size": 1, "sha256": "0" * 64} for name in binaryen.required_files(host)],
            }

            archive_path = directory / archive["name"]
            binaryen.download_archive(version, record, archive_path)
            output = directory / host
            output.mkdir()
            record["files"] = binaryen.unpack_optimizer(archive_path, version, record, output, discover=True)
            candidate["hosts"][host] = record
            archive_path.unlink()

    return binaryen.validate_manifest(candidate)


def update_manifest(root: Path, expected: dict, candidate: dict) -> None:
    """Atomically replace one valid pin after rejecting downgrade and concurrent edits."""
    binaryen.validate_manifest(candidate)
    if candidate["version"] <= expected["version"]:
        raise ValueError("Binaryen updates require a newer release; refusing downgrade or replacement")

    path = binaryen.regular_path(root / PIN)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".binaryen-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write((json.dumps(candidate, indent=2) + "\n").encode("ascii"))

    try:
        if binaryen.read_manifest(root) != expected:
            raise ValueError("Binaryen pin changed during the update; retry after review")

        temporary.chmod(0o644)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    """Print the local version offline, check official content, or prepare an explicit update."""
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--version", action="store_true", help="print the local Binaryen version offline")
    mode.add_argument("--check", action="store_true", help="check freshness and unchanged official archive identities")
    mode.add_argument("--update", action="store_true", help="prepare all host checksums for a reviewed pull request")
    args = parser.parse_args(argv)
    try:
        current = binaryen.read_manifest(ROOT)
        if args.version:
            print(current["version"])

            return 0

        latest = binaryen.fetch_json(f"{binaryen.API}/latest")
        version, _ = release_assets(latest)
        released = latest if version == current["version"] else binaryen.fetch_json(
            f"{binaryen.API}/tags/version_{current['version']}"
        )
        check_current_assets(current, released)
        if version < current["version"]:
            raise ValueError("official latest release is older than the pin; refusing downgrade")

        if version == current["version"]:
            print(f"Binaryen {version} is current; all five pinned release archives match.")

            return 0

        if not args.update:
            print(
                f"Binaryen {current['version']} is outdated; latest stable is {version}. "
                "Run python3 scripts/tool_versions.py --update, validate, and open a reviewed pull request.",
                file=sys.stderr,
            )

            return 1

        candidate = candidate_manifest(current, latest)
        update_manifest(ROOT, current, candidate)
        print(f"Updated Binaryen pin: {current['version']} -> {version}. Validate and open a reviewed pull request.")

        return 0
    except (OSError, ValueError, TypeError, KeyError, RecursionError, URLError) as error:
        print(f"Tool version check failed: {error}", file=sys.stderr)

        return 1


if __name__ == "__main__":
    raise SystemExit(main())
