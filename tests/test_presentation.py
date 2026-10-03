"""Generic geometry invariants, exercised with synthetic positive and negative artifacts."""

import importlib.util
import json
import math
from pathlib import Path
import re
import tempfile
import unittest
import xml.etree.ElementTree as ET

NS = '{http://www.w3.org/2000/svg}'


def numbers(value: str) -> list[float]:
    """Read the renderer's numeric path notation without interpreting arbitrary SVG."""
    return [float(part) for part in re.findall(r'-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?', value)]


def require(condition: bool, message: str) -> None:
    """Reject an invariant violation with context suitable for a candidate-build report."""
    if not condition:
        raise AssertionError(message)


def validate_geometry(svg: ET.Element, state: dict) -> None:
    """Verify emitted ownership endpoints, circle clearance and evenly fitted personal ports."""
    nodes = {node['id']: node for node in state['nodes']}
    rendered = {element.get('data-node-id'): element for element in svg.iter()
                if element.get('data-node-id')}
    pairs = [element for element in svg.iter()
             if element.get('data-edge-id') or element.get('data-connection') == 'domain-bridge']
    departures: dict[str, list[float]] = {}

    for group in pairs:
        identifiers = (group.get('data-from'), group.get('data-to'))
        require(all(identifier in nodes and identifier in rendered for identifier in identifiers),
                'connection endpoints must reference rendered state nodes')
        endpoints = [nodes[identifier] for identifier in identifiers]
        centers = [(node['x'], node['y']) for node in endpoints]
        radii = [node['radius'] for node in endpoints]
        paths = list(group)
        require(len(paths) == 2 and paths[0].get('d') == paths[1].get('d'),
                'base and animated connection geometry must agree')
        values = numbers(paths[0].get('d', ''))
        require(len(values) == 8 and all(math.isfinite(value) for value in values),
                'connection must be one finite cubic')
        points = list(zip(values[::2], values[1::2]))

        for tip, handle, center, radius in ((points[0], points[1], centers[0], radii[0]),
                                            (points[3], points[2], centers[1], radii[1])):
            require(abs(math.dist(tip, center) - radius) < .00001,
                    'connection must end on the circle boundary')
            ray = (tip[0] - center[0], tip[1] - center[1])
            tangent = (handle[0] - tip[0], handle[1] - tip[1])
            denominator = math.hypot(*ray) * math.hypot(*tangent)
            require(denominator > 0, 'connection must have a nonzero outward endpoint tangent')
            alignment = sum(a * b for a, b in zip(ray, tangent)) / denominator
            require(alignment > .99999, 'connection tangent must point radially outward')

        for index in range(501):
            t = index / 500
            weights = ((1-t)**3, 3*(1-t)**2*t, 3*(1-t)*t*t, t**3)
            point = tuple(sum(p[axis] * w for p, w in zip(points, weights)) for axis in (0, 1))
            require(all(math.dist(point, center) >= radius - .00001
                        for center, radius in zip(centers, radii)), 'connection enters a circle')

        if endpoints[0].get('scope') == 'personal':
            departures.setdefault(identifiers[0], []).append(
                math.atan2(points[0][1] - centers[0][1], points[0][0] - centers[0][0]))

    actual_edges = {(group.get('data-from'), group.get('data-to')) for group in pairs}
    expected_edges = {(edge['from'], edge['to']) for edge in state['edges']
                      if edge.get('show_in_readme') and edge['kind'] == 'contains'
                      and nodes[edge['from']]['kind'] == 'domain'
                      and nodes[edge['to']]['kind'] == 'project'
                      and nodes[edge['from']]['show_in_readme'] and nodes[edge['to']]['show_in_readme']}
    require(expected_edges.issubset(actual_edges), 'visible ownership connections are missing')

    for angles in departures.values():
        angles.sort()
        if len(angles) < 2:
            continue

        for index, angle in enumerate(angles):
            gap = (angles[(index + 1) % len(angles)] - angle) % math.tau
            require(abs(gap - math.tau / len(angles)) < .000001, 'personal departures are not evenly distributed')


