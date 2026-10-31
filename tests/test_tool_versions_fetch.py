"""Exercise bounded official metadata and checksum archive transport without network."""

from __future__ import annotations

import json
import unittest
from unittest.mock import patch
from urllib.request import Request

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
from support.tool_versions import Response, release
from sourcefield_tools import binaryen


class BinaryenTransportTests(unittest.TestCase):
    """Keep HTTPS origins, redirects and body bounds explicit at the shared download boundary."""

    def test_metadata_uses_fixed_endpoint_and_bounded_read(self):
        body = json.dumps(release()).encode("ascii")
        response = Response(body)
        with patch.object(binaryen, "open_response", return_value=response) as opened:
            document = binaryen.fetch_json(binaryen.API + "/latest")

        self.assertEqual(document["tag_name"], "version_133")
        opened.assert_called_once_with(binaryen.API + "/latest", asset=False)
        self.assertEqual(response.read_sizes, [1024 * 1024 + 1])

    def test_arbitrary_metadata_endpoint_is_rejected(self):
        with patch.object(binaryen, "open_response") as opened:
            with self.assertRaisesRegex(ValueError, "fixed official"):
                binaryen.fetch_json("https://api.github.com/repos/attacker/binaryen/releases/latest")

        opened.assert_not_called()

    def test_oversized_metadata_is_rejected(self):
        response = Response(b" " * (1024 * 1024 + 1))
        with patch.object(binaryen, "open_response", return_value=response):
            with self.assertRaisesRegex(ValueError, "exceeds 1 MiB"):
                binaryen.fetch_json(binaryen.API + "/latest")

    def assert_response_rejected(self, response, message):
        """Reject one response's metadata before reading its body."""
        with patch.object(binaryen, "open_response", return_value=response):
            with self.assertRaisesRegex(ValueError, message):
                binaryen.fetch_json(binaryen.API + "/latest")

        self.assertEqual(response.read_sizes, [])

    def test_wrong_response_host_is_rejected(self):
        self.assert_response_rejected(Response(b"{}", url="https://attacker.invalid/"), "approved origin")

    def test_changed_api_path_is_rejected(self):
        self.assert_response_rejected(Response(b"{}", url=binaryen.API + "/other"), "endpoint changed")

    def test_http_error_is_rejected(self):
        self.assert_response_rejected(Response(b"{}", status=500), "status 200")

    def test_oversized_content_length_is_rejected(self):
        self.assert_response_rejected(Response(b"{}", **{"Content-Length": "1048577"}), "admitted size")

    def test_nondecimal_content_length_is_rejected(self):
        self.assert_response_rejected(Response(b"{}", **{"Content-Length": "-1"}), "bounded decimal")

    def test_transport_has_no_ambient_proxy_and_uses_timeout(self):
        with patch.object(binaryen, "build_opener") as factory:
            binaryen.open_response(binaryen.API + "/latest", asset=False)

        handlers = factory.call_args.args
        self.assertEqual(handlers[0].proxies, {})
        self.assertIsInstance(handlers[1], binaryen.AssetRedirects)
        self.assertEqual(factory.return_value.open.call_args.kwargs, {"timeout": 30})

    def test_release_cdn_redirect_is_admitted_without_credentials(self):
        handler = binaryen.AssetRedirects(True)
        request = Request(
            "https://github.com/archive", headers={"Authorization": "private", "Proxy-Authorization": "private"}
        )

        redirect = handler.redirect_request(
            request, None, 302, "Found", {}, "https://release-assets.githubusercontent.com/file?sig=abc"
        )

        self.assertEqual(redirect.full_url, "https://release-assets.githubusercontent.com/file?sig=abc")
        self.assertIsNone(redirect.get_header("Authorization"))
        self.assertIsNone(redirect.get_header("Proxy-authorization"))

    def assert_redirect_rejected(self, destination, *, asset=True):
        """Submit one unsafe redirect to the actual pre-request origin guard."""
        handler = binaryen.AssetRedirects(asset)
        request = Request("https://github.com/archive")

        with self.assertRaises(ValueError):
            handler.redirect_request(request, None, 302, "Found", {}, destination)

    def test_metadata_redirect_is_rejected(self):
        self.assert_redirect_rejected(binaryen.API + "/latest", asset=False)

    def test_foreign_redirect_is_rejected(self):
        self.assert_redirect_rejected("https://attacker.invalid/file")

    def test_https_downgrade_is_rejected(self):
        self.assert_redirect_rejected("http://release-assets.githubusercontent.com/file")

    def test_credential_bearing_redirect_is_rejected(self):
        self.assert_redirect_rejected("https://private@release-assets.githubusercontent.com/file")

    def test_nonstandard_port_is_rejected(self):
        self.assert_redirect_rejected("https://release-assets.githubusercontent.com:444/file")

    def test_redirect_loop_is_bounded(self):
        handler = binaryen.AssetRedirects(True)
        handler.count = 4

        with self.assertRaisesRegex(ValueError, "redirects"):
            handler.redirect_request(
                Request("https://github.com/archive"), None, 302, "Found", {}, "https://github.com/archive"
            )


if __name__ == "__main__":
    unittest.main()
