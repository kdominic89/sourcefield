#!/usr/bin/env python3
"""Exercise all supported synthetic profile compositions in a real WASM-capable browser."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from fixture_support import write_readme


def verify_profiles(
    root: Path,
    binary: Path,
    output: Path,
    playwright_module: str,
    browser: str | None = None,
) -> None:
    """Generate isolated fixtures, then enforce the same actual-browser gate for each."""
    if output.exists():
        raise ValueError("browser fixture output must be a new directory")

    history = output / "producer-history"
    environment = {**os.environ, "SOURCEFIELD_TEST_HISTORY_OUTPUT": str(history)}
    subprocess.run(
        ["cargo", "test", "--locked", "-p", "sourcefield-cli",
         "generated_live_history_round_trips_producer_metadata"],
        cwd=root, env=environment, check=True,
    )
    if not (history / "index.json").is_file():
        raise AssertionError("the Rust producer did not export its required history fixture")

    fixtures = {
        "personal": root / "config/profile.toml",
        "organization": root / "config/organization-profile.toml",
        "multiple-organizations": root / "examples/multi-organization.toml",
    }

    for variant, config in fixtures.items():
        consumer = output / variant
        consumer.mkdir(parents=True)
        write_readme(consumer / "README.md", config)
        subprocess.run(
            [
                str(binary),
                "generate",
                "--root",
                str(consumer),
                "--config",
                str(config),
                "--fallback-snapshot",
                str(root / "config/offline-snapshot.json"),
                "--runtime",
                str(root / "runtime"),
                "--readme",
                "README.md",
                "--offline",
            ],
            check=True,
        )
        subprocess.run(
            [str(binary), "validate", "--root", str(consumer), "--config", str(config)],
            check=True,
        )
        subprocess.run(
            [sys.executable, "-B", str(root / "scripts/validate_artifact.py"),
             "--root", str(consumer), "--config", str(config), "--require-wasm"],
            check=True,
        )
        # Validate real producer output; synthetic geometry alone can miss protocol drift.
        sys.path.insert(0, str(root / "tests"))
        from test_presentation import validate_presentation

        validate_presentation(consumer / "assets")
        command = [
            "node",
            str(root / "scripts/verify-browser.mjs"),
            "--site",
            str(consumer / "docs"),
            "--output",
            str(output / "evidence" / variant),
            "--playwright-module",
            playwright_module,
        ]

        if browser is not None:
            command.extend(["--browser", browser])

        subprocess.run(command, env=environment, check=True)
        report = json.loads((output / "evidence" / variant / "results.json").read_text())
        if "real Rust producer history archive selected" not in json.dumps(report):
            raise AssertionError("browser evidence is missing the required Rust producer history check")


def main() -> int:
    """Run offline fixture generation with an explicitly provisioned browser test tool."""
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--playwright-module", required=True)
    parser.add_argument("--browser")
    args = parser.parse_args()
    verify_profiles(
        root,
        args.binary.resolve(),
        args.output.absolute(),
        args.playwright_module,
        args.browser,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
