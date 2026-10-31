"""Exercise authenticated provisioning, extraction and cache rejection boundaries."""

from __future__ import annotations

import errno
import hashlib
import io
import os
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
from support.tool_versions import HOST, Response, archive_fixture, manifest, write_manifest
from sourcefield_tools import binaryen


class BinaryenPinTests(unittest.TestCase):
    """Bind all supported hosts to exact archives and complete optimizer runtime files."""

    def test_authored_manifest_has_both_macos_libraries(self):
        document = binaryen.read_manifest(ROOT)

        self.assertEqual(document["version"], 133)
        self.assertEqual(set(document["hosts"]), set(binaryen.HOSTS))
        for host in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
            self.assertEqual([item["path"] for item in document["hosts"][host]["files"]],
                             ["bin/wasm-opt", "lib/libbinaryen.dylib"])

    def assert_rejected(self, change, message):
        """Submit one authored manifest mutation to the real parser boundary."""
        document = manifest()
        change(document)

        with self.assertRaisesRegex(ValueError, message):
            binaryen.validate_manifest(document)

    def test_missing_host_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"].pop(HOST), "all five")

    def test_unknown_host_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"].update({"unknown": {}}), "all five")

    def test_boolean_version_is_rejected(self):
        self.assert_rejected(lambda value: value.update(version=True), "integer")

    def test_unknown_schema_is_rejected(self):
        self.assert_rejected(lambda value: value.update(schema_version=2), "schema_version")

    def test_changed_optimizer_policy_is_rejected(self):
        self.assert_rejected(lambda value: value["optimizer_flags"].append("--skip-validation"), "policy")

    def test_missing_macos_library_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"]["aarch64-apple-darwin"]["files"].pop(), "exactly")

    def test_archive_alias_is_rejected(self):
        self.assert_rejected(
            lambda value: value["hosts"][HOST]["archive"].update(name="../archive.tar.gz"), "archive name"
        )

    def test_boolean_size_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"][HOST]["archive"].update(size=True), "integer")

    def test_oversized_archive_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"][HOST]["archive"].update(size=2**30), "integer")

    def test_uppercase_digest_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"][HOST]["archive"].update(sha256="A" * 64), "SHA-256")

    def test_extra_member_path_is_rejected(self):
        self.assert_rejected(lambda value: value["hosts"][HOST]["files"][0].update(path="lib/evil.so"), "file path")

    def test_duplicate_json_keys_are_rejected(self):
        source = b'{"version":133,"version":134}'

        with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
            binaryen.json.loads(source, object_pairs_hook=binaryen.unique_object)


