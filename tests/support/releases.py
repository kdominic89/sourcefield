"""Build complete synthetic release fixtures shared by installation and publication tests."""

import json
from pathlib import Path

import release_manifest
import release_package
from runtime_manifest import RUNTIME_FILES
from sourcefield_tools.artifacts import digest_file
from sourcefield_tools.release import TARGETS, read_lock


def release_lock() -> dict:
    """Return a complete synthetic release identity, never a purported public release."""

    return {
        "schema_version": 1,
        "repository": "kdominic89/sourcefield",
        "source_commit": "a" * 40,
        "release": "v1.2.3",
        "workflow": ".github/workflows/release.yml",
        "assets": {
            target: {"name": f"sourcefield-{target}.zip", "sha256": "b" * 64}
            for target in set(TARGETS.values()) | {"browser"}
        },
    }


def make_assets(root: Path) -> dict:
    """Arrange a complete synthetic native/browser release with real ZIP metadata."""
    binary = root / "native"
    binary.write_bytes(b"synthetic executable")
    runtime = root / "runtime"

    for relative in RUNTIME_FILES:
        path = runtime / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"synthetic runtime")

    (runtime / "runtime-manifest.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "source_revision": "a" * 40,
                "generator_version": "1.2.3",
                "source_fingerprint": "c" * 64,
                "files": {
                    name: digest_file(runtime / name)
                    for name in RUNTIME_FILES
                },
            }
        )
    )

    for target in sorted(set(TARGETS.values()) | {"browser"}):
        release_package.package(
            root, root / "assets", target, "a" * 40, "v1.2.3", binary
        )

    lock_path = release_manifest.manifest(root / "assets", "a" * 40, "v1.2.3")

    return read_lock(lock_path)


def published_state(root: Path) -> dict:
    """Arrange the complete remote inventory of an already-published synthetic release."""

    return {"draft": False, "tag": True, "assets": {
        path.name: {"name": path.name, "size": path.stat().st_size,
                    "digest": f"sha256:{digest_file(path)}"}
        for path in (root / "assets").iterdir()
    }}
