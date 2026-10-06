"""Bounded artifact hashing and host-independent archive metadata."""

import hashlib
import zipfile
from pathlib import Path


def digest_file(path: Path) -> str:
    """Hash bounded chunks rather than loading a release archive into memory."""
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)

    return result.hexdigest()


def zip_info(name: str, executable: bool = False) -> zipfile.ZipInfo:
    """Normalize timestamps and permissions instead of inheriting host metadata."""
    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    info.create_system = 3
    info.external_attr = (0o100755 if executable else 0o100644) << 16
    # Stored entries avoid compressor-version drift across build hosts.
    info.compress_type = zipfile.ZIP_STORED

    return info
