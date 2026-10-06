"""Verify fixed-endpoint fetch policy without real network requests."""

from __future__ import annotations

import unittest
from unittest.mock import patch
from urllib.request import Request

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
from support.tool_versions import RegistryResponse, payload
import tool_versions


class RegistryFetchTests(unittest.TestCase):
    """Keep bounded reads, endpoint admission and redirect refusal independently tested."""

    def test_fetch_uses_fixed_endpoint_timeout_and_bounded_read(self):
        response = RegistryResponse(payload())
        with patch.object(tool_versions, "build_opener") as factory:
            factory.return_value.open.return_value = response

            result = tool_versions.fetch_registry_status("0.15.0")

        request = factory.return_value.open.call_args.args[0]
        self.assertEqual(result, tool_versions.RegistryStatus("0.15.0", False))
        self.assertEqual(request.full_url, tool_versions.REGISTRY)
        self.assertEqual(factory.return_value.open.call_args.kwargs, {"timeout": 15})
        self.assertIsInstance(factory.call_args.args[0], tool_versions.RejectRedirects)
        self.assertEqual(response.read_sizes, [tool_versions.MAX_RESPONSE_BYTES + 1])

    def test_fetch_rejects_undeclared_oversized_stream(self):
        response = RegistryResponse(b" " * (tool_versions.MAX_RESPONSE_BYTES + 1))
        with patch.object(tool_versions, "build_opener") as factory:
            factory.return_value.open.return_value = response

            with self.assertRaisesRegex(ValueError, "exceeds 1 MiB"):
                tool_versions.fetch_registry_status("0.15.0")

        self.assertEqual(response.read_sizes, [tool_versions.MAX_RESPONSE_BYTES + 1])

    def assert_fetch_rejected_before_read(self, response: RegistryResponse) -> None:
        """Exercise one invalid response header or identity without consuming response bytes."""
        # Arrange
        with patch.object(tool_versions, "build_opener") as factory:
            factory.return_value.open.return_value = response

            # Act
            with self.assertRaises(ValueError):
                tool_versions.fetch_registry_status("0.15.0")

        # Assert
        self.assertEqual(response.read_sizes, [])

    def test_fetch_rejects_redirected_endpoint(self):
        """Reject a response from another endpoint before reading its body."""
        self.assert_fetch_rejected_before_read(RegistryResponse(payload(), url="https://example.com/redirect"))

    def test_fetch_rejects_error_status(self):
        """Reject an HTTP error before reading its body."""
        self.assert_fetch_rejected_before_read(RegistryResponse(payload(), status=500))

    def test_fetch_rejects_unbounded_content_length(self):
        """Reject unbounded declared body length before reading."""
        self.assert_fetch_rejected_before_read(RegistryResponse(payload(), **{"Content-Length": "999999999999999"}))

    def test_fetch_rejects_oversized_content_length(self):
        """Reject body length just above the admission bound before reading."""
        self.assert_fetch_rejected_before_read(RegistryResponse(payload(), **{"Content-Length": "1048577"}))

    def test_fetch_rejects_negative_content_length(self):
        """Reject a negative declared body length before reading."""
        self.assert_fetch_rejected_before_read(RegistryResponse(payload(), **{"Content-Length": "-1"}))

    def assert_redirect_rejected(self, location: str) -> None:
        """Probe one redirect target without relying on an opener or network request."""
        # Arrange
        handler = tool_versions.RejectRedirects()
        request = Request(tool_versions.REGISTRY)

        # Act / Assert
        with self.assertRaisesRegex(ValueError, "redirects"):
            handler.redirect_request(request, None, 302, "Found", {}, location)

    def test_redirect_rejects_another_host(self):
        """Refuse redirects to another host."""
        self.assert_redirect_rejected("https://example.com/")

    def test_redirect_rejects_http_downgrade(self):
        """Refuse redirects that downgrade HTTPS."""
        self.assert_redirect_rejected("http://crates.io/")

    def test_redirect_rejects_same_endpoint(self):
        """Refuse all redirects, including back to the fixed endpoint."""
        self.assert_redirect_rejected(tool_versions.REGISTRY)


if __name__ == "__main__":
    unittest.main()
