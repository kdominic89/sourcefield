#!/usr/bin/env python3
"""Validate the public SOURCEFIELD artifact with Python's standard library."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys
import tomllib
import xml.etree.ElementTree as ET
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

SVG_FILES = ("sourcefield.dark.svg", "sourcefield.light.svg", "sourcefield.static.svg")
BROWSER_FILES = ("index.html", "app.css", "app.js", "profile-state.json", "simulation-fallback.js")


class HtmlResources(HTMLParser):
    """Collect executable and stylesheet references for local-resource validation."""

    def __init__(self) -> None:
        """Initialize resource lists for one HTML document."""
        super().__init__()
        self.scripts: list[str] = []
        self.styles: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        """Record script and stylesheet source attributes."""
        values = dict(attrs)
        if tag == "script" and values.get("src"):
            self.scripts.append(values["src"] or "")

        if tag == "link" and values.get("rel") == "stylesheet" and values.get("href"):
            self.styles.append(values["href"] or "")


def fail(message: str) -> None:
    """Abort the current validation group with a human-readable reason."""
    raise AssertionError(message)


def load_json(path: Path) -> Any:
    """Read JSON and preserve useful file context on malformed input."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"invalid JSON {path}: {error}")


def local_name(tag: str) -> str:
    """Return an XML name without its namespace prefix."""
    return tag.rsplit("}", 1)[-1]


def require_files(root: Path, assets: Path | None = None, docs: Path | None = None,
                  config: Path | None = None, readme: Path | None = None) -> None:
    """Check generated consumer files without requiring copied generator implementation."""
    assets = assets or root / "assets"
    docs = docs or root / "docs"
    paths = [assets / name for name in (*SVG_FILES, "profile-state.json")]
    paths.extend(docs / name for name in BROWSER_FILES)
    paths.extend((config or root / "config/profile.toml", readme or root / "README.md"))

    for path in paths:
        if path.is_symlink() or any(parent.is_symlink() for parent in path.parents):
            fail(f"required file must not traverse a symlink: {path}")

        if not path.is_file():
            fail(f"missing required artifact file: {path}")


def validate_config(root: Path, config_path: Path | None = None) -> dict[str, Any]:
    """Validate generic configuration structure independently of profile identity."""
    with (config_path or root / "config/profile.toml").open("rb") as handle:
        config = tomllib.load(handle)

    if config.get("schema_version") != 1:
        fail("config schema_version must be 1")

    for section in ("domains", "technologies", "projects", "publications"):
        identifiers = [item["id"] for item in config.get(section, [])]
        if len(identifiers) != len(set(identifiers)):
            fail(f"duplicate IDs in {section}")

    render = config["render"]
    if render["width"] < 1200 or render["height"] < 640:
        fail("README canvas is too small")

    return config


def effective_config(authored: dict[str, Any], assets: Path) -> dict[str, Any]:
    """Read the native generator's merged capture without inferring authority from graph nodes.

    This supplemental validator does not authenticate imports. Run native validation first;
    lock/provenance checks belong to generation and replay, not a second Python resolver.
    """
    path = assets / "resolved-config.json"
    if not path.is_file():
        if authored.get("imports"):
            fail("imported profiles require resolved-config.json")

        return authored

    config = load_json(path)
    if config.get("schema_version") != 1 or any(config.get("profile", {}).get(key) != value
            for key, value in authored.get("profile", {}).items()):
        fail("resolved configuration does not match the authored profile")

    return config


def validate_approved_profile(config: dict[str, Any], contract: dict[str, Any]) -> None:
    """Enforce independently supplied editorial facts instead of embedding consumer identities."""
    allowed = {"fields", "domains", "technologies", "projects", "publications", "packages"}
    if not isinstance(contract, dict) or not contract or set(contract) - allowed:
        fail("editorial contract must contain recognized independent expectations")

    for section, fields in contract.get("fields", {}).items():
        for key, expected in fields.items():
            if config.get(section, {}).get(key) != expected:
                fail(f"editorial contract mismatch: {section}.{key}")

    for section in ("domains", "technologies", "projects", "publications"):
        actual = {item["id"]: item for item in config.get(section, [])}
        if section not in contract:
            continue

        expected = contract[section]
        if set(actual) != set(expected):
            fail(f"editorial contract identities differ: {section}")

        for identifier, fields in expected.items():
            for key, value in fields.items():
                if actual[identifier].get(key) != value:
                    fail(f"editorial contract mismatch: {section}.{identifier}.{key}")

    if "packages" in contract:
        actual_packages = {package["id"] for group in config.get("publications", [])
                           for package in group.get("packages", [])}
        if actual_packages != set(contract["packages"]):
            fail("editorial contract package identities differ")

