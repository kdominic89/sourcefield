#!/usr/bin/env python3
"""Exercise all supported synthetic profile compositions in a real WASM-capable browser."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

from fixture_support import write_readme


ICON_KEYS = ("builtin:sourcefield", "builtin:database-safe", "browser-probe")
ICON_CATALOG = """
[icons.browser-probe]
radius = 64

[[icons.browser-probe.elements]]
geometry = { shape = "rect", origin = [-48, -48], size = [96, 96], corner_radius = 3 }
fill = "recess"
stroke = "mint"
stroke_width = 1

[[icons.browser-probe.elements]]
geometry = { shape = "circle", center = [0, 0], radius = 8 }
fill = "amber"
stroke = "none"
motion = { kind = "signal", phase = 1 }

[[icons.browser-probe.elements]]
geometry = { shape = "ellipse", center = [0, 0], radii = [16, 6] }
fill = "none"
stroke = "purple"

[[icons.browser-probe.elements]]
geometry = { shape = "path", commands = [{ command = "move", to = [-20, 0] }, { command = "line", to = [20, 0] }] }
fill = "none"
stroke = "blue"
"""


def icon_fixture(source: Path, destination: Path, *, organization: bool = False) -> Path:
    """Copy a synthetic config and its local imports, adding bounded catalog coverage."""
    text = source.read_text(encoding="utf-8")
    data = tomllib.loads(text)
    projects = data.get("projects", [])
    imports = data.get("imports", [])

    if imports:
        for imported in imports:
            location = imported["source"]

            if location["kind"] != "local":
                raise ValueError("browser fixtures require local organization imports")

            relative = Path(location["path"])

            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError("browser fixture imports must stay beside the copied config")

            icon_fixture(source.parent / relative, destination.parent / relative, organization=True)
    elif organization and len(projects) < len(ICON_KEYS):
        for index in range(len(projects), len(ICON_KEYS)):
            text += f"""
[[projects]]
id = "browser-icon-{index}"
label = "Browser icon {index}"
surface_label = "Icon {index}"
visibility = "private-abstract"
status = "active"
visual = "project"
summary = "Isolated release-browser catalog fixture."
show_in_readme = true
"""
    elif len(projects) < len(ICON_KEYS):
        raise ValueError("browser fixture requires three projects or local organization imports")

    project_index = 0

    def add_icon(match: re.Match[str]) -> str:
        """Assign exactly the three fixture selectors without rewriting authored geometry."""
        nonlocal project_index
        block = match.group(0)

        if project_index < len(ICON_KEYS):
            if re.search(r"^icon\s*=", block, flags=re.MULTILINE):
                raise ValueError("browser fixture source already contains an icon selector")

            block = block.replace("[[projects]]", f'[[projects]]\nicon = "{ICON_KEYS[project_index]}"', 1)

        project_index += 1
        return block

    text = re.sub(r"(?m)^\[\[projects\]\][\s\S]*?(?=^\[|\Z)", add_icon, text)

    if project_index:
        if "icons" in data:
            raise ValueError("browser fixture source already contains an icon catalog")

        text += ICON_CATALOG

    tomllib.loads(text)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(text, encoding="utf-8")

    return destination


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

    for variant, source_config in fixtures.items():
        config = icon_fixture(source_config, output / "inputs" / variant / source_config.name)
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
            "--require-icons",
        ]

        if browser is not None:
            command.extend(["--browser", browser])

        subprocess.run(command, env=environment, check=True)
        report = json.loads((output / "evidence" / variant / "results.json").read_text())
        if not report["checks"]["icons"].get("requiredFixture"):
            raise AssertionError("browser evidence is missing the required icon catalog fixture")

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
