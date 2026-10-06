"""Generic geometry invariants, exercised with synthetic positive and negative artifacts."""

import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

# Bootstrap uninstalled tooling so discovery and direct execution share module identities.
import support
import validate_artifact
from sourcefield_tools.presentation import (
    validate_geometry, validate_ornaments, validate_packages, validate_presentation,
)


NS = '{http://www.w3.org/2000/svg}'


def fixture() -> tuple[ET.Element, dict]:
    """Build a minimal synthetic ownership graph with one valid radial connection."""
    state = {'schema_version': 3, 'nodes': [
        {'id': 'domain:example', 'kind': 'domain', 'scope': 'personal', 'domain': 'example',
         'x': 100, 'y': 100, 'radius': 10, 'show_in_readme': True},
        {'id': 'project:tool', 'kind': 'project', 'domain': 'example',
         'x': 300, 'y': 100, 'radius': 10, 'show_in_readme': True}],
        'edges': [{'from': 'domain:example', 'to': 'project:tool', 'kind': 'contains', 'show_in_readme': True}]}

    svg = ET.fromstring('<svg xmlns="http://www.w3.org/2000/svg">'
        '<g data-node-id="domain:example"><circle data-satellite="personal-project" data-signal-delay="0"/></g>'
        '<g data-node-id="project:tool"><g data-project-ring="outer" class="rotate"/>'
        '<g data-project-ring="middle" class="rotate reverse"/></g>'
        '<g data-edge-id="ownership" data-from="domain:example" data-to="project:tool">'
        '<path d="M110 100C150 100 250 100 290 100"/><path d="M110 100C150 100 250 100 290 100"/></g></svg>')

    return svg, state


class PresentationTests(unittest.TestCase):
    """Positive and adversarial coverage of reusable presentation invariants."""

    def test_valid_radial_connection(self):
        # Arrange
        svg, state = fixture()

        # Act / Assert
        validate_geometry(svg, state)

    def test_rejects_endpoint_outside_circle(self):
        # Arrange
        svg, state = fixture()
        state['nodes'][0]['radius'] = 11

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'circle boundary'):
            validate_geometry(svg, state)

    def test_rejects_missing_connection(self):
        # Arrange
        svg, state = fixture()
        svg.remove(list(svg)[-1])

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'connections are missing'):
            validate_geometry(svg, state)

    def test_rejects_uneven_personal_departures(self):
        # Arrange
        svg, state = fixture()
        state['nodes'].append({'id': 'project:second', 'kind': 'project', 'domain': 'example',
                              'x': 300, 'y': 100, 'radius': 10, 'show_in_readme': True})
        ET.SubElement(svg, NS + 'g', {'data-node-id': 'project:second'})
        connection = ET.SubElement(svg, NS + 'g', {'data-edge-id': 'second',
            'data-from': 'domain:example', 'data-to': 'project:second'})

        for _ in range(2):
            ET.SubElement(connection, NS + 'path', {'d': 'M110 100C150 100 250 100 290 100'})

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'personal departures'):
            validate_geometry(svg, state)

    def test_valid_scoped_signals(self):
        # Arrange
        svg, state = fixture()

        # Act / Assert
        validate_ornaments(svg, state)

    def test_rejects_signal_from_another_domain(self):
        # Arrange
        svg, state = fixture()
        state['nodes'][1]['domain'] = 'other'

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'owning domain'):
            validate_ornaments(svg, state)

    def test_rejects_same_direction_middle_ring(self):
        # Arrange
        svg, state = fixture()
        middle = next(element for element in svg.iter() if element.get('data-project-ring') == 'middle')
        middle.set('class', 'rotate')

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'middle ring direction'):
            validate_ornaments(svg, state)

    def test_explicit_directory_and_all_flavors(self):
        # Arrange
        svg, state = fixture()
        with tempfile.TemporaryDirectory() as directory:
            assets = Path(directory)
            (assets / 'profile-state.json').write_text(json.dumps(state), encoding='utf-8')
            for flavor in ('dark', 'light', 'static'):
                ET.ElementTree(svg).write(assets / f'sourcefield.{flavor}.svg')

            # Act / Assert
            validate_presentation(assets)


