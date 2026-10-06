#!/usr/bin/env python3
"""Read, check, or explicitly update the authoritative wasm-pack release pin."""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
import tempfile
from pathlib import Path
from typing import NamedTuple
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, Request, build_opener

ROOT = Path(__file__).resolve().parents[1]
PIN = "tools/wasm-pack-version.txt"
REGISTRY = "https://crates.io/api/v1/crates/wasm-pack"
MAX_PIN_BYTES = 64
MAX_RESPONSE_BYTES = 1024 * 1024
TIMEOUT_SECONDS = 15
VERSION_PATTERN = re.compile(r"(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})")
PRE_RELEASE_IDENTIFIER = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
RELEASE_PATTERN = re.compile(
    VERSION_PATTERN.pattern
    + rf"(?:-{PRE_RELEASE_IDENTIFIER}(?:\.{PRE_RELEASE_IDENTIFIER})*)?"
    + r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)


class RegistryStatus(NamedTuple):
    """Carry the current pin's yank status and latest stable from one registry response."""

    latest: str
    current_yanked: bool | None


class RejectRedirects(HTTPRedirectHandler):
    """Keep the fixed HTTPS registry boundary from following an untrusted location."""

    def redirect_request(self, request, stream, code, message, headers, new_url):
        """Reject redirects before urllib can issue a request to another endpoint."""
        raise ValueError("registry redirects are not allowed")


def version_parts(value: str) -> tuple[int, int, int]:
    """Accept only bounded canonical stable releases, suitable for a Cargo version flag."""
    if not isinstance(value, str) or VERSION_PATTERN.fullmatch(value) is None:
        raise ValueError("version must be a bounded canonical stable major.minor.patch release")

    return tuple(int(part) for part in value.split("."))


def pin_path(root: Path) -> Path:
    """Refuse substituted parent directories, symlinks, and nonregular pin files."""
    path = root
    for part in Path(PIN).parts:
        path = path / part
        if path.is_symlink():
            raise ValueError("tool pin must not contain symlinks")

    if not stat.S_ISREG(path.stat().st_mode):
        raise ValueError("tool pin must be a regular file")

    return path


def read_pin(root: Path) -> str:
    """Read one short ASCII release with exactly one final LF and no extra whitespace."""
    path = pin_path(root)
    with path.open("rb") as stream:
        data = stream.read(MAX_PIN_BYTES + 1)

    if len(data) > MAX_PIN_BYTES:
        raise ValueError("tool pin exceeds 64 bytes")

    value = data.decode("ascii")
    if not value.endswith("\n"):
        raise ValueError("tool pin must end with one LF")

    version = value[:-1]
    version_parts(version)

    return version


def registry_status(payload: bytes, current: str) -> RegistryStatus:
    """Validate one bounded release inventory before diagnosing the current and latest pins."""
    if len(payload) > MAX_RESPONSE_BYTES:
        raise ValueError("registry response exceeds 1 MiB")

    try:
        document = json.loads(payload)
    except (ValueError, RecursionError) as error:
        raise ValueError("registry response must be valid JSON") from error

    if not isinstance(document, dict):
        raise TypeError("registry response must be a JSON object")

    crate = document.get("crate")
    if not isinstance(crate, dict):
        raise TypeError("registry response must contain crate metadata")

    if crate.get("id") != "wasm-pack":
        raise ValueError("registry response crate identity does not match wasm-pack")

    latest = crate.get("max_stable_version")
    version_parts(latest)
    versions = document.get("versions")
    if not isinstance(versions, list) or not 1 <= len(versions) <= 512:
        raise ValueError("registry release inventory must contain 1 to 512 versions")

    releases = {}
    for entry in versions:
        if not isinstance(entry, dict):
            raise ValueError("registry release inventory is inconsistent: release must be an object")

        number = entry.get("num")
        yanked = entry.get("yanked")
        if (
            not isinstance(number, str) or len(number) > 128 or RELEASE_PATTERN.fullmatch(number) is None
            or not isinstance(yanked, bool)
        ):
            raise ValueError("registry release inventory is inconsistent: invalid release number or yank status")

        if number in releases:
            raise ValueError("registry release inventory is inconsistent: duplicate release number")

        releases[number] = yanked

    if releases.get(latest) is not False:
        raise ValueError("registry release inventory is inconsistent: latest stable must be present and not yanked")

    stable = [number for number, yanked in releases.items() if not yanked and VERSION_PATTERN.fullmatch(number)]
    if max(stable, key=version_parts) != latest:
        raise ValueError("registry release inventory is inconsistent: latest stable does not match listed releases")

    return RegistryStatus(latest, releases.get(current))