def approved_discovered_package(package: dict[str, Any], config: dict[str, Any]) -> bool:
    """Bind a discovered ID to its own publication family's verified registry owner."""
    observed_owner = (package.get("owner") or "").casefold()
    package_id = package.get("id", "").casefold()
    if not observed_owner or not package.get("version"):
        return False

    for group in config.get("publications", []):
        owner = (group.get("owner") or config.get("collection", {}).get("nuget_owner") or "").casefold()
        if owner != observed_owner:
            continue

        if any(package_id == prefix.casefold() or package_id.startswith(prefix.casefold() + ".")
               for prefix in group.get("discovery_prefixes", [])):
            return True

    return False


def validate_state(root: Path, config: dict[str, Any], assets: Path | None = None,
                   docs: Path | None = None) -> dict[str, Any]:
    """Check configured identities, geometry and graph references in both public copies."""
    assets_path = (assets or root / "assets") / "profile-state.json"
    docs_path = (docs or root / "docs") / "profile-state.json"
    if assets_path.read_bytes() != docs_path.read_bytes():
        fail("assets/profile-state.json and docs/profile-state.json must be byte-identical")

    state = load_json(assets_path)

    if state.get("schema_version") != 3:
        fail("state schema_version must be 3")

    if not re.fullmatch(r"[A-F0-9]{16}", state.get("semantic_hash", "")):
        fail("semantic_hash must be 16 uppercase hexadecimal characters")

    canvas = state.get("canvas", {})
    if canvas.get("width") != config["render"]["width"] or canvas.get("height", 0) < config["render"]["height"]:
        fail("state canvas differs from configuration")

    if state.get("profile", {}).get("username") != config["profile"]["username"]:
        fail("state profile username is incorrect")

    if state.get("profile", {}).get("organization") != config["profile"]["organization"]:
        fail("state organization is incorrect")

    nodes = state.get("nodes")
    edges = state.get("edges")
    if not isinstance(nodes, list) or not nodes:
        fail("state must contain nodes")

    if not isinstance(edges, list) or not edges:
        fail("state must contain edges")

    ids = [node.get("id") for node in nodes]
    duplicates = [item for item, count in Counter(ids).items() if count > 1]
    if duplicates:
        fail("duplicate node IDs: " + ", ".join(map(str, duplicates)))

    id_set = set(ids)

    width = canvas["width"]
    height = canvas["height"]
    for node in nodes:
        if any(not isinstance(node.get(axis), (int, float)) or isinstance(node.get(axis), bool)
               or not math.isfinite(node[axis]) for axis in ("x", "y", "radius")) or node["radius"] <= 0:
            fail(f"node {node.get('id')} has invalid coordinates")

        if not (-200 <= node["x"] <= width + 200 and -200 <= node["y"] <= height + 200):
            fail(f"node {node.get('id')} lies implausibly outside the canvas")

        if node.get("visibility") == "private-abstract" and node.get("url") is not None:
            fail(f"private abstract node {node.get('id')} must not expose a URL")

    for edge in edges:
        if edge.get("from") not in id_set or edge.get("to") not in id_set:
            fail(f"edge references unknown node: {edge}")

    expected_packages = {
        p["id"] for publication in config.get("publications", []) for p in publication.get("packages", [])
    }

    for package in state.get("packages", []):
        if approved_discovered_package(package, config):
            expected_packages.add(package["id"])

    state_packages = {item.get("id") for item in state.get("packages", [])}
    package_nodes = {node["id"].removeprefix("package:") for node in nodes if node.get("kind") == "package"}

    if len(state_packages) != len(state.get("packages", [])):
        fail("state package identities must be unique")

    # Observations may be absent in offline captures; authored package nodes still exist.
    if not state_packages.issubset(expected_packages) or package_nodes != expected_packages:
        fail("state package list/package nodes differ from the approved package set")

    if state.get("stats", {}).get("package_count") != len(expected_packages):
        fail("state package_count is incorrect")

    expected_core_nodes = {f"project:{item['id']}" for item in config.get("projects", [])}
    missing = expected_core_nodes - id_set
    if missing:
        fail("state misses core nodes: " + ", ".join(sorted(missing)))

    return state


