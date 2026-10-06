"""Keep mandatory icon coverage isolated from the legacy source fixtures."""

from pathlib import Path
import tempfile
import tomllib
import unittest

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
from support import ROOT
from release_browser_fixtures import ICON_KEYS, icon_fixture


class IconFixtureTests(unittest.TestCase):
    """Exercise copied configurations and fail closed on unsupported fixture inputs."""

    def test_three_compositions_cover_builtins_and_custom_without_source_changes(self):
        sources = [
            ROOT / "config/profile.toml",
            ROOT / "config/organization-profile.toml",
            ROOT / "examples/multi-organization.toml",
        ]
        originals = {source: source.read_bytes() for source in sources}
        originals.update({source: source.read_bytes() for source in [
            ROOT / "examples/organization.toml",
            ROOT / "examples/second-organization.toml",
        ]})

        with tempfile.TemporaryDirectory() as temporary:
            copies = [icon_fixture(source, Path(temporary) / str(index) / source.name)
                      for index, source in enumerate(sources)]
            configurations = [tomllib.loads(copy.read_text()) for copy in copies]
            manifests = [tomllib.loads((copies[2].parent / item["source"]["path"]).read_text())
                         for item in configurations[2]["imports"]]

            for source, content in originals.items():
                self.assertEqual(source.read_bytes(), content)

            for config in configurations[:2] + manifests:
                self.assertEqual(tuple(project["icon"] for project in config["projects"][:3]), ICON_KEYS)
                self.assertEqual(config["icons"]["browser-probe"]["radius"], 64)
                self.assertEqual([element["geometry"]["shape"] for element in
                                  config["icons"]["browser-probe"]["elements"]],
                                 ["rect", "circle", "ellipse", "path"])

            self.assertEqual(len(manifests), 2)
            self.assertEqual(configurations[2]["imports"], tomllib.loads(originals[sources[2]].decode())["imports"])
            self.assertEqual([project["radius"] for project in configurations[0]["projects"]],
                             [project["radius"] for project in
                              tomllib.loads(originals[sources[0]].decode())["projects"]])

    def test_import_escape_is_rejected_before_reading_outside_fixture(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "profile.toml"
            source.write_text('[[imports]]\nid="outside"\n[imports.source]\nkind="local"\npath="../outside.toml"\n')

            with self.assertRaisesRegex(ValueError, "must stay beside"):
                icon_fixture(source, root / "copy/profile.toml")

            self.assertFalse((root / "copy").exists())

    def test_remote_import_is_rejected_without_fetching(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "profile.toml"
            source.write_text('[[imports]]\nid="remote"\n[imports.source]\nkind="github"\npath="organization.toml"\n')

            with self.assertRaisesRegex(ValueError, "require local"):
                icon_fixture(source, root / "copy/profile.toml")

            self.assertFalse((root / "copy").exists())

    def test_missing_fixture_projects_fails_instead_of_skipping_icons(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "profile.toml"
            source.write_text("schema_version = 1\n")

            with self.assertRaisesRegex(ValueError, "requires three projects"):
                icon_fixture(source, root / "copy/profile.toml")

            self.assertFalse((root / "copy").exists())


if __name__ == "__main__":
    unittest.main()