class BinaryenProvisioningTests(unittest.TestCase):
    """Use isolated caches and real tar parsing while replacing only HTTP transport."""

    def setUp(self):
        """Create one small authenticated Linux fixture without executing any tools."""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.cache = self.root / "build/cache"
        self.body, self.record = archive_fixture([
            ("binaryen-version_133/bin/wasm-opt", b"optimizer fixture", tarfile.REGTYPE),
        ])
        self.document = manifest()
        self.document["hosts"][HOST] = self.record
        write_manifest(self.root, self.document)

    def provision(self):
        """Run the complete cache path with one bounded synthetic HTTPS response."""
        response = Response(self.body, url=binaryen.DOWNLOAD + "/version_133/" + self.record["archive"]["name"])
        with patch.object(binaryen, "open_response", return_value=response):
            result = binaryen.ensure_optimizer(self.root, HOST, self.cache)

        return result, response

    def test_provision_authenticates_archive_and_executable(self):
        result, response = self.provision()

        self.assertEqual(result.read_bytes(), b"optimizer fixture")
        self.assertTrue(result.is_absolute())
        self.assertTrue(all(0 < size <= binaryen.CHUNK_BYTES for size in response.read_sizes))

    def test_verified_cache_is_reused_without_network(self):
        result, _ = self.provision()
        with patch.object(binaryen, "open_response") as network:
            cached = binaryen.ensure_optimizer(self.root, HOST, self.cache, offline=True)

        self.assertEqual(cached, result)
        network.assert_not_called()

    def test_concurrent_completed_cache_is_reauthenticated_after_nonempty_rename(self):
        def another_builder_finishes(prepared, destination):
            shutil.copytree(prepared, destination)
            raise OSError(errno.ENOTEMPTY, "another builder finished first")

        with patch.object(Path, "rename", another_builder_finishes):
            result, _ = self.provision()

        self.assertEqual(result.read_bytes(), b"optimizer fixture")
        self.assertEqual(len(list(self.cache.iterdir())), 1)

    def test_cache_rename_permission_error_is_not_treated_as_concurrent_success(self):
        with patch.object(Path, "rename", side_effect=PermissionError("denied")):
            with self.assertRaisesRegex(PermissionError, "denied"):
                self.provision()

        self.assertEqual(list(self.cache.iterdir()), [])

    def test_tampered_cached_executable_is_rejected(self):
        result, _ = self.provision()
        result.write_bytes(b"Optimizer fixture")

        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            binaryen.ensure_optimizer(self.root, HOST, self.cache)

    def test_unexpected_cached_library_is_rejected(self):
        result, _ = self.provision()
        (result.parent / "libfake.so").write_bytes(b"foreign")

        with self.assertRaisesRegex(ValueError, "unexpected"):
            binaryen.ensure_optimizer(self.root, HOST, self.cache)

    def test_symlinked_cached_executable_is_rejected(self):
        result, _ = self.provision()
        other = self.root / "outside"
        result.rename(other)
        result.symlink_to(other)

        with self.assertRaisesRegex(ValueError, "symlink"):
            binaryen.ensure_optimizer(self.root, HOST, self.cache)

    def test_symlinked_cache_parent_is_rejected(self):
        actual = self.root / "actual"
        actual.mkdir()
        self.cache.parent.mkdir()
        self.cache.symlink_to(actual, target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "symlink"):
            binaryen.ensure_optimizer(self.root, HOST, self.cache)

    def test_offline_cache_miss_never_downloads(self):
        with patch.object(binaryen, "open_response") as network:
            with self.assertRaisesRegex(ValueError, "not provisioned"):
                binaryen.ensure_optimizer(self.root, HOST, self.cache, offline=True)

        network.assert_not_called()

    def test_bad_archive_digest_never_reaches_tar_parser(self):
        self.record["archive"]["sha256"] = "0" * 64
        write_manifest(self.root, self.document)
        response = Response(self.body, url=binaryen.DOWNLOAD + "/archive")
        with (
            patch.object(binaryen, "open_response", return_value=response),
            patch.object(binaryen, "unpack_optimizer") as unpack,
        ):
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                binaryen.ensure_optimizer(self.root, HOST, self.cache)

        unpack.assert_not_called()
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_short_archive_is_rejected(self):
        response = Response(self.body[:-1], url=binaryen.DOWNLOAD + "/archive")
        with patch.object(binaryen, "open_response", return_value=response):
            with self.assertRaisesRegex(ValueError, "size or SHA-256"):
                binaryen.ensure_optimizer(self.root, HOST, self.cache)

        self.assertEqual(list(self.cache.iterdir()), [])

    def test_overlong_archive_is_rejected(self):
        response = Response(self.body + b"x", url=binaryen.DOWNLOAD + "/archive")
        with patch.object(binaryen, "open_response", return_value=response):
            with self.assertRaisesRegex(ValueError, "exceeds its pinned size"):
                binaryen.ensure_optimizer(self.root, HOST, self.cache)

        self.assertEqual(list(self.cache.iterdir()), [])

    def assert_archive_rejected(self, entries, message, change=None):
        """Exercise one authenticated malformed archive through complete provisioning."""
        self.body, self.record = archive_fixture(entries)
        if change:
            change(self.record)

        self.document["hosts"][HOST] = self.record
        write_manifest(self.root, self.document)
        response = Response(self.body, url=binaryen.DOWNLOAD + "/archive")
        with patch.object(binaryen, "open_response", return_value=response):
            with self.assertRaisesRegex(ValueError, message):
                binaryen.ensure_optimizer(self.root, HOST, self.cache)

        self.assertEqual(list(self.cache.iterdir()), [])

    def test_duplicate_required_archive_member_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin/wasm-opt", b"a", tarfile.REGTYPE)] * 2, "duplicate")

    def test_symlink_archive_member_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin/wasm-opt", b"", tarfile.SYMTYPE)], "unsafe")

    def test_hardlink_archive_member_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin/wasm-opt", b"", tarfile.LNKTYPE)], "unsafe")

    def test_archive_traversal_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/../outside", b"a", tarfile.REGTYPE)], "unsafe")

    def test_absolute_archive_path_is_rejected(self):
        self.assert_archive_rejected([("/binaryen-version_133/bin/wasm-opt", b"a", tarfile.REGTYPE)], "unsafe")

    def test_archive_dot_alias_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/./bin/wasm-opt", b"a", tarfile.REGTYPE)], "unsafe")

    def test_archive_backslash_alias_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin\\wasm-opt", b"a", tarfile.REGTYPE)], "unsafe")

    def test_pax_metadata_is_rejected_before_its_body_is_read(self):
        self.assert_archive_rejected([("binaryen-version_133/extended", b"", tarfile.XHDTYPE)], "extended metadata")

    def test_gnu_long_path_metadata_is_rejected_before_its_body_is_read(self):
        self.assert_archive_rejected(
            [("binaryen-version_133/long", b"", tarfile.GNUTYPE_LONGNAME)], "extended metadata"
        )

    def test_sparse_metadata_is_rejected_before_its_geometry_is_read(self):
        self.assert_archive_rejected([("binaryen-version_133/sparse", b"", tarfile.GNUTYPE_SPARSE)], "sparse metadata")

    def test_similarly_named_archive_root_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133-foreign/bin/wasm-opt", b"a", tarfile.REGTYPE)], "unsafe")

    def test_missing_optimizer_member_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin/other", b"a", tarfile.REGTYPE)], "missing")

    def test_required_member_hash_mismatch_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin/wasm-opt", b"a", tarfile.REGTYPE)], "SHA-256",
                                     lambda record: record["files"][0].update(sha256="0" * 64))

    def test_required_member_size_mismatch_is_rejected(self):
        self.assert_archive_rejected([("binaryen-version_133/bin/wasm-opt", b"a", tarfile.REGTYPE)], "pinned size",
                                     lambda record: record["files"][0].update(size=2))

    def test_excessive_member_count_is_rejected(self):
        entries = [(f"binaryen-version_133/{number}", b"a", tarfile.REGTYPE) for number in range(3)]
        with patch.object(binaryen, "MAX_MEMBERS", 2):
            self.assert_archive_rejected(entries, "excessive")

    def test_decompression_budget_is_enforced(self):
        source = binaryen.BoundedReader(io.BytesIO(b"12345"))
        source.remaining = 3

        with self.assertRaisesRegex(ValueError, "expanded byte budget"):
            source.read(5)

    def test_symlinked_pin_is_rejected(self):
        path = self.root / binaryen.PIN
        path.rename(self.root / "original")
        path.symlink_to(self.root / "original")

        with self.assertRaisesRegex(ValueError, "symlink"):
            binaryen.read_manifest(self.root)

    def test_oversized_pin_is_rejected(self):
        (self.root / binaryen.PIN).write_bytes(b" " * (binaryen.MAX_MANIFEST_BYTES + 1))

        with self.assertRaisesRegex(ValueError, "64 KiB"):
            binaryen.read_manifest(self.root)