def validate_svg_payload(raw: str) -> None:
    """Reject executable SVG and external rendering resources, while allowing public links."""
    if re.search(r"<!\s*(?:DOCTYPE|ENTITY)", raw, re.IGNORECASE):
        fail("SVG declarations are forbidden")

    tree = ET.fromstring(raw)
    allowed = {
        "svg",
        "title",
        "desc",
        "defs",
        "style",
        "g",
        "a",
        "text",
        "tspan",
        "path",
        "rect",
        "circle",
        "ellipse",
        "line",
        "polyline",
        "polygon",
        "linearGradient",
        "radialGradient",
        "stop",
        "pattern",
        "clipPath",
        "mask",
        "filter",
        "feGaussianBlur",
        "feMerge",
        "feMergeNode",
        "feColorMatrix",
        "feBlend",
        "feComposite",
        "feFlood",
        "feOffset",
        "animate",
        "animateTransform",
        "animateMotion",
        "mpath",
        "use",
    }

    for element in tree.iter():
        tag = local_name(element.tag)
        if tag not in allowed or not element.tag.startswith("{http://www.w3.org/2000/svg}"):
            fail(f"forbidden SVG element: {tag}")

        for attribute, value in element.attrib.items():
            name = local_name(attribute).lower()
            if name.startswith("on") or name in {"src", "base"}:
                fail(f"forbidden SVG attribute: {name}")

            if name == "href" and any(ord(character) < 32 for character in value):
                fail("control characters in SVG link")

            if name == "href" and value and not value.startswith("#"):
                link = urlsplit(value)
                if tag != "a" or link.scheme != "https" or not link.hostname or link.username or link.password:
                    fail("external SVG resource or unsafe anchor")

            if name == "attributename" and value.lower() not in {"opacity", "transform", "r", "stroke-dashoffset"}:
                fail("animation target is outside the presentation allowlist")

            # Human-readable labels and URLs are not CSS; inspect only presentation contexts.
            if name in {"style", "fill", "stroke", "filter", "clip-path", "mask", "cursor",
                        "marker", "marker-start", "marker-mid", "marker-end"}:
                validate_css_value(value)

        if tag == "style":
            validate_css_value(element.text or "")


def validate_css_value(value: str) -> None:
    """Allow only fragment URL references and prohibit CSS escape-based obfuscation."""
    unsafe = r"[\\]|/\*|@import|(?:expression|image-set|image|cross-fade|paint)\s*\(|javascript:|data:"
    if re.search(unsafe, value, re.IGNORECASE):
        fail("unsafe or obfuscated SVG style value")

    for match in re.finditer(r"url\s*\((.*?)\)", value, re.IGNORECASE | re.DOTALL):
        target = match.group(1).strip().strip("\"'")
        if not re.fullmatch(r"#[A-Za-z_][A-Za-z0-9_.:-]*", target):
            fail("external SVG CSS resource")


def validate_svg(root: Path, config: dict[str, Any], assets: Path | None = None) -> None:
    """Check dimensions, accessible labels, payload policy and motion variants."""
    assets = assets or root / "assets"
    state = load_json(assets / "profile-state.json")
    namespace = "{http://www.w3.org/2000/svg}"
    for name in ("sourcefield.dark.svg", "sourcefield.light.svg", "sourcefield.static.svg"):
        path = assets / name
        raw = path.read_text(encoding="utf-8")
        if len(raw.encode("utf-8")) > 900_000:
            fail(f"{name} exceeds the 900 KB README budget")

        try:
            tree = ET.fromstring(raw)
        except ET.ParseError as error:
            fail(f"invalid SVG {name}: {error}")
        if local_name(tree.tag) != "svg":
            fail(f"{name} root element is not svg")

        if int(tree.attrib.get("width", "0")) != config["render"]["width"]:
            fail(f"{name} width differs from config")

        if int(tree.attrib.get("height", "0")) != state["canvas"]["height"]:
            fail(f"{name} height differs from generated state")

        if tree.find(f"{namespace}title") is None or tree.find(f"{namespace}desc") is None:
            fail(f"{name} requires accessible title and description")

        validate_svg_payload(raw)
        motion = "<animate" in raw or "@keyframes" in raw
        if name.endswith("static.svg") and motion:
            fail("sourcefield.static.svg must be motion-free")

        if not name.endswith("static.svg") and not motion:
            fail(f"{name} is expected to contain declarative motion")


