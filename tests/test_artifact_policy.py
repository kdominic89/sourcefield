"""Verify public SVG admission, imported authority and semantic state schemas."""

import json
import tempfile
import unittest
from pathlib import Path

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import validate_artifact as validator


class SvgPolicyTests(unittest.TestCase):
    """Ensure public links remain useful without admitting executable/resource payloads."""

    def test_allows_public_anchor_and_local_paint(self):
        source = (
            '<svg xmlns="http://www.w3.org/2000/svg"><a href="https://www.nuget.org/packages/test">'
            '<rect fill="url(#paint)"/></a></svg>'
        )

        validator.validate_svg_payload(source)

    def assert_svg_rejected(self, payload: str, diagnostic: str) -> None:
        """Submit one active or externally linked payload inside an otherwise plain SVG root."""
        # Arrange
        source = f'<svg xmlns="http://www.w3.org/2000/svg">{payload}</svg>'

        # Act
        with self.assertRaises(AssertionError) as error:
            validator.validate_svg_payload(source)

        # Assert
        self.assertIn(diagnostic, str(error.exception))

    def test_rejects_svg_image_set_style(self):
        """Reject image-set resource syntax in inline style."""
        self.assert_svg_rejected(
            '<rect style="fill: image-set(&quot;https://example.com/a&quot;)"/>',
            'unsafe or obfuscated SVG style value',
        )

    def test_rejects_svg_script_element(self):
        """Reject executable script elements."""
        self.assert_svg_rejected('<script/>', 'forbidden SVG element: script')

    def test_rejects_svg_foreign_object(self):
        """Reject embedded foreign-object content."""
        self.assert_svg_rejected('<foreignObject/>', 'forbidden SVG element: foreignObject')

    def test_rejects_svg_event_handler(self):
        """Reject event-handler attributes."""
        self.assert_svg_rejected('<rect onload="alert(1)"/>', 'forbidden SVG attribute: onload')

    def test_rejects_svg_external_use(self):
        """Reject use references to external resources."""
        self.assert_svg_rejected('<use href="https://example.com/resource"/>', 'external SVG resource or unsafe anchor')

    def test_rejects_svg_javascript_link(self):
        """Reject JavaScript anchor destinations."""
        self.assert_svg_rejected('<a href="javascript:alert(1)"/>', 'external SVG resource or unsafe anchor')

    def test_rejects_svg_external_paint(self):
        """Reject paint references to external resources."""
        self.assert_svg_rejected('<rect fill="url(https://example.com/image)"/>', 'external SVG CSS resource')

    def test_rejects_svg_stylesheet_import(self):
        """Reject stylesheet imports."""
        self.assert_svg_rejected(
            '<style>@import "https://example.com";</style>',
            'unsafe or obfuscated SVG style value',
        )

    def test_rejects_svg_escaped_external_url(self):
        """Reject CSS-escaped external resource references."""
        self.assert_svg_rejected(
            '<style>.a { fill: u\\72l(https://example.com); }</style>',
            'unsafe or obfuscated SVG style value',
        )

    def test_rejects_svg_animated_link(self):
        """Reject animation that changes an anchor destination."""
        self.assert_svg_rejected(
            '<animate attributeName="href" values="javascript:alert(1)"/>',
            'animation target is outside the presentation allowlist',
        )

    def test_rejects_svg_credentialed_link(self):
        """Reject public links containing credentials."""
        self.assert_svg_rejected('<a href="https://user:pass@example.com"/>', 'external SVG resource or unsafe anchor')

    def test_rejects_svg_external_xml_base(self):
        """Reject an XML base that redirects resource resolution."""
        self.assert_svg_rejected('<rect xml:base="https://example.com"/>', 'forbidden SVG attribute: base')

    def test_allows_plain_accessibility_text_that_looks_like_css(self):
        source = ('<svg xmlns="http://www.w3.org/2000/svg" '
                  'aria-label="Metadata: image(foo) and data: documentation"/>')

        validator.validate_svg_payload(source)

    def test_rejects_entity_declarations(self):
        source = '<!DOCTYPE svg [<!ENTITY x "payload">]><svg xmlns="http://www.w3.org/2000/svg"/>'

        with self.assertRaises(AssertionError):
            validator.validate_svg_payload(source)


