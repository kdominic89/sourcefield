"""Verify shared artifact primitives preserve bounded IO and normalized ZIP metadata."""

import hashlib
import io
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
from sourcefield_tools import artifacts


class BoundedReader(io.BytesIO):
    """Reject unbounded reads so regressions fail before loading an entire artifact."""

    def __init__(self, payload: bytes):
        """Store synthetic bytes and record each requested buffer size."""
        super().__init__(payload)
        self.read_sizes = []

    def read(self, size: int = -1) -> bytes:
        """Permit at most one MiB per streaming digest read."""
        if not 0 < size <= 1024 * 1024:
            raise AssertionError(f"unexpected artifact read size: {size}")

        self.read_sizes.append(size)

        return super().read(size)


class ArtifactDigestTests(unittest.TestCase):
    """Exercise real filesystem failures and adversarial stream-size checks."""

    def test_digest_matches_sha256_across_multiple_buffers(self):
        # Arrange
        payload = b"synthetic artifact" * (128 * 1024)
        reader = BoundedReader(payload)
        expected = hashlib.sha256(payload).hexdigest()

        # Act
        with patch.object(Path, "open", return_value=reader):
            result = artifacts.digest_file(ROOT / "unused-synthetic-artifact")

        # Assert
        self.assertEqual(result, expected)
        self.assertGreater(len(reader.read_sizes), 2)
        self.assertTrue(all(size == 1024 * 1024 for size in reader.read_sizes))
        self.assertTrue(reader.closed)

    def test_empty_file_has_standard_sha256(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "empty"
            path.write_bytes(b"")

            # Act
            result = artifacts.digest_file(path)

            # Assert
            self.assertEqual(result, hashlib.sha256(b"").hexdigest())

    def test_missing_file_propagates_io_failure(self):
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "missing"

            # Act / Assert
            with self.assertRaises(FileNotFoundError):
                artifacts.digest_file(path)


class ArtifactZipMetadataTests(unittest.TestCase):
    """Keep archive identity independent of host permissions, timestamps and compressors."""

    def test_plain_entry_uses_normalized_metadata(self):
        # Arrange
        name = "runtime/app.js"

        # Act
        entry = artifacts.zip_info(name)

        # Assert
        self.assertEqual(entry.filename, name)
        self.assertEqual(entry.date_time, (1980, 1, 1, 0, 0, 0))
        self.assertEqual(entry.create_system, 3)
        self.assertEqual(entry.external_attr >> 16, 0o100644)
        self.assertEqual(entry.compress_type, zipfile.ZIP_STORED)

    def test_executable_entry_preserves_only_authored_execute_permission(self):
        # Arrange
        name = "sourcefield"

        # Act
        entry = artifacts.zip_info(name, executable=True)

        # Assert
        self.assertEqual(entry.filename, name)
        self.assertEqual(entry.external_attr >> 16, 0o100755)
        self.assertEqual(entry.compress_type, zipfile.ZIP_STORED)


if __name__ == "__main__":
    unittest.main()
