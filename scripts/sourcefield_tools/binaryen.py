"""Provision only reviewed Binaryen bytes; never discover build executables on PATH."""

from __future__ import annotations

import errno
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import tarfile
import tempfile
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener

PIN = "tools/binaryen.json"
API = "https://api.github.com/repos/WebAssembly/binaryen/releases"
DOWNLOAD = "https://github.com/WebAssembly/binaryen/releases/download"
MAX_MANIFEST_BYTES = 64 * 1024
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
MAX_EXPANDED_BYTES = 1024 * 1024 * 1024
MAX_MEMBER_BYTES = 512 * 1024 * 1024
MAX_MEMBERS = 256
CHUNK_BYTES = 64 * 1024
TIMEOUT_SECONDS = 30
HOSTS = {
    "aarch64-apple-darwin": "arm64-macos",
    "x86_64-apple-darwin": "x86_64-macos",
    "x86_64-unknown-linux-gnu": "x86_64-linux",
    "aarch64-unknown-linux-gnu": "aarch64-linux",
    "x86_64-pc-windows-msvc": "x86_64-windows",
}
OPTIMIZER_FLAGS = (
    "-O", "--enable-bulk-memory", "--enable-multivalue", "--enable-mutable-globals",
    "--enable-nontrapping-float-to-int", "--enable-reference-types", "--enable-sign-ext",
)


def positive_integer(value: object, maximum: int, label: str) -> int:
    """Reject Boolean, negative and unbounded numeric metadata without coercion."""
    if type(value) is not int or not 1 <= value <= maximum:
        raise ValueError(f"{label} must be an integer from 1 to {maximum}")

    return value


def digest_value(value: object) -> str:
    """Require one canonical lowercase SHA-256 digest."""
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise ValueError("SHA-256 must contain exactly 64 lowercase hexadecimal characters")

    return value


def regular_path(path: Path, *, required: bool = True) -> Path:
    """Refuse symlink substitution in a local file or any ancestor."""
    path = path.absolute()
    for component in (path, *path.parents):
        if component.is_symlink():
            raise ValueError(f"symlink in build-tool path: {component}")

    if required and not stat.S_ISREG(path.stat().st_mode):
        raise ValueError(f"build-tool input must be a regular file: {path}")

    return path


def required_files(host: str) -> tuple[str, ...]:
    """Keep only the optimizer and its required sibling macOS shared library."""
    if host not in HOSTS:
        raise ValueError(f"unsupported Binaryen host: {host}")

    if host.endswith("apple-darwin"):
        return "bin/wasm-opt", "lib/libbinaryen.dylib"

    return ("bin/wasm-opt.exe" if "windows" in host else "bin/wasm-opt",)


def archive_name(version: int, host: str) -> str:
    """Construct the official release asset name from validated version and host."""
    positive_integer(version, 999999, "Binaryen version")
    required_files(host)

    return f"binaryen-version_{version}-{HOSTS[host]}.tar.gz"


def validate_manifest(document: object) -> dict:
    """Validate the complete authored pin before any download, extraction or execution."""
    if not isinstance(document, dict) or set(document) != {"schema_version", "version", "optimizer_flags", "hosts"}:
        raise ValueError("Binaryen manifest must contain only schema_version, version, optimizer_flags and hosts")

    if type(document["schema_version"]) is not int or document["schema_version"] != 1:
        raise ValueError("Binaryen manifest schema_version must be 1")

    version = positive_integer(document["version"], 999999, "Binaryen version")
    if document["optimizer_flags"] != list(OPTIMIZER_FLAGS):
        raise ValueError("Binaryen optimizer_flags must preserve the reviewed optimization and feature policy")

    hosts = document["hosts"]
    if not isinstance(hosts, dict) or set(hosts) != set(HOSTS):
        raise ValueError("Binaryen manifest must pin all five supported hosts")

    for host, record in hosts.items():
        if not isinstance(record, dict) or set(record) != {"archive", "files"}:
            raise ValueError("Binaryen host pin must contain only archive and files")

        archive = record["archive"]
        if not isinstance(archive, dict) or set(archive) != {"name", "size", "sha256"}:
            raise ValueError("Binaryen archive pin must contain name, size and sha256")

        if archive["name"] != archive_name(version, host):
            raise ValueError("Binaryen archive name does not match the version and host")

        positive_integer(archive["size"], MAX_ARCHIVE_BYTES, "Binaryen archive size")
        digest_value(archive["sha256"])
        files = record["files"]
        if not isinstance(files, list) or len(files) != len(required_files(host)):
            raise ValueError("Binaryen host must pin exactly its required optimizer files")

        for record_file, expected in zip(files, required_files(host), strict=True):
            if not isinstance(record_file, dict) or set(record_file) != {"path", "size", "sha256"}:
                raise ValueError("Binaryen file pin must contain path, size and sha256")

            if record_file["path"] != expected:
                raise ValueError("Binaryen file path does not match the required optimizer layout")

            positive_integer(record_file["size"], MAX_MEMBER_BYTES, "Binaryen file size")
            digest_value(record_file["sha256"])

    return document


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    """Reject duplicate JSON keys instead of accepting parser-dependent identities."""
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError(f"duplicate JSON key: {name}")

        result[name] = value

    return result


