"""Synthetic official Binaryen metadata and bounded transport doubles."""

from __future__ import annotations

import copy
import hashlib
import io
import json
from pathlib import Path
import tarfile

from support import ROOT
from sourcefield_tools import binaryen

HOST = "x86_64-unknown-linux-gnu"


def manifest() -> dict:
    """Copy the actual authored schema so tests change one boundary at a time."""

    return copy.deepcopy(binaryen.read_manifest(ROOT))


def release(document: dict | None = None) -> dict:
    """Represent relevant official release metadata without contacting GitHub."""
    document = document or manifest()
    version = document["version"]
    tag = f"version_{version}"

    return {
        "tag_name": tag, "prerelease": False, "draft": False,
        "html_url": f"https://github.com/WebAssembly/binaryen/releases/tag/{tag}",
        "assets": [
            {"name": item["archive"]["name"], "size": item["archive"]["size"], "state": "uploaded",
             "digest": "sha256:" + item["archive"]["sha256"],
             "browser_download_url": f"{binaryen.DOWNLOAD}/{tag}/{item['archive']['name']}"}
            for item in document["hosts"].values()
        ],
    }


def future(document: dict, version: int = 134) -> dict:
    """Keep fixture identities coherent while changing the release number."""
    result = copy.deepcopy(document)
    result["version"] = version
    for host, item in result["hosts"].items():
        item["archive"]["name"] = binaryen.archive_name(version, host)

    return result


def write_manifest(root: Path, document: dict) -> Path:
    """Write one isolated pin for command and cache tests."""
    path = root / binaryen.PIN
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document) + "\n", encoding="ascii")

    return path


def archive_fixture(entries: list[tuple[str, bytes, bytes]], version: int = 133) -> tuple[bytes, dict]:
    """Create tar members with explicit names/data/types and pin their exact bytes."""
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name, data, kind in entries:
            member = tarfile.TarInfo(name)
            member.type = kind
            member.size = len(data) if kind == tarfile.REGTYPE else 0
            member.linkname = "../../outside" if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE) else ""
            archive.addfile(member, io.BytesIO(data))

    data = output.getvalue()
    payload = next(
        (value for name, value, _ in entries if name == f"binaryen-version_{version}/bin/wasm-opt"), b"missing"
    )

    return data, {
        "archive": {"name": binaryen.archive_name(version, HOST), "size": len(data),
                    "sha256": hashlib.sha256(data).hexdigest()},
        "files": [
            {"path": "bin/wasm-opt", "size": max(1, len(payload)), "sha256": hashlib.sha256(payload).hexdigest()},
        ],
    }


class Response(io.BytesIO):
    """Expose controlled HTTP metadata and record bounded body reads."""

    def __init__(self, body: bytes, *, url: str = binaryen.API + "/latest", status: int = 200, **headers):
        """Initialize one response with no network interaction."""
        super().__init__(body)
        self.url, self.status, self.headers = url, status, headers
        self.read_sizes = []

    def geturl(self) -> str:
        """Return the response's admitted or deliberately invalid final endpoint."""

        return self.url

    def read(self, size: int = -1) -> bytes:
        """Capture the caller's limit before returning synthetic response bytes."""
        self.read_sizes.append(size)

        return super().read(size)
