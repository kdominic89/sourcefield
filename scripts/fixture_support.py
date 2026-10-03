"""Shared synthetic consumer setup for executable publication and browser gates."""

from pathlib import Path
import tomllib


def write_readme(destination: Path, config: Path) -> None:
    """Create authored fallback links and empty managed regions before generation."""
    with config.open("rb") as handle:
        pages = tomllib.load(handle)["profile"]["pages_url"]

    destination.write_text(
        '# Synthetic profile\n\n'
        '<picture>\n'
        '<source media="(prefers-reduced-motion: reduce)" srcset="assets/sourcefield.static.svg">\n'
        '<source media="(prefers-color-scheme: light)" srcset="assets/sourcefield.light.svg">\n'
        '<img src="assets/sourcefield.dark.svg" alt="Synthetic profile">\n'
        '</picture>\n\n'
        f'[Open the interactive profile]({pages})\n\n'
        '<!-- sourcefield:projects:start --><!-- sourcefield:projects:end -->\n'
        '<!-- sourcefield:packages:start --><!-- sourcefield:packages:end -->\n',
        encoding="utf-8",
    )