class BinaryenSnapshotTests(unittest.TestCase):
    """Authenticate the exact private execution copy after mutable-cache validation."""

    def setUp(self):
        """Create a private root and one frozen manifest for each independent scenario."""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.document = manifest()
        self.destination = self.root / "private-optimizer"

    def source_fixture(self, host=HOST):
        """Create a complete host cache with small exact executable and library pins."""
        base = self.root / "cache"
        payloads = {}
        records = []
        for relative in binaryen.required_files(host):
            data = ("verified:" + relative).encode("ascii")
            path = base / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            payloads[relative] = data
            records.append({"path": relative, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()})

        self.document["hosts"][host]["files"] = records

        return base / records[0]["path"], payloads

    def assert_host_snapshot(self, host):
        """Copy one admitted platform layout and verify only its pinned files survive."""
        source, expected = self.source_fixture(host)

        result = binaryen.snapshot_optimizer(source, self.document, host, self.destination)

        self.assertTrue(result.is_absolute())
        self.assertEqual(result.relative_to(self.destination).as_posix(), binaryen.required_files(host)[0])
        self.assertEqual(
            {path.relative_to(self.destination).as_posix(): path.read_bytes()
             for path in self.destination.rglob("*") if path.is_file()}, expected,
        )
        if os.name == "posix":
            self.assertEqual(self.destination.stat().st_mode & 0o777, 0o700)
            self.assertTrue(result.stat().st_mode & 0o100)

    def test_linux_x64_snapshot_contains_only_the_authenticated_optimizer(self):
        self.assert_host_snapshot("x86_64-unknown-linux-gnu")

    def test_linux_arm_snapshot_contains_only_the_authenticated_optimizer(self):
        self.assert_host_snapshot("aarch64-unknown-linux-gnu")

    def test_windows_snapshot_preserves_the_executable_suffix(self):
        self.assert_host_snapshot("x86_64-pc-windows-msvc")

    def test_macos_arm_snapshot_contains_the_authenticated_sibling_library(self):
        self.assert_host_snapshot("aarch64-apple-darwin")

    def test_macos_x64_snapshot_contains_the_authenticated_sibling_library(self):
        self.assert_host_snapshot("x86_64-apple-darwin")

    def test_cache_changed_after_prior_validation_is_rejected_and_removed(self):
        source, _ = self.source_fixture()
        binaryen.validate_cache(source.parent.parent, self.document["hosts"][HOST])
        source.write_bytes(b"V" + source.read_bytes()[1:])

        with self.assertRaisesRegex(ValueError, "snapshot.*SHA-256"):
            binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertFalse(self.destination.exists())

    def test_source_changed_after_open_is_rejected_by_the_copied_byte_hash(self):
        source, _ = self.source_fixture()
        original_fdopen = os.fdopen
        def change_opened_source(descriptor, mode):
            stream = original_fdopen(descriptor, mode)
            source.write_bytes(b"V" + source.read_bytes()[1:])

            return stream

        with patch.object(binaryen.os, "fdopen", side_effect=change_opened_source):
            with self.assertRaisesRegex(ValueError, "snapshot.*SHA-256"):
                binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertFalse(self.destination.exists())

    def test_missing_source_does_not_leave_an_execution_copy(self):
        source, _ = self.source_fixture()
        source.unlink()

        with self.assertRaises(FileNotFoundError):
            binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertFalse(self.destination.exists())

    def test_symlinked_source_does_not_leave_an_execution_copy(self):
        source, _ = self.source_fixture()
        actual = source.with_name("actual")
        source.rename(actual)
        source.symlink_to(actual)

        with self.assertRaisesRegex(ValueError, "symlink"):
            binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertFalse(self.destination.exists())

    def test_corrupted_library_removes_the_already_copied_executable(self):
        host = "aarch64-apple-darwin"
        source, _ = self.source_fixture(host)
        binaryen.validate_cache(source.parent.parent, self.document["hosts"][host])
        library = source.parent.parent / "lib/libbinaryen.dylib"
        library.write_bytes(b"V" + library.read_bytes()[1:])

        with self.assertRaisesRegex(ValueError, "snapshot.*SHA-256"):
            binaryen.snapshot_optimizer(source, self.document, host, self.destination)

        self.assertFalse(self.destination.exists())

    def test_symlinked_library_removes_the_already_copied_executable(self):
        host = "aarch64-apple-darwin"
        source, _ = self.source_fixture(host)
        library = source.parent.parent / "lib/libbinaryen.dylib"
        actual = self.root / "actual-library"
        library.rename(actual)
        library.symlink_to(actual)

        with self.assertRaisesRegex(ValueError, "symlink"):
            binaryen.snapshot_optimizer(source, self.document, host, self.destination)

        self.assertFalse(self.destination.exists())
        self.assertTrue(actual.is_file())

    def test_missing_library_removes_the_already_copied_executable(self):
        host = "aarch64-apple-darwin"
        source, _ = self.source_fixture(host)
        (source.parent.parent / "lib/libbinaryen.dylib").unlink()

        with self.assertRaises(FileNotFoundError):
            binaryen.snapshot_optimizer(source, self.document, host, self.destination)

        self.assertFalse(self.destination.exists())

    def test_oversized_source_stops_at_the_pinned_size(self):
        source, _ = self.source_fixture()
        source.write_bytes(source.read_bytes() + b"extra")

        with self.assertRaisesRegex(ValueError, "snapshot.*size"):
            binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertFalse(self.destination.exists())

    def test_truncated_source_is_rejected(self):
        source, _ = self.source_fixture()
        source.write_bytes(source.read_bytes()[:-1])

        with self.assertRaisesRegex(ValueError, "snapshot.*size"):
            binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertFalse(self.destination.exists())

    def test_later_cache_changes_do_not_change_the_private_copy(self):
        source, expected = self.source_fixture()
        result = binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        source.write_bytes(b"different cache contents")

        self.assertEqual(result.read_bytes(), expected["bin/wasm-opt"])

    def test_existing_destination_is_preserved(self):
        source, _ = self.source_fixture()
        self.destination.mkdir()
        sentinel = self.destination / "sentinel"
        sentinel.write_bytes(b"keep")

        with self.assertRaises(FileExistsError):
            binaryen.snapshot_optimizer(source, self.document, HOST, self.destination)

        self.assertEqual(sentinel.read_bytes(), b"keep")

    def test_symlinked_destination_parent_is_rejected(self):
        source, _ = self.source_fixture()
        target = self.root / "actual"
        target.mkdir()
        parent = self.root / "linked"
        parent.symlink_to(target, target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "symlink"):
            binaryen.snapshot_optimizer(source, self.document, HOST, parent / "copy")

        self.assertEqual(list(target.iterdir()), [])



if __name__ == "__main__":
    unittest.main()
