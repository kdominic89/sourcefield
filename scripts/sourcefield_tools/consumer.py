"""Portable path rules shared by consumer generation and publication."""

from pathlib import PurePosixPath


def relative_path(value: str) -> str:
    """Allow only normalized consumer-relative paths, including both README destinations."""
    # Repository paths are a portable wire format, not host-native path strings.
    path = PurePosixPath(value)
    reason = None
    if ":" in value:
        reason = "colon is not permitted in portable repository paths"
    elif "\\" in value:
        reason = "backslash is not permitted; use portable forward-slash separators"
    elif any(ord(character) < 32 for character in value):
        reason = "control characters are not permitted"
    elif path.is_absolute():
        reason = "absolute paths are not permitted"
    elif any(part.casefold() in {"..", ".git"} for part in path.parts):
        reason = "parent traversal and Git metadata components are not permitted"
    elif not path.parts or str(path) != value:
        reason = "expected a nonempty normalized consumer-relative path"

    if reason is not None:
        raise ValueError(f"invalid consumer-relative path {value!r}: {reason}")

    return value