def read_manifest(root: Path) -> dict:
    """Read a bounded, regular ASCII manifest with no symlinked path components."""
    path = regular_path(root / PIN)
    with path.open("rb") as stream:
        payload = stream.read(MAX_MANIFEST_BYTES + 1)

    if len(payload) > MAX_MANIFEST_BYTES:
        raise ValueError("Binaryen manifest exceeds 64 KiB")

    return validate_manifest(json.loads(payload.decode("ascii"), object_pairs_hook=unique_object))


def https_url(url: str, hosts: set[str]) -> str:
    """Admit only credential-free HTTPS at an explicit origin set."""
    parsed = urlsplit(url)
    if (
        parsed.scheme != "https" or parsed.hostname not in hosts or parsed.port not in (None, 443)
        or parsed.username is not None or parsed.password is not None or parsed.fragment
    ):
        raise ValueError("build-tool transport requires credential-free HTTPS at an approved origin")

    return url


class AssetRedirects(HTTPRedirectHandler):
    """Allow GitHub's signed release CDN while rejecting arbitrary redirect targets."""

    def __init__(self, asset: bool):
        """Keep redirect admission local to one download, never registry metadata."""
        super().__init__()
        self.asset = asset
        self.count = 0

    def redirect_request(self, request, stream, code, message, headers, new_url):
        """Validate destinations before urllib sends another request."""
        self.count += 1
        if not self.asset or self.count > 4:
            raise ValueError("build-tool redirects are not allowed here")

        https_url(new_url, {"github.com", "release-assets.githubusercontent.com"})
        # Rebuild headers so authorization or proxy credentials cannot cross origins.
        return Request(new_url, headers={"User-Agent": "sourcefield-build-tools/1"})


def open_response(url: str, *, asset: bool):
    """Use default TLS verification without inherited proxy credentials or ambient auth."""
    https_url(url, {"github.com"} if asset else {"api.github.com"})
    request = Request(
        url,
        headers={"Accept": "application/octet-stream" if asset else "application/json",
                 "User-Agent": "sourcefield-build-tools/1"},
    )
    opener = build_opener(ProxyHandler({}), AssetRedirects(asset))

    return opener.open(request, timeout=TIMEOUT_SECONDS)


def check_response(response, *, asset: bool, expected: int | None, maximum: int) -> None:
    """Validate final response identity and bounded length before consuming its body."""
    hosts = {"github.com", "release-assets.githubusercontent.com"} if asset else {"api.github.com"}
    https_url(response.geturl(), hosts)
    if response.status != 200:
        raise ValueError("build-tool response must have HTTP status 200")

    value = response.headers.get("Content-Length")
    if value is not None:
        if not value.isascii() or not value.isdecimal() or len(value) > 10:
            raise ValueError("build-tool Content-Length must be a bounded decimal size")

        length = int(value)
        if length > maximum or (expected is not None and length != expected):
            raise ValueError("build-tool Content-Length does not match the admitted size")


def fetch_json(url: str) -> dict:
    """Read only bounded official GitHub release metadata, with no redirects."""
    if url != f"{API}/latest" and re.fullmatch(re.escape(API) + r"/tags/version_[1-9][0-9]{0,5}", url) is None:
        raise ValueError("release metadata must use the fixed official GitHub endpoint")

    with open_response(url, asset=False) as response:
        check_response(response, asset=False, expected=None, maximum=1024 * 1024)
        if response.geturl() != url:
            raise ValueError("release metadata endpoint changed")

        data = response.read(1024 * 1024 + 1)

    if len(data) > 1024 * 1024:
        raise ValueError("release metadata exceeds 1 MiB")

    document = json.loads(data, object_pairs_hook=unique_object)
    if not isinstance(document, dict):
        raise ValueError("release metadata must be a JSON object")

    return document


