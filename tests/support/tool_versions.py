"""Provide the synthetic registry bytes and HTTP response used by tool-version tests."""

import io
import json

import tool_versions


def payload(version: str = "0.15.0", **crate_fields: object) -> bytes:
    """Return the official endpoint's relevant crate and release fields."""
    crate = {"id": "wasm-pack", "max_stable_version": version, **crate_fields}

    return json.dumps({"crate": crate, "versions": [{"num": version, "yanked": False}]}).encode("ascii")


class RegistryResponse(io.BytesIO):
    """Record body reads while exposing the response metadata admitted by the fetch helper."""

    def __init__(self, body: bytes, url: str = tool_versions.REGISTRY, status: int = 200, **headers: str):
        """Initialize one isolated response with explicit URL, status and headers."""
        super().__init__(body)
        self.url = url
        self.status = status
        self.headers = headers
        self.read_sizes: list[int] = []

    def geturl(self) -> str:
        """Expose the final endpoint URL without making a request."""

        return self.url

    def read(self, size: int = -1) -> bytes:
        """Record the requested bound before consuming the synthetic body."""
        self.read_sizes.append(size)

        return super().read(size)