def fetch_registry_status(current: str) -> RegistryStatus:
    """Fetch only the official endpoint with a timeout, response cap, and no redirects."""
    request = Request(
        REGISTRY,
        headers={"Accept": "application/json", "User-Agent": "sourcefield-tool-freshness/1"},
    )
    opener = build_opener(RejectRedirects())
    with opener.open(request, timeout=TIMEOUT_SECONDS) as response:
        if response.geturl() != REGISTRY or response.status != 200:
            raise ValueError("registry response must be HTTP 200 from the fixed endpoint")

        content_length = response.headers.get("Content-Length")
        if content_length is not None:
            if not content_length.isdecimal() or len(content_length) > 10:
                raise ValueError("registry Content-Length must be a bounded decimal size")

            if int(content_length) > MAX_RESPONSE_BYTES:
                raise ValueError("registry response exceeds 1 MiB")

        payload = response.read(MAX_RESPONSE_BYTES + 1)

    return registry_status(payload, current)


def update_pin(root: Path, expected: str, latest: str) -> None:
    """Atomically replace the reviewed pin; reject downgrade or concurrent pin changes."""
    if version_parts(latest) < version_parts(expected):
        raise ValueError("registry version is older than the current pin; refusing downgrade")

    path = pin_path(root)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".wasm-pack-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(f"{latest}\n".encode("ascii"))

    try:
        if read_pin(root) != expected:
            raise ValueError("tool pin changed during the registry check; retry after review")

        temporary.chmod(0o644)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    """Print a local version offline, fail on staleness, or explicitly update only its pin."""
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--version", action="store_true", help="print the local pin without network access")
    mode.add_argument("--check", action="store_true", help="fail if the pin differs from crates.io latest stable")
    mode.add_argument("--update", action="store_true", help="update the pin for a manually reviewed pull request")
    args = parser.parse_args(argv)
    try:
        current = read_pin(ROOT)
        if args.version:
            print(current)

            return 0

        status = fetch_registry_status(current)
        latest = status.latest
        if status.current_yanked is None:
            raise ValueError(
                f"wasm-pack pin {current} is missing from the registry inventory; latest stable is {latest}. "
                f"Verify the registry response and manually review {PIN} before choosing a replacement."
            )

        if status.current_yanked:
            message = f"wasm-pack pin {current} is yanked; latest stable is {latest}."
            if version_parts(latest) < version_parts(current):
                raise ValueError(
                    f"{message} The candidate is older than the pin; refusing downgrade. "
                    "Review a non-yanked replacement, "
                    f"manually edit {PIN}, validate the build, and open a reviewed pull request."
                )

            print(f"Warning: {message}", file=sys.stderr)

        if version_parts(latest) < version_parts(current):
            raise ValueError("registry version is older than the current pin; refusing downgrade")

        if args.update and current != latest:
            update_pin(ROOT, current, latest)
            print(f"Updated wasm-pack pin: {current} -> {latest}. Validate and open a reviewed pull request.")

            return 0

        if current != latest:
            print(
                f"wasm-pack pin {current} is outdated; latest stable is {latest}. "
                "Run python3 scripts/tool_versions.py --update, validate the build, and open a reviewed pull request.",
                file=sys.stderr,
            )

            return 1

        print(f"wasm-pack pin {current} matches crates.io latest stable.")

        return 0
    except (OSError, ValueError, TypeError, URLError, HTTPError) as error:
        print(f"Tool version check failed: {error}", file=sys.stderr)

        return 1


if __name__ == "__main__":
    raise SystemExit(main())