def validate_ornaments(svg: ET.Element, state: dict) -> None:
    """Check ring directions and ownership-scoped signal inventory without fixed counts."""
    visible = [node for node in state['nodes'] if node['show_in_readme']]
    nodes = {element.get('data-node-id'): element for element in svg.iter() if element.get('data-node-id')}

    for node in visible:
        if node['kind'] not in ('domain', 'project'):
            continue

        require(node['id'] in nodes, 'visible field node is missing')
        element = nodes[node['id']]
        if node['kind'] == 'project':
            rings = {child.get('data-project-ring'): child for child in element.iter()
                     if child.get('data-project-ring')}
            require(set(rings) == {'outer', 'middle'}, 'project requires independent rings')
            require(rings['outer'].get('class') == 'rotate', 'outer ring direction changed')
            require(rings['middle'].get('class') == 'rotate reverse', 'middle ring direction changed')
            continue

        organization = node.get('scope') == 'organization'
        kind = 'package' if organization else 'project'
        satellite = 'nuget-package' if organization else 'personal-project'
        expected = sum(other['kind'] == kind and other.get('domain') == node.get('domain') for other in visible)
        signals = [child for child in element.iter() if child.get('data-satellite') == satellite]
        require(len(signals) == expected, 'orbital signals must match their owning domain inventory')
        require(len({child.get('data-signal-delay') for child in signals}) == expected,
                'signal timing must be distinct within each domain')

    if any(element.get('data-connection') == 'domain-bridge' for element in svg.iter()):
        gradient = next((element for element in svg.iter() if element.get('id') == 'bridge-gradient'), None)
        require(gradient is not None, 'ownership bridge gradient is missing')
        require([child.get('stop-color') for child in gradient] ==
                ['#4DE7C2', '#8B7CFF', '#D582FF', '#FFB86B'], 'ownership bridge gradient changed')


def validate_packages(svg: ET.Element) -> None:
    """Check publication connectors and alternating rings independently of group counts."""
    packages = [element for element in svg.iter() if element.get('data-node-kind') == 'package']
    lines = [numbers(element.get('d', '')) for element in svg.iter() if element.get('class') == 'package-connector']
    require(len(lines) == len(packages), 'every package requires one incoming connector')
    previous = None

    for package in packages:
        glyph = next(element for element in package.iter() if element.get('transform', '').startswith('translate('))
        x, y = numbers(glyph.get('transform'))
        orbit = next(element for element in glyph if element.get('data-package-row') is not None)
        row = int(orbit.get('data-package-row'))
        require(orbit.get('class') == ('rotate reverse' if row % 2 else 'rotate'), 'package ring direction changed')
        candidates = [line for line in lines if len(line) == 3 and line[0] == x and line[2] == y - 16]
        require(len(candidates) == 1 and candidates[0][1] < candidates[0][2], 'package connector misses its ring')
        if row:
            require(previous is not None and previous[0] == x and candidates[0][1] == previous[1] + 16,
                    'package connector does not start on the preceding ring')

        previous = (x, y)


