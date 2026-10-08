"""Check the current consumer workflow source conventions with the standard library.

These checks inspect block-mapping, literal ``uses`` scalars and ignore YAML block-scalar
bodies. They do not parse or validate arbitrary YAML; workflow execution and YAML syntax
remain GitHub's responsibility. ``check_pin`` separately authenticates lock/pin equality.
"""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path
import re


GENERATOR = "kdominic89/sourcefield/.github/workflows/generate.yml"
USES_KEY = re.compile(r"^\s*(?:-\s+)?(?:uses|\"uses\"|'uses')\s*:\s*(.*?)\s*$")
BLOCK_SCALAR = re.compile(r"^.+?:\s*[|>](?:[+-][1-9]?|[1-9][+-]?)?(?:\s+#.*)?\s*$")
REMOTE_REFERENCE = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_./-]+)?@[0-9a-f]{40}")
CONTAINER_REFERENCE = re.compile(r"docker://[^@\s]+@sha256:[0-9a-f]{64}")


def _literal_reference(value: str, path: Path, number: int) -> str:
    """Read the plain or quoted one-line reference supported by the consumer template."""
    if value.startswith(("'", '"')):
        quote = value[0]
        closing = value.find(quote, 1)
        tail = value[closing + 1:].strip() if closing != -1 else ""

        if closing == -1 or (tail and not tail.startswith("#")):
            raise AssertionError(f"workflow requires a literal uses reference: {path}:{number}")

        reference = value[1:closing]
    else:
        reference = value.split("#", 1)[0].strip()

    if not reference or reference.startswith(("|", ">")) or any(part.isspace() for part in reference):
        raise AssertionError(f"workflow requires a literal uses reference: {path}:{number}")

    return reference


def _uses_references(text: str, path: Path) -> Iterator[str]:
    """Yield declared literal references without interpreting comments or shell block content."""
    block_indent: int | None = None

    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.lstrip(" ")

        if not stripped or stripped.startswith("#"):
            continue

        indent = len(line) - len(stripped)

        if block_indent is not None:
            if indent > block_indent:
                continue

            block_indent = None

        match = USES_KEY.fullmatch(line)

        if match is not None:
            yield _literal_reference(match.group(1), path, number)
        elif BLOCK_SCALAR.fullmatch(stripped):
            # The dash owns a sequence entry; its mapping key is two columns farther in.
            block_indent = indent + (2 if stripped.startswith("- ") else 0)


def _validate_reference(reference: str) -> None:
    """Require immutable remote identities while allowing checkout-local action implementations."""
    if reference.startswith("./"):
        return

    if reference.startswith("docker://"):
        if CONTAINER_REFERENCE.fullmatch(reference) is None:
            raise AssertionError(f"container action requires an immutable digest: {reference}")

        return

    if REMOTE_REFERENCE.fullmatch(reference) is None:
        raise AssertionError(f"workflow requires immutable full commit references: {reference}")


def validate_workflows(root: Path) -> None:
    """Check the current pinned reusable consumer contract and an optional validation workflow.

    ``update-profile.yml`` must declare exactly one Sourcefield generator. Every present remote
    action/workflow reference is immutable; local actions are allowed. Pinned configure-pages
    steps and explicitly forwarded optional PROFILE_TOKEN secrets are supported but not required.
    A separate ``validate.yml`` is optional because the distributed consumer template delegates
    generation checks upstream.
    """
    workflows = root / ".github/workflows"
    generator_count = 0

    for name in ("update-profile.yml", "validate.yml"):
        path = workflows / name

        if name == "validate.yml" and not path.exists():
            continue

        try:
            text = path.read_text(encoding="utf-8")
        except FileNotFoundError as error:
            raise AssertionError(f"consumer workflow file is missing: {path}") from error

        if "cargo generate-lockfile" in text:
            raise AssertionError("workflows must consume the committed Cargo.lock")

        for reference in _uses_references(text, path):
            _validate_reference(reference)

            if name == "update-profile.yml" and reference.partition("@")[0] == GENERATOR:
                generator_count += 1

    if generator_count != 1:
        raise AssertionError("workflow requires exactly one pinned Sourcefield reusable generator")