def validate_readme_and_site(root: Path, config: dict[str, Any], assets: Path | None = None,
                            docs: Path | None = None, readme_path: Path | None = None) -> None:
    """Check local runtime references and README fallback links."""
    docs = docs or root / "docs"
    assets = assets or root / "assets"
    readme_path = readme_path or root / "README.md"
    readme = readme_path.read_text(encoding="utf-8")
    references = [Path(os.path.relpath(assets / name, readme_path.parent)).as_posix() for name in SVG_FILES]
    references.append(config["profile"]["pages_url"])
    for reference in references:
        if reference not in readme:
            fail(f"README misses {reference}")

    if "prefers-reduced-motion" not in readme:
        fail("README must provide a reduced-motion source")

    html_path = docs / "index.html"
    html = html_path.read_text(encoding="utf-8")
    parser = HtmlResources()
    parser.feed(html)
    if parser.scripts != ["./app.js"]:
        fail(f"Pages scripts must remain local; found {parser.scripts}")

    if parser.styles != ["./app.css"]:
        fail(f"Pages stylesheet must remain local; found {parser.styles}")

    if "https://" in " ".join(parser.scripts + parser.styles):
        fail("Pages runtime resources must not use a CDN")

    for relative in ("app.js", "app.css", "simulation-fallback.js"):
        if not (docs / relative).is_file():
            fail(f"missing browser resource {relative}")


def validate_public_secret_surface(root: Path, assets: Path | None = None,
                                   docs: Path | None = None, config_path: Path | None = None) -> None:
    """Detect common GitHub token signatures; this is not a general secret detector."""
    patterns = [
        re.compile(r"ghp_[A-Za-z0-9]{20,}"),
        re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
        re.compile(r"gho_[A-Za-z0-9]{20,}"),
    ]

    roots = [assets or root / "assets", docs or root / "docs", (config_path or root / "config/profile.toml").parent]
    for base in roots:
        for path in base.rglob("*"):
            if not path.is_file() or path.suffix.lower() in {".png", ".wasm", ".zip"}:
                continue

            text = path.read_text(encoding="utf-8", errors="ignore")
            for pattern in patterns:
                if pattern.search(text):
                    fail(f"possible token found in public artifact: {path}")


def validate_workflows(root: Path) -> None:
    """Check workflow source conventions without claiming execution evidence."""
    update = (root / ".github/workflows/update-profile.yml").read_text(encoding="utf-8")
    validate = (root / ".github/workflows/validate.yml").read_text(encoding="utf-8")
    for action in (
        "actions/checkout",
        "actions/configure-pages",
        "actions/upload-pages-artifact",
        "actions/deploy-pages",
    ):
        references = re.findall(rf"uses:\s*{re.escape(action)}@([^\s]+)", update + "\n" + validate)
        if not references or any(not re.fullmatch(r"[0-9a-f]{40}", reference) for reference in references):
            fail(f"workflow requires immutable full commit references for {action}")

    if "cargo generate-lockfile" in update or "cargo generate-lockfile" in validate:
        fail("workflows must consume the committed Cargo.lock")

    if "PROFILE_TOKEN:" not in update:
        fail("optional PROFILE_TOKEN mapping is missing")


def validate_wasm(root: Path, require_wasm: bool, docs: Path | None = None) -> str | None:
    """Check the module header; runtime behavior is covered by browser tests."""
    path = (docs or root / "docs") / "pkg/sourcefield_wasm_bg.wasm"
    if not path.is_file():
        if require_wasm:
            fail("compiled WebAssembly is required but docs/pkg/sourcefield_wasm_bg.wasm is missing")

        return (
            "compiled WebAssembly is not checked in; the local JavaScript bootstrap works "
            "immediately and the update workflow builds the Rust module"
        )

    with path.open("rb") as handle:
        payload = handle.read(8)
    if len(payload) < 8 or payload[:4] != b"\x00asm":
        fail("docs/pkg/sourcefield_wasm_bg.wasm is not a valid WebAssembly binary")

    if payload[4:8] != b"\x01\x00\x00\x00":
        fail("docs/pkg/sourcefield_wasm_bg.wasm uses an unsupported WebAssembly binary version")

    return None