def download_archive(version: int, record: dict, destination: Path) -> None:
    """Stream exact pinned archive bytes to disk and authenticate before opening a parser."""
    archive = record["archive"]
    url = f"{DOWNLOAD}/version_{version}/{archive['name']}"
    digest = hashlib.sha256()
    count = 0
    with open_response(url, asset=True) as response, destination.open("xb") as output:
        check_response(response, asset=True, expected=archive["size"], maximum=MAX_ARCHIVE_BYTES)
        while block := response.read(min(CHUNK_BYTES, archive["size"] + 1 - count)):
            count += len(block)
            if count > archive["size"]:
                raise ValueError("Binaryen archive exceeds its pinned size")

            output.write(block)
            digest.update(block)

    if count != archive["size"] or digest.hexdigest() != archive["sha256"]:
        raise ValueError("Binaryen archive size or SHA-256 does not match the committed pin")


def verify_file(path: Path, record: dict) -> None:
    """Reauthenticate a cached executable or library every time it is used."""
    path = regular_path(path)
    if path.stat().st_size != record["size"]:
        raise ValueError(f"Binaryen cached file size mismatch: {record['path']}")

    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(CHUNK_BYTES):
            digest.update(block)

    if digest.hexdigest() != record["sha256"]:
        raise ValueError(f"Binaryen cached file SHA-256 mismatch: {record['path']}")


class BoundedReader(io.RawIOBase):
    """Bound inflated tar bytes even when tarfile skips unselected members."""

    def __init__(self, stream):
        """Wrap one gzip reader without buffering the complete archive."""
        self.stream = stream
        self.remaining = MAX_EXPANDED_BYTES

    def read(self, size: int = -1) -> bytes:
        """Stop decompression once the global expansion budget is exhausted."""
        if size < 0:
            size = CHUNK_BYTES

        data = self.stream.read(min(size, self.remaining + 1))
        self.remaining -= len(data)
        if self.remaining < 0:
            raise ValueError("Binaryen archive exceeds the expanded byte budget")

        return data


class RegularTarInfo(tarfile.TarInfo):
    """Refuse extended headers before tarfile allocates or interprets their payload."""

    def _proc_pax(self, archive):
        """Reject PAX metadata; reviewed assets need only ordinary regular files and directories."""
        raise ValueError("Binaryen archive contains unsupported extended metadata")

    def _proc_gnulong(self, archive):
        """Reject GNU long paths before their unbounded body is read into memory."""
        raise ValueError("Binaryen archive contains unsupported extended metadata")

    def _proc_sparse(self, archive):
        """Reject GNU sparse files instead of allocating from their claimed geometry."""
        raise ValueError("Binaryen archive contains unsupported sparse metadata")