class ImportedAuthorityTests(unittest.TestCase):
    """Supplemental checks consume merged configuration, never infer approval from output nodes."""

    def test_merged_imported_packages_are_authoritative(self):
        authored = {"profile": {"username": "example"}, "imports": [{"id": "other"}]}
        resolved = {"schema_version": 1, "profile": authored["profile"],
                    "publications": [{"packages": [{"id": "Other.Provider"}]}]}

        with tempfile.TemporaryDirectory() as temporary:
            assets = Path(temporary)
            (assets / "resolved-config.json").write_text(json.dumps(resolved))

            result = validator.effective_config(authored, assets)

            self.assertEqual(result["publications"][0]["packages"][0]["id"], "Other.Provider")

    def test_missing_import_capture_is_rejected(self):
        authored = {"imports": [{"id": "other"}]}
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(AssertionError, "require resolved-config"):
                validator.effective_config(authored, Path(temporary))

    def test_changed_captured_identity_is_rejected(self):
        authored = {"profile": {"username": "example"}}
        resolved = {"schema_version": 1, "profile": {"username": "someone-else"}}
        with tempfile.TemporaryDirectory() as temporary:
            assets = Path(temporary)
            (assets / "resolved-config.json").write_text(json.dumps(resolved))

            with self.assertRaisesRegex(AssertionError, "does not match"):
                validator.effective_config(authored, assets)


class StateSchemaTests(unittest.TestCase):
    """Validate the one current schema while refusing legacy and future formats."""

    def write_state(self, root: Path, schema: int) -> dict:
        """Arrange paired synthetic public state copies for an independent case."""
        config = {
            "render": {"width": 1800, "height": 1680},
            "profile": {"username": "example", "organization": "example-org"},
            "projects": [{"id": "example"}],
        }

        state = {
            "schema_version": schema,
            "semantic_hash": "A" * 16,
            "canvas": config["render"],
            "profile": config["profile"],
            "nodes": [
                {
                    "id": "project:example",
                    "kind": "project",
                    "x": 100,
                    "y": 100,
                    "radius": 20,
                }
            ],
            "edges": [{"from": "project:example", "to": "project:example"}],
            "packages": [],
            "stats": {"package_count": 0},
        }

        for directory in ("assets", "docs"):
            (root / directory).mkdir()
            (root / directory / "profile-state.json").write_text(json.dumps(state))

        return config

    def test_current_schema_is_accepted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 3)

            result = validator.validate_state(root, config)

            self.assertEqual(result["schema_version"], 3)

    def test_authored_packages_without_cached_observations_are_valid(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 3)
            config["publications"] = [{"packages": [{"id": "Example.Package"}]}]
            state = json.loads((root / "assets/profile-state.json").read_text())
            state["nodes"].append({"id": "package:Example.Package", "kind": "package",
                                   "x": 200, "y": 200, "radius": 16})
            state["stats"]["package_count"] = 1
            for name in ("assets", "docs"):
                (root / name / "profile-state.json").write_text(json.dumps(state))

            result = validator.validate_state(root, config)

            self.assertEqual(result["stats"]["package_count"], 1)
            self.assertEqual(result["packages"], [])

    def test_legacy_schema_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 2)

            with self.assertRaisesRegex(
                AssertionError, "state schema_version must be 3"
            ):
                validator.validate_state(root, config)

    def test_future_schema_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config = self.write_state(root, 4)

            with self.assertRaisesRegex(
                AssertionError, "state schema_version must be 3"
            ):
                validator.validate_state(root, config)


class ConfigurationPolicyTests(unittest.TestCase):
    """Keep the artifact validator independent of an individual profile."""

    def test_generic_configuration_accepts_another_profile(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "config").mkdir()
            (root / "config/profile.toml").write_text(
                "schema_version = 1\n[render]\nwidth = 1800\nheight = 1680\n"
            )

            config = validator.validate_config(root)

            self.assertEqual(config["render"]["width"], 1800)


if __name__ == "__main__":
    unittest.main()
