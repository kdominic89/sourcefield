"""Reject unmodeled external commands in installation and publication test doubles."""

import tempfile
import unittest
from pathlib import Path

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
from support.transports import consumer_git_transport, installation_transport, publication_transport


class TransportContractTests(unittest.TestCase):
    """A fake command runner must fail closed when its command contract changes."""

    def test_installation_transport_rejects_unknown_command(self):
        """An unknown installation command cannot be reported as successful."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            transport = installation_transport(Path(temporary))

            # Act
            with self.assertRaises(AssertionError) as error:
                transport(["gh", "unknown-operation"], check=True)

            # Assert
            self.assertIn("unmodeled installation", str(error.exception))

    def test_publication_transport_rejects_unknown_command(self):
        """An unknown publication command cannot create or publish release state."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            transport = publication_transport(Path(temporary), {})

            # Act
            with self.assertRaises(AssertionError) as error:
                transport(["gh", "unknown-operation"], check=True)

            # Assert
            self.assertIn("unmodeled publication", str(error.exception))

    def test_consumer_transport_rejects_unknown_command(self):
        """An unknown Git command cannot silently pass a consumer publication test."""
        # Arrange
        transport = consumer_git_transport

        # Act
        with self.assertRaises(AssertionError) as error:
            transport(["git", "unknown-operation"], check=True)

        # Assert
        self.assertIn("unmodeled consumer Git", str(error.exception))

    def test_installation_transport_rejects_unverified_executable(self):
        """A version-looking command must still name the verified staging executable."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            transport = installation_transport(root)

            # Act
            with self.assertRaises(AssertionError) as error:
                transport([str(root / "unverified"), "--version"], check=True)

            # Assert
            self.assertIn("unmodeled installation", str(error.exception))

    def test_publication_transport_rejects_changed_repository(self):
        """A supported API verb must retain the exact synthetic repository identity."""
        # Arrange
        with tempfile.TemporaryDirectory() as temporary:
            state = {}
            transport = publication_transport(Path(temporary), state)

            # Act
            with self.assertRaisesRegex(AssertionError, "unmodeled"):
                transport(["gh", "api", "repos/other/sourcefield/commits/tags/v1.2.3", "--jq", ".sha"],
                          check=True)

            # Assert
            self.assertEqual(state, {})

    def test_consumer_transport_rejects_changed_revision_query(self):
        """A revision query for another ref cannot reuse the modeled HEAD result."""
        # Arrange
        command = ["git", "rev-parse", "other-branch"]

        # Act
        with self.assertRaises(AssertionError) as error:
            consumer_git_transport(command, check=True)

        # Assert
        self.assertIn("unmodeled consumer Git", str(error.exception))


if __name__ == "__main__":
    unittest.main()
