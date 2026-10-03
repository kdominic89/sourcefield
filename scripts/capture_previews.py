#!/usr/bin/env python3
"""Capture README and interactive previews with a local Chromium installation."""

from __future__ import annotations

import argparse
import http.server
import shutil
import socketserver
import subprocess
import threading
import json
from pathlib import Path


class PreviewHandler(http.server.SimpleHTTPRequestHandler):
    """Serve local preview resources with visible request and error diagnostics."""


def find_browser() -> str:
    """Locate an installed browser without installing or weakening its sandbox."""
    for name in ("chromium", "chromium-browser", "google-chrome", "google-chrome-stable"):
        value = shutil.which(name)
        if value:
            return value

    mac_chrome = Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
    if mac_chrome.is_file():
        return str(mac_chrome)

    raise SystemExit("Chromium or Chrome is required to capture previews")


def capture(browser: str, url: str, output: Path, width: int, height: int, budget: int = 2500) -> None:
    """Capture a page and propagate browser failures to the caller."""
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [
        browser,
        "--headless=new",
        "--hide-scrollbars",
        f"--window-size={width},{height}",
        f"--virtual-time-budget={budget}",
        f"--screenshot={output}",
        url,
    ]

    subprocess.run(
        command,
        check=True,
        timeout=45,
    )


def main() -> int:
    """Capture both surfaces using the configured canonical canvas dimensions."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--site", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    site = args.site.resolve()
    output = args.output.resolve()
    browser = find_browser()
    with (site / "profile-state.json").open(encoding="utf-8") as handle:
        dimensions = json.load(handle)["canvas"]

    width, height = dimensions["width"], dimensions["height"]

    capture(
        browser,
        (site / "sourcefield.dark.svg").as_uri(),
        output / "sourcefield-readme-preview.png",
        width,
        height,
        1200,
    )

    handler = lambda *values, **kwargs: PreviewHandler(*values, directory=str(site), **kwargs)
    with socketserver.TCPServer(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_address[1]
            capture(
                browser,
                f"http://127.0.0.1:{port}/index.html",
                output / "sourcefield-pages-preview.png",
                width,
                height,
                4000,
            )
        finally:
            server.shutdown()
            thread.join(timeout=2)

    print(output / "sourcefield-readme-preview.png")
    print(output / "sourcefield-pages-preview.png")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