def package_fixture() -> ET.Element:
    """Build two package rows with ring-boundary connectors and alternating rotation."""

    return ET.fromstring(
        '<svg xmlns="http://www.w3.org/2000/svg">'
        '<path class="package-connector" d="M100 20V84"/>'
        '<path class="package-connector" d="M100 116V134"/>'
        '<g data-node-kind="package"><g transform="translate(100 100)">'
        '<g data-package-row="0" class="rotate"/></g></g>'
        '<g data-node-kind="package"><g transform="translate(100 150)">'
        '<g data-package-row="1" class="rotate reverse"/></g></g></svg>'
    )


class PackagePresentationTests(unittest.TestCase):
    """Protect the moved package geometry checks with positive and negative generated-markup fixtures."""

    def test_accepts_alternating_rows_with_boundary_connectors(self):
        # Arrange
        svg = package_fixture()

        # Act / Assert
        validate_packages(svg)

    def test_rejects_missing_package_connector(self):
        # Arrange
        svg = package_fixture()
        svg.remove(list(svg)[0])

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "one incoming connector"):
            validate_packages(svg)

    def test_rejects_connector_missing_ring_boundary(self):
        # Arrange
        svg = package_fixture()
        list(svg)[0].set("d", "M100 20V83")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "misses its ring"):
            validate_packages(svg)

    def test_rejects_connector_starting_inside_preceding_ring(self):
        # Arrange
        svg = package_fixture()
        list(svg)[1].set("d", "M100 115V134")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "preceding ring"):
            validate_packages(svg)

    def test_rejects_same_direction_successive_package_rings(self):
        # Arrange
        svg = package_fixture()
        row = next(element for element in svg.iter() if element.get("data-package-row") == "1")
        row.set("class", "rotate")

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, "ring direction"):
            validate_packages(svg)


class EditorialContractTests(unittest.TestCase):
    """Independent facts preserve editorial checks without embedding a specific consumer."""

    def test_accepts_matching_independent_facts(self):
        # Arrange
        config = {'profile': {'username': 'example'}, 'projects': [{'id': 'tool', 'summary': 'Approved'}]}
        contract = {'fields': {'profile': {'username': 'example'}}, 'projects': {'tool': {'summary': 'Approved'}}}

        # Act / Assert
        validate_artifact.validate_approved_profile(config, contract)

    def test_rejects_changed_editorial_content(self):
        # Arrange
        config = {'projects': [{'id': 'tool', 'summary': 'Changed'}]}
        contract = {'projects': {'tool': {'summary': 'Approved'}}}

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'editorial contract mismatch'):
            validate_artifact.validate_approved_profile(config, contract)

    def test_rejects_missing_editorial_identity(self):
        # Arrange
        config = {'projects': []}
        contract = {'projects': {'tool': {}}}

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'identities differ'):
            validate_artifact.validate_approved_profile(config, contract)

    def test_rejects_empty_contract(self):
        # Arrange
        config, contract = {}, {}

        # Act / Assert
        with self.assertRaisesRegex(AssertionError, 'recognized independent expectations'):
            validate_artifact.validate_approved_profile(config, contract)

    def test_accepts_discovery_for_its_own_group_owner(self):
        # Arrange
        config = {'publications': [{'owner': 'first', 'discovery_prefixes': ['Example.First']},
                                   {'owner': 'second', 'discovery_prefixes': ['Example.Second']}]}

        package = {'id': 'Example.Second.Provider', 'owner': 'second', 'version': '1.0.0'}

        # Act
        approved = validate_artifact.approved_discovered_package(package, config)

        # Assert
        self.assertTrue(approved)

    def test_rejects_package_prefix_from_another_owner(self):
        # Arrange
        config = {'publications': [{'owner': 'first', 'discovery_prefixes': ['Example.First']},
                                   {'owner': 'second', 'discovery_prefixes': ['Example.Second']}]}

        package = {'id': 'Example.Second.Provider', 'owner': 'first', 'version': '1.0.0'}

        # Act
        approved = validate_artifact.approved_discovered_package(package, config)

        # Assert
        self.assertFalse(approved)


if __name__ == '__main__':
    unittest.main()