def validate_presentation(assets: Path) -> None:
    """Check all three actual generated flavors at an explicit asset directory."""
    state = json.loads((assets / 'profile-state.json').read_text(encoding='utf-8'))
    require(state.get('schema_version') == 3, 'presentation state requires schema_version 3')

    for flavor in ('dark', 'light', 'static'):
        svg = ET.parse(assets / f'sourcefield.{flavor}.svg').getroot()
        validate_geometry(svg, state)
        validate_ornaments(svg, state)
        validate_packages(svg)


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
        svg, state = fixture()

        validate_geometry(svg, state)

    def test_rejects_endpoint_outside_circle(self):
        svg, state = fixture()
        state['nodes'][0]['radius'] = 11

        with self.assertRaisesRegex(AssertionError, 'circle boundary'):
            validate_geometry(svg, state)

    def test_rejects_missing_connection(self):
        svg, state = fixture()
        svg.remove(list(svg)[-1])

        with self.assertRaisesRegex(AssertionError, 'connections are missing'):
            validate_geometry(svg, state)

    def test_rejects_uneven_personal_departures(self):
        svg, state = fixture()
        state['nodes'].append({'id': 'project:second', 'kind': 'project', 'domain': 'example',
                              'x': 300, 'y': 100, 'radius': 10, 'show_in_readme': True})
        ET.SubElement(svg, NS + 'g', {'data-node-id': 'project:second'})
        connection = ET.SubElement(svg, NS + 'g', {'data-edge-id': 'second',
            'data-from': 'domain:example', 'data-to': 'project:second'})
        for _ in range(2):
            ET.SubElement(connection, NS + 'path', {'d': 'M110 100C150 100 250 100 290 100'})

        with self.assertRaisesRegex(AssertionError, 'personal departures'):
            validate_geometry(svg, state)

    def test_valid_scoped_signals(self):
        svg, state = fixture()

        validate_ornaments(svg, state)

    def test_rejects_signal_from_another_domain(self):
        svg, state = fixture()
        state['nodes'][1]['domain'] = 'other'

        with self.assertRaisesRegex(AssertionError, 'owning domain'):
            validate_ornaments(svg, state)

    def test_rejects_same_direction_middle_ring(self):
        svg, state = fixture()
        middle = next(element for element in svg.iter() if element.get('data-project-ring') == 'middle')
        middle.set('class', 'rotate')

        with self.assertRaisesRegex(AssertionError, 'middle ring direction'):
            validate_ornaments(svg, state)

    def test_explicit_directory_and_all_flavors(self):
        svg, state = fixture()
        with tempfile.TemporaryDirectory() as directory:
            assets = Path(directory)
            (assets / 'profile-state.json').write_text(json.dumps(state), encoding='utf-8')
            for flavor in ('dark', 'light', 'static'):
                ET.ElementTree(svg).write(assets / f'sourcefield.{flavor}.svg')

            validate_presentation(assets)


class EditorialContractTests(unittest.TestCase):
    """Independent facts preserve editorial checks without embedding a specific consumer."""

    @staticmethod
    def validator():
        """Load the standalone standard-library validator."""
        path = Path(__file__).resolve().parents[1] / 'scripts/validate_artifact.py'
        spec = importlib.util.spec_from_file_location('artifact_validator', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        return module

    def test_accepts_matching_independent_facts(self):
        config = {'profile': {'username': 'example'}, 'projects': [{'id': 'tool', 'summary': 'Approved'}]}
        contract = {'fields': {'profile': {'username': 'example'}}, 'projects': {'tool': {'summary': 'Approved'}}}
        validator = self.validator()

        validator.validate_approved_profile(config, contract)

    def test_rejects_changed_editorial_content(self):
        config = {'projects': [{'id': 'tool', 'summary': 'Changed'}]}
        contract = {'projects': {'tool': {'summary': 'Approved'}}}
        validator = self.validator()

        with self.assertRaisesRegex(AssertionError, 'editorial contract mismatch'):
            validator.validate_approved_profile(config, contract)

    def test_rejects_missing_editorial_identity(self):
        config = {'projects': []}
        contract = {'projects': {'tool': {}}}
        validator = self.validator()

        with self.assertRaisesRegex(AssertionError, 'identities differ'):
            validator.validate_approved_profile(config, contract)

    def test_rejects_empty_contract(self):
        validator = self.validator()

        with self.assertRaisesRegex(AssertionError, 'recognized independent expectations'):
            validator.validate_approved_profile({}, {})

    def test_accepts_discovery_for_its_own_group_owner(self):
        config = {'publications': [{'owner': 'first', 'discovery_prefixes': ['Example.First']},
                                   {'owner': 'second', 'discovery_prefixes': ['Example.Second']}]}
        package = {'id': 'Example.Second.Provider', 'owner': 'second', 'version': '1.0.0'}
        validator = self.validator()

        approved = validator.approved_discovered_package(package, config)

        self.assertTrue(approved)

    def test_rejects_package_prefix_from_another_owner(self):
        config = {'publications': [{'owner': 'first', 'discovery_prefixes': ['Example.First']},
                                   {'owner': 'second', 'discovery_prefixes': ['Example.Second']}]}
        package = {'id': 'Example.Second.Provider', 'owner': 'first', 'version': '1.0.0'}
        validator = self.validator()

        approved = validator.approved_discovered_package(package, config)

        self.assertFalse(approved)


if __name__ == '__main__':
    unittest.main()
