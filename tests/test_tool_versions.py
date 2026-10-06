"""Exercise the pin authority and bounded registry/update boundaries without network access."""

from __future__ import annotations

import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.error import URLError
from urllib.request import Request

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import tool_versions


def payload(version: str = "0.15.0", **crate_fields) -> bytes:
    """Arrange the official endpoint's relevant crate and release fields."""
    crate = {"id": "wasm-pack", "max_stable_version": version, **crate_fields}

    return json.dumps({"crate": crate, "versions": [{"num": version, "yanked": False}]}).encode("ascii")


class RegistryResponse(io.BytesIO):
    """Model a bounded HTTP body with the metadata consumed by the actual fetch helper."""

    def __init__(self, body: bytes, url: str = tool_versions.REGISTRY, status: int = 200, **headers):
        super().__init__(body)
        self.url = url
        self.status = status
        self.headers = headers
        self.read_sizes = []

    def geturl(self):
        return self.url

    def read(self, size=-1):
        self.read_sizes.append(size)

        return super().read(size)


class ToolVersionTests(unittest.TestCase):
    """Keep one explicit action per case and isolate every pin mutation in a temporary root."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.pin = self.root / tool_versions.PIN
        self.pin.parent.mkdir()
        self.pin.write_text("0.15.0\n", encoding="ascii")

    def run_main(self, mode: str, latest: str = "0.15.0") -> tuple[int, str, str]:
        """Execute the public command while replacing only its registry and repository root."""
        document = json.loads(payload(latest))
        current = self.pin.read_text(encoding="ascii").strip()
        if latest != current:
            document["versions"].append({"num": current, "yanked": False})

        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(json.dumps(document).encode("ascii"))

            result = tool_versions.main([mode])

        return result, stdout.getvalue(), stderr.getvalue()

    def test_local_version_is_exact_and_offline(self):
        stdout = io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "fetch_registry_status") as registry,
            contextlib.redirect_stdout(stdout),
        ):
            result = tool_versions.main(["--version"])

        self.assertEqual((result, stdout.getvalue()), (0, "0.15.0\n"))
        registry.assert_not_called()

    def test_pin_rejects_noncanonical_or_unbounded_versions(self):
        invalid = [
            "0.15",
            "v0.15.0",
            "00.15.0",
            "0.15.0-rc.1",
            "0.15.0+build",
            " 0.15.0",
            "0.15.0\n\n",
            "0.15.0\r\n",
            "0.15.0",
            "1" * 65,
            "\u00e9.15.0\n",
        ]
        for value in invalid:
            with self.subTest(value=value):
                self.pin.write_bytes(value.encode("utf-8"))

                with self.assertRaises(ValueError):
                    tool_versions.read_pin(self.root)

    def test_pin_rejects_symlink_before_read_or_update(self):
        target = self.root / "external"
        target.write_text("0.15.0\n", encoding="ascii")
        self.pin.unlink()
        self.pin.symlink_to(target)

        with self.assertRaisesRegex(ValueError, "symlink"):
            tool_versions.update_pin(self.root, "0.15.0", "0.16.0")

        self.assertEqual(target.read_text(), "0.15.0\n")

    def test_pin_rejects_parent_symlink(self):
        self.pin.parent.rename(self.root / "actual-tools")
        self.pin.parent.symlink_to(self.root / "actual-tools", target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "symlink"):
            tool_versions.read_pin(self.root)

    def test_pin_rejects_nonregular_file(self):
        self.pin.unlink()
        self.pin.mkdir()

        with self.assertRaisesRegex(ValueError, "regular file"):
            tool_versions.read_pin(self.root)

    def test_current_check_is_read_only(self):
        before = self.pin.read_bytes()

        result, stdout, stderr = self.run_main("--check")

        self.assertEqual((result, stderr), (0, ""))
        self.assertIn("matches crates.io", stdout)
        self.assertEqual(self.pin.read_bytes(), before)

    def test_stale_check_is_actionable_and_read_only(self):
        before = self.pin.read_bytes()

        result, stdout, stderr = self.run_main("--check", "0.16.0")

        self.assertEqual((result, stdout), (1, ""))
        self.assertIn("python3 scripts/tool_versions.py --update", stderr)
        self.assertIn("reviewed pull request", stderr)
        self.assertEqual(self.pin.read_bytes(), before)

    def test_update_changes_only_pin(self):
        sibling = self.pin.parent / "untouched.json"
        sibling.write_text("{}\n", encoding="ascii")

        result, stdout, stderr = self.run_main("--update", "0.16.0")

        self.assertEqual((result, stderr), (0, ""))
        self.assertIn("0.15.0 -> 0.16.0", stdout)
        self.assertEqual(self.pin.read_bytes(), b"0.16.0\n")
        self.assertEqual(sibling.read_bytes(), b"{}\n")
        self.assertEqual(
            sorted(path.name for path in self.pin.parent.iterdir()), ["untouched.json", "wasm-pack-version.txt"]
        )

    def test_update_rejects_downgrade(self):
        before = self.pin.read_bytes()

        with self.assertRaisesRegex(ValueError, "refusing downgrade"):
            tool_versions.update_pin(self.root, "0.15.0", "0.14.0")

        self.assertEqual(self.pin.read_bytes(), before)
        self.assertEqual(list(self.pin.parent.iterdir()), [self.pin])

    def test_update_rejects_concurrent_pin_change_and_cleans_temporary(self):
        self.pin.write_text("0.17.0\n", encoding="ascii")

        with self.assertRaisesRegex(ValueError, "pin changed"):
            tool_versions.update_pin(self.root, "0.15.0", "0.16.0")

        self.assertEqual(self.pin.read_bytes(), b"0.17.0\n")
        self.assertEqual(list(self.pin.parent.iterdir()), [self.pin])

    def test_registry_accepts_matching_non_yanked_stable_release(self):
        source = payload()

        result = tool_versions.registry_status(source, "0.15.0")

        self.assertEqual(result, tool_versions.RegistryStatus("0.15.0", False))

    def test_registry_accepts_prereleases_and_yanked_history_without_promoting_them(self):
        document = json.loads(payload())
        document["versions"].extend([
            {"num": "0.16.0", "yanked": True},
            {"num": "0.17.0-rc.1", "yanked": False},
            {"num": "0.14.0+build.01", "yanked": False},
        ])
        source = json.dumps(document).encode("ascii")

        result = tool_versions.registry_status(source, "0.15.0")

        self.assertEqual(result, tool_versions.RegistryStatus("0.15.0", False))

    def test_registry_rejects_latest_below_non_yanked_current_pin(self):
        document = json.loads(payload("0.14.0"))
        document["versions"].append({"num": "0.15.0", "yanked": False})
        source = json.dumps(document).encode("ascii")

        with self.assertRaisesRegex(ValueError, "inconsistent: latest stable"):
            tool_versions.registry_status(source, "0.15.0")

    def test_registry_rejects_malformed_historical_release_numbers(self):
        numbers = [None, 16, "", "v0.14.0", "00.14.0", "0.14.0-01", "0.14.0-rc..1",
                   "0.14.0+", "0.14.0+build..1", "0.14.0-" + "x" * 128]
        for number in numbers:
            with self.subTest(number=number):
                document = json.loads(payload())
                document["versions"].append({"num": number, "yanked": False})
                source = json.dumps(document).encode("ascii")

                with self.assertRaisesRegex(ValueError, "inconsistent"):
                    tool_versions.registry_status(source, "0.15.0")

    def test_registry_rejects_wrong_identity_invalid_versions_and_bad_shapes(self):
        invalid = [
            b"not JSON",
            b"[]",
            b"{}",
            payload(id="other"),
            payload("0.16.0-beta.1"),
            payload("0.16.0+build"),
            payload("00.16.0"),
            payload("9999999999.0.0"),
            payload(max_stable_version=None),
            payload(max_stable_version=16),
        ]
        for source in invalid:
            with self.subTest(source=source), self.assertRaises((ValueError, TypeError)):
                tool_versions.registry_status(source, "0.15.0")

    def test_registry_rejects_yanked_missing_duplicate_and_mass_inventory(self):
        base = json.loads(payload())
        inventories = [
            None,
            [],
            [{"num": "0.14.0", "yanked": False}],
            [{"num": "0.15.0", "yanked": True}],
            [{"num": "0.15.0", "yanked": 0}],
            base["versions"] * 2,
            base["versions"] * 513,
        ]
        for versions in inventories:
            with self.subTest(versions=versions):
                source = json.dumps({**base, "versions": versions}).encode("ascii")

                with self.assertRaises((ValueError, TypeError)):
                    tool_versions.registry_status(source, "0.15.0")

    def test_yanked_pin_with_lower_latest_is_explicit_and_requires_manual_review(self):
        source = json.dumps({"crate": {"id": "wasm-pack", "max_stable_version": "0.14.0"}, "versions": [
            {"num": "0.15.0", "yanked": True}, {"num": "0.14.0", "yanked": False},
        ]}).encode("ascii")
        for mode in ("--check", "--update"):
            with self.subTest(mode=mode):
                stdout, stderr = io.StringIO(), io.StringIO()
                with (
                    patch.object(tool_versions, "ROOT", self.root),
                    patch.object(tool_versions, "build_opener") as factory,
                    contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
                ):
                    factory.return_value.open.return_value = RegistryResponse(source)

                    result = tool_versions.main([mode])

                self.assertEqual((result, stdout.getvalue()), (1, ""))
                self.assertIn("pin 0.15.0 is yanked", stderr.getvalue())
                self.assertIn("latest stable is 0.14.0", stderr.getvalue())
                self.assertIn("refusing downgrade", stderr.getvalue())
                self.assertIn("manually edit tools/wasm-pack-version.txt", stderr.getvalue())
                self.assertIn("reviewed pull request", stderr.getvalue())
                self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")
                factory.return_value.open.assert_called_once()

    def test_yanked_pin_with_newer_latest_checks_without_mutation(self):
        source = json.dumps({"crate": {"id": "wasm-pack", "max_stable_version": "0.16.0"}, "versions": [
            {"num": "0.15.0", "yanked": True}, {"num": "0.16.0", "yanked": False},
        ]}).encode("ascii")
        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(source)

            result = tool_versions.main(["--check"])

        self.assertEqual((result, stdout.getvalue()), (1, ""))
        self.assertIn("pin 0.15.0 is yanked", stderr.getvalue())
        self.assertIn("latest stable is 0.16.0", stderr.getvalue())
        self.assertIn("python3 scripts/tool_versions.py --update", stderr.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")
        factory.return_value.open.assert_called_once()

    def test_yanked_pin_with_newer_latest_updates_with_warning(self):
        source = json.dumps({"crate": {"id": "wasm-pack", "max_stable_version": "0.16.0"}, "versions": [
            {"num": "0.15.0", "yanked": True}, {"num": "0.16.0", "yanked": False},
        ]}).encode("ascii")
        stdout, stderr = io.StringIO(), io.StringIO()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "build_opener") as factory,
            contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
        ):
            factory.return_value.open.return_value = RegistryResponse(source)

            result = tool_versions.main(["--update"])

        self.assertEqual(result, 0)
        self.assertIn("Warning: wasm-pack pin 0.15.0 is yanked", stderr.getvalue())
        self.assertIn("0.15.0 -> 0.16.0", stdout.getvalue())
        self.assertEqual(self.pin.read_bytes(), b"0.16.0\n")
        factory.return_value.open.assert_called_once()

    def test_missing_pin_fails_closed_with_distinct_diagnostic(self):
        for mode in ("--check", "--update"):
            with self.subTest(mode=mode):
                stdout, stderr = io.StringIO(), io.StringIO()
                with (
                    patch.object(tool_versions, "ROOT", self.root),
                    patch.object(tool_versions, "build_opener") as factory,
                    contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr),
                ):
                    factory.return_value.open.return_value = RegistryResponse(payload("0.16.0"))

                    result = tool_versions.main([mode])

                self.assertEqual((result, stdout.getvalue()), (1, ""))
                self.assertIn("pin 0.15.0 is missing from", stderr.getvalue())
                self.assertIn("latest stable is 0.16.0", stderr.getvalue())
                self.assertNotIn("is yanked", stderr.getvalue())
                self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")

    def test_inconsistent_pin_inventory_is_rejected_before_mutation(self):
        invalid_entries = [
            [{"num": "0.15.0", "yanked": False}, {"num": "0.15.0", "yanked": True}],
            [{"num": "0.15.0", "yanked": "false"}],
            [{"num": "0.15.0"}],
            [{"num": "0.17.0", "yanked": False}],
            [{"num": "not-a-version", "yanked": False}],
            [None],
        ]
        for entries in invalid_entries:
            with self.subTest(entries=entries):
                self.pin.write_text("0.15.0\n", encoding="ascii")
                document = json.loads(payload("0.16.0"))
                document["versions"].extend(entries)
                stderr = io.StringIO()
                with (
                    patch.object(tool_versions, "ROOT", self.root),
                    patch.object(tool_versions, "build_opener") as factory,
                    contextlib.redirect_stderr(stderr),
                ):
                    factory.return_value.open.return_value = RegistryResponse(json.dumps(document).encode("ascii"))

                    result = tool_versions.main(["--update"])

                self.assertEqual(result, 1)
                self.assertIn("inconsistent", stderr.getvalue())
                self.assertEqual(self.pin.read_bytes(), b"0.15.0\n")

    def test_registry_rejects_mass_body_before_parsing(self):
        source = b" " * (tool_versions.MAX_RESPONSE_BYTES + 1)

        with self.assertRaisesRegex(ValueError, "exceeds 1 MiB"):
            tool_versions.registry_status(source, "0.15.0")

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

    def test_fetch_rejects_location_status_and_declared_oversized_body_before_read(self):
        responses = [
            RegistryResponse(payload(), url="https://example.com/redirect"),
            RegistryResponse(payload(), status=500),
            RegistryResponse(payload(), **{"Content-Length": "999999999999999"}),
            RegistryResponse(payload(), **{"Content-Length": "1048577"}),
            RegistryResponse(payload(), **{"Content-Length": "-1"}),
        ]
        for response in responses:
            with self.subTest(url=response.url, headers=response.headers, status=response.status):
                with patch.object(tool_versions, "build_opener") as factory:
                    factory.return_value.open.return_value = response

                    with self.assertRaises(ValueError):
                        tool_versions.fetch_registry_status("0.15.0")

                self.assertEqual(response.read_sizes, [])

    def test_fetch_rejects_undeclared_oversized_stream(self):
        response = RegistryResponse(b" " * (tool_versions.MAX_RESPONSE_BYTES + 1))
        with patch.object(tool_versions, "build_opener") as factory:
            factory.return_value.open.return_value = response

            with self.assertRaisesRegex(ValueError, "exceeds 1 MiB"):
                tool_versions.fetch_registry_status("0.15.0")

        self.assertEqual(response.read_sizes, [tool_versions.MAX_RESPONSE_BYTES + 1])

    def test_redirect_handler_refuses_other_hosts_and_downgrade(self):
        handler = tool_versions.RejectRedirects()
        request = Request(tool_versions.REGISTRY)
        locations = ["https://example.com/", "http://crates.io/", tool_versions.REGISTRY]
        for location in locations:
            with self.subTest(location=location), self.assertRaisesRegex(ValueError, "redirects"):
                handler.redirect_request(request, None, 302, "Found", {}, location)

    def test_network_failure_is_concise_and_preserves_pin(self):
        stderr = io.StringIO()
        before = self.pin.read_bytes()
        with (
            patch.object(tool_versions, "ROOT", self.root),
            patch.object(tool_versions, "fetch_registry_status", side_effect=URLError("offline")),
            contextlib.redirect_stderr(stderr),
        ):
            result = tool_versions.main(["--update"])

        self.assertEqual(result, 1)
        self.assertIn("offline", stderr.getvalue())
        self.assertEqual(len(stderr.getvalue().splitlines()), 1)
        self.assertEqual(self.pin.read_bytes(), before)

    def test_cli_emits_only_the_version_offline(self):
        command = [sys.executable, "-B", str(ROOT / "scripts/tool_versions.py"), "--version"]
        expected = (ROOT / tool_versions.PIN).read_text(encoding="ascii")

        result = subprocess.run(command, capture_output=True, text=True, check=False)

        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, expected, ""))


if __name__ == "__main__":
    unittest.main()