def unpack_optimizer(archive: Path, version: int, record: dict, destination: Path, *, discover: bool = False) -> list:
    """Copy only admitted regular members, never tar paths, links, modes or extraction APIs."""
    prefix = f"binaryen-version_{version}/"
    expected = {prefix + item["path"]: item for item in record["files"]}
    seen = set()
    found = []
    total = 0
    with (
        gzip.open(archive, "rb") as compressed,
        tarfile.open(fileobj=BoundedReader(compressed), mode="r|", tarinfo=RegularTarInfo) as source,
    ):
        for entry in source:
            name = entry.name
            pure = PurePosixPath(name)
            if (
                name in seen or len(seen) >= MAX_MEMBERS or not name or "\\" in name or pure.is_absolute()
                or ".." in pure.parts or pure.as_posix() != name.rstrip("/")
                or (not entry.isfile() and not entry.isdir())
                or not (name == prefix.rstrip("/") or name.startswith(prefix))
            ):
                raise ValueError("Binaryen archive contains an unsafe, duplicate or excessive member")

            seen.add(name)
            if entry.size < 0 or entry.size > MAX_MEMBER_BYTES:
                raise ValueError("Binaryen archive member exceeds its size budget")

            total += entry.size
            if total > MAX_EXPANDED_BYTES:
                raise ValueError("Binaryen archive exceeds its declared byte budget")

            if name not in expected:
                continue

            pin = expected[name]
            if not entry.isfile() or (not discover and entry.size != pin["size"]):
                raise ValueError("Binaryen required member is not a regular file of the pinned size")

            target = destination / pin["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            digest = hashlib.sha256()
            count = 0
            with source.extractfile(entry) as member, target.open("xb") as output:
                while block := member.read(CHUNK_BYTES):
                    count += len(block)
                    output.write(block)
                    digest.update(block)

            actual = {"path": pin["path"], "size": count, "sha256": digest.hexdigest()}
            if count != entry.size or (not discover and actual != pin):
                raise ValueError("Binaryen required member SHA-256 does not match the committed pin")

            # Ignore archive permission bits, especially setuid/setgid and platform-specific modes.
            target.chmod(0o755 if pin["path"].startswith("bin/") else 0o644)
            found.append(actual)

    if {item["path"] for item in found} != {item["path"] for item in record["files"]}:
        raise ValueError("Binaryen archive is missing a required optimizer file")

    return sorted(found, key=lambda item: list(expected).index(prefix + item["path"]))


def validate_cache(directory: Path, record: dict) -> None:
    """Refuse contaminated caches and substituted parents as well as altered binaries."""
    regular_path(directory, required=False)
    allowed = {item["path"] for item in record["files"]}
    allowed.update(str(PurePosixPath(name).parent) for name in tuple(allowed))
    for path in directory.rglob("*"):
        if path.is_symlink() or path.relative_to(directory).as_posix() not in allowed:
            raise ValueError("Binaryen cache contains an unexpected file or symlink")

    for item in record["files"]:
        verify_file(directory / item["path"], item)


def ensure_optimizer(root: Path, host: str, cache: Path, *, offline: bool = False) -> Path:
    """Return one authenticated optimizer, provisioning only the committed official archive."""
    manifest = read_manifest(root)
    required_files(host)
    record = manifest["hosts"][host]
    cache = regular_path(cache, required=False)
    cache.mkdir(parents=True, exist_ok=True)
    key = f"binaryen-{manifest['version']}-{host}-{record['archive']['sha256']}"
    destination = cache / key
    if destination.exists() or destination.is_symlink():
        validate_cache(destination, record)

        return destination / record["files"][0]["path"]

    if offline:
        raise ValueError("Binaryen is not provisioned in the verified cache; run once without --offline")

    # Prepare in the same filesystem; only a complete, checked tree becomes a reusable cache.
    temporary = Path(tempfile.mkdtemp(prefix=".binaryen-", dir=cache))
    try:
        archive = temporary / "archive.tar.gz"
        download_archive(manifest["version"], record, archive)
        prepared = temporary / "ready"
        prepared.mkdir()
        unpack_optimizer(archive, manifest["version"], record, prepared)
        validate_cache(prepared, record)
        try:
            prepared.rename(destination)
        except OSError as error:
            if error.errno not in (errno.EEXIST, errno.ENOTEMPTY):
                raise

            # A concurrent builder may finish first, but never trust its files without rechecking.
            validate_cache(destination, record)

    finally:
        shutil.rmtree(temporary)

    validate_cache(destination, record)

    return destination / record["files"][0]["path"]


def _copy_snapshot_file(source: Path, target: Path, pin: dict) -> None:
    """Hash the bounded bytes actually copied, rather than trusting an earlier cache check."""
    source = regular_path(source)
    # A raced symlink or FIFO must not redirect or indefinitely block the execution copy.
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    descriptor = os.open(source, flags)
    with os.fdopen(descriptor, "rb") as input_file, target.open("xb") as output:
        metadata = os.fstat(input_file.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size != pin["size"]:
            raise ValueError(f"Binaryen snapshot file size or type mismatch: {pin['path']}")

        digest = hashlib.sha256()
        count = 0
        while block := input_file.read(min(CHUNK_BYTES, pin["size"] + 1 - count)):
            count += len(block)
            if count > pin["size"]:
                raise ValueError(f"Binaryen snapshot file exceeds its pinned size: {pin['path']}")

            output.write(block)
            digest.update(block)

    if count != pin["size"]:
        raise ValueError(f"Binaryen snapshot file size mismatch: {pin['path']}")

    if digest.hexdigest() != pin["sha256"]:
        raise ValueError(f"Binaryen snapshot file SHA-256 mismatch: {pin['path']}")

    target.chmod(0o700 if pin["path"].startswith("bin/") else 0o600)


def snapshot_optimizer(source: Path, manifest: dict, host: str, destination: Path) -> Path:
    """Create a private authenticated optimizer copy from the caller's frozen manifest.

    The reusable cache may change while Cargo runs. Only these freshly copied and
    authenticated executable/library bytes may be invoked by the current build.
    """
    validate_manifest(manifest)
    required_files(host)
    record = manifest["hosts"][host]
    source = regular_path(source)
    source_root = source.parent.parent
    if source != source_root / record["files"][0]["path"]:
        raise ValueError("Binaryen snapshot source must match the pinned optimizer layout")

    destination = regular_path(destination, required=False)
    destination.mkdir(mode=0o700)
    try:
        for pin in record["files"]:
            target = destination / pin["path"]
            target.parent.mkdir(mode=0o700, exist_ok=True)
            _copy_snapshot_file(source_root / pin["path"], target, pin)
    except BaseException:
        shutil.rmtree(destination)
        raise

    return destination / record["files"][0]["path"]
