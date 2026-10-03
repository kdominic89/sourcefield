#!/usr/bin/env python3
"""Verify ignored generated files survive two fresh-checkout publication cycles."""

from __future__ import annotations

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

from consumer_candidate import candidate
from consumer_publish import apply_candidate
from fixture_support import write_readme


def git(root: Path, *arguments: str) -> str:
    """Run Git only inside the temporary fixture and return checked text output."""
    return subprocess.run(
        ["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
         "-c", "commit.gpgsign=false", *arguments],
        cwd=root, check=True, capture_output=True, text=True,
    ).stdout.strip()


def verify(root: Path, binary: Path) -> None:
    """Publish twice through local Git transport and assert ignored runtime regeneration."""
    with tempfile.TemporaryDirectory(prefix="sourcefield-publication-") as temporary:
        workspace = Path(temporary).resolve()
        remote = workspace / "remote.git"
        git(workspace, "init", "--bare", "--initial-branch=main", str(remote))
        seed = workspace / "seed"
        git(workspace, "clone", str(remote), str(seed))
        shutil.copytree(root / "config", seed / "config")
        (seed / ".gitignore").write_text("/docs/pkg/\n", encoding="utf-8")
        write_readme(seed / "README.md", seed / "config/profile.toml")
        git(seed, "add", "config", ".gitignore", "README.md")
        git(seed, "commit", "-m", "test: initialize publication fixture")
        git(seed, "push", "origin", "main")
        installation = workspace / "installation"
        installation.mkdir()
        shutil.copyfile(binary, installation / binary.name)
        (installation / binary.name).chmod(0o755)
        shutil.copytree(root / "runtime", installation / "runtime")

        for cycle in range(2):
            checkout = workspace / f"checkout-{cycle}"
            git(workspace, "clone", str(remote), str(checkout))
            if (checkout / "docs/pkg").exists():
                raise AssertionError("generated runtime unexpectedly survived a fresh clone")

            revision = git(checkout, "rev-parse", "HEAD")
            output = workspace / f"candidate-{cycle}"
            candidate(checkout, output, installation, "config/profile.toml", ["README.md"], True, False)
            staged = apply_candidate(checkout, output, revision, "main")
            if any(name.startswith("docs/pkg/") for name in staged):
                raise AssertionError("publication staged ignored runtime artifacts")

            if not (checkout / "docs/pkg/sourcefield_wasm_bg.wasm").is_file():
                raise AssertionError("publication did not regenerate the ignored WASM module")

            # The second identical generation may be a legitimate no-op.
            changes = git(checkout, "diff", "--cached", "--name-only")
            if changes:
                git(checkout, "commit", "-m", "test: publish generated fixture")
                git(checkout, "push", "origin", "main")

        print("PUBLICATION OK: two fresh clones regenerated ignored runtime files")


def main() -> int:
    """Select an already rebuilt native executable; never build or publish a real profile."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    verify(Path(__file__).resolve().parents[1], args.binary.resolve())

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