def write_report(
    path: Path,
    root: Path,
    state: dict[str, Any] | None,
    passed: list[str],
    warnings: list[str],
    failure: str | None,
) -> None:
    """Write only checks actually executed by this artifact validator."""
    semantic_hash = state.get("semantic_hash", "UNAVAILABLE") if state else "UNAVAILABLE"
    nodes = len(state.get("nodes", [])) if state else 0
    edges = len(state.get("edges", [])) if state else 0
    packages = state.get("stats", {}).get("package_count", 0) if state else 0
    result = "FAIL" if failure else "PASS"

    lines = [
        "# Validation report",
        "",
        "This report was generated by `scripts/validate_artifact.py`.",
        "",
        f"- Result: **{result}**",
        f"- Semantic state: `{semantic_hash}`",
        f"- Files validated from: `{root.name}`",
        f"- Passed checks: {len(passed)}",
        f"- Warnings: {len(warnings)}",
        f"- Failures: {1 if failure else 0}",
        "",
        "## Artifact summary",
        "",
        f"- Nodes: {nodes}",
        f"- Edges: {edges}",
        f"- NuGet packages: {packages}",
        "",
        "## Passed",
        "",
    ]

    lines.extend(f"- {item}" for item in passed)
    if not passed:
        lines.append("- None")

    lines.extend(["", "## Warnings", ""])
    lines.extend(f"- {item}" for item in warnings)
    if not warnings:
        lines.append("- None")

    lines.extend(["", "## Failures", ""])
    lines.append(f"- {failure}" if failure else "- None")
    lines.extend(
        [
            "",
            "## Verification boundary",
            "",
            "This command checks artifact structure and selected source policies only. "
            "It does not execute JavaScript, compile Rust, validate archive integrity, or prove "
            "browser behavior. scripts/verify.sh enforces native checks; scripts/build-wasm.sh "
            "builds the release WebAssembly module.",
            "",
        ]
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    """Run artifact checks and optionally persist a scope-limited report."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--report", type=Path)
    parser.add_argument(
        "--require-wasm",
        action="store_true",
        help="Fail when the compiled wasm-pack binary is not present.",
    )
    parser.add_argument(
        "--approved-profile", type=Path, metavar="CONTRACT", help="Enforce an independent editorial JSON contract."
    )
    parser.add_argument("--workflow-root", type=Path, help="Also verify consumer workflow source controls.")
    parser.add_argument("--config", type=Path, default=Path("config/profile.toml"))
    parser.add_argument("--assets", type=Path, default=Path("assets"))
    parser.add_argument("--docs", type=Path, default=Path("docs"))
    parser.add_argument("--readme", type=Path, default=Path("README.md"))
    args = parser.parse_args()
    root = args.root.resolve()
    assets = root / args.assets
    docs = root / args.docs
    config_path = root / args.config
    readme = root / args.readme
    report = args.report
    if report is not None and not report.is_absolute():
        report = root / report

    passed: list[str] = []
    warnings: list[str] = []
    state: dict[str, Any] | None = None
    failure: str | None = None

    try:
        require_files(root, assets, docs, config_path, readme)
        passed.append("required generated artifact files exist and do not traverse symlinks")

        config = effective_config(validate_config(root, config_path), assets)
        if args.approved_profile:
            validate_approved_profile(config, load_json(args.approved_profile))

        passed.append("configuration structure is valid")
        if args.approved_profile:
            passed.append("consumer facts match the supplied independent editorial contract")

        state = validate_state(root, config, assets, docs)
        passed.extend(
            [
                "README and Pages use a byte-identical semantic state",
                f"all {len(state['nodes'])} node identifiers are unique and all {len(state['edges'])} edges resolve",
                "private abstract nodes expose no repository URL; manually approved descriptions remain public",
                f"the configured set of {state['stats']['package_count']} package nodes is present",
            ]
        )

        validate_svg(root, config, assets)
        passed.append("SVG variants satisfy dimensions, accessible-label and local-resource policy checks")

        validate_readme_and_site(root, config, assets, docs, readme)
        passed.append("README fallbacks and GitHub Pages runtime resources are local and complete")

        validate_public_secret_surface(root, assets, docs, config_path)
        passed.append("no common GitHub token signatures were detected in generated public data")

        if args.workflow_root:
            validate_workflows(args.workflow_root.resolve())
            passed.append("workflow source actions use immutable pins and the committed lockfile")

        wasm_warning = validate_wasm(root, args.require_wasm, docs)
        if wasm_warning:
            warnings.append(wasm_warning)
        else:
            passed.append("the local WebAssembly binary has a valid module header")

    except (AssertionError, KeyError, TypeError, ValueError, OSError, tomllib.TOMLDecodeError) as error:
        failure = str(error)
        if report is not None:
            write_report(report, root, state, passed, warnings, failure)

        print(f"VALIDATION FAILED: {error}", file=sys.stderr)
        return 1

    if report is not None:
        write_report(report, root, state, passed, warnings, None)

    assert state is not None
    print(
        "VALIDATION OK: "
        f"{len(state['nodes'])} nodes, {len(state['edges'])} edges, "
        f"{state['stats']['package_count']} packages, state {state['semantic_hash']}"
    )
    for warning in warnings:
        print(f"VALIDATION WARNING: {warning}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
