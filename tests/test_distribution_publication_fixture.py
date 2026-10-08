"""Cover the retained-input setup of the real fresh-checkout publication gate."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import verify_publication


class PublicationFixtureTests(unittest.TestCase):
    """Keep explicit capture provenance without manufacturing missing observations."""

    def test_seed_copies_snapshot_bytes_to_the_consumer_capture_path(self):
        """Preserve formatting, dates and the original authoring seed byte for byte."""
        with tempfile.TemporaryDirectory() as temporary:
            # Arrange
            root = Path(temporary) / "source"
            (root / "config").mkdir(parents=True)
            (root / "config/profile.toml").write_text(
                '[profile]\npages_url = "https://example.invalid/profile/"\n', encoding="ascii"
            )

            captured = b'{ "schema_version": 1, "fetched_at": "2026-09-03T00:00:00Z" }\r\n'
            (root / "config/offline-snapshot.json").write_bytes(captured)
            seed = Path(temporary) / "seed"
            seed.mkdir()

            # Act
            verify_publication.seed_consumer(root, seed)

            # Assert
            self.assertEqual((seed / "assets/source-snapshot.json").read_bytes(), captured)
            self.assertEqual((seed / "config/offline-snapshot.json").read_bytes(), captured)
            self.assertEqual((root / "config/offline-snapshot.json").read_bytes(), captured)
            self.assertTrue((seed / "README.md").is_file())
            self.assertEqual(
                json.loads((seed / ".sourcefield-owned.json").read_text()),
                {
                    "schema_version": 1,
                    "files": {"assets/source-snapshot.json": hashlib.sha256(captured).hexdigest()},
                    "authored_files": {},
                },
            )

    def test_seed_rejects_missing_observations_without_fabricating_a_capture(self):
        """A missing synthetic input must fail instead of becoming an empty snapshot."""
        with tempfile.TemporaryDirectory() as temporary:
            # Arrange
            root = Path(temporary) / "source"
            (root / "config").mkdir(parents=True)
            (root / "config/profile.toml").write_text(
                '[profile]\npages_url = "https://example.invalid/profile/"\n', encoding="ascii"
            )

            seed = Path(temporary) / "seed"
            seed.mkdir()

            # Act
            with self.assertRaises(FileNotFoundError) as failure:
                verify_publication.seed_consumer(root, seed)

            # Assert
            self.assertEqual(Path(failure.exception.filename), root / "config/offline-snapshot.json")
            self.assertFalse((seed / "assets/source-snapshot.json").exists())
            self.assertFalse((seed / ".sourcefield-owned.json").exists())

    def test_publication_boundary_rejects_changed_capture_bytes(self):
        """Reject reserialization even when both inputs describe equivalent JSON."""
        with tempfile.TemporaryDirectory() as temporary:
            # Arrange
            root = Path(temporary)
            (root / "assets").mkdir()
            expected = b'{ "schema_version": 1 }\n'
            rewritten = b'{"schema_version":1}\n'
            (root / "assets/source-snapshot.json").write_bytes(rewritten)

            # Act
            with self.assertRaisesRegex(AssertionError, "changed the retained synthetic capture"):
                verify_publication.require_capture(root, expected)

            # Assert
            self.assertEqual((root / "assets/source-snapshot.json").read_bytes(), rewritten)


if __name__ == "__main__":
    unittest.main()
