//! Deterministic, data-driven SVG presentation for SOURCEFIELD.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::{collections::BTreeMap, fmt::Write as _};

mod geometry;
mod ornaments;
mod presentation;

use presentation::{Footer, OwnershipCurve, Presentation};

use geometry::{Curve, Point};
use sourcefield_core::{EdgeKind, Node, NodeKind, ProfileState, SnapshotMode, Visibility};

/// Color scheme for an independently usable SVG artifact.
#[derive(Debug, Clone, Copy)]
pub enum Theme {
    /// Low-luminance profile presentation.
    Dark,
    /// High-luminance presentation with dark readable labels.
    Light,
}

/// Semantic colors keep light-theme text independent of decorative opacity.
#[derive(Clone, Copy)]
struct Palette {
    background: &'static str,
    secondary: &'static str,
    surface: &'static str,
    text: &'static str,
    muted: &'static str,
    quiet: &'static str,
    grid: &'static str,
    mint: &'static str,
    purple: &'static str,
    amber: &'static str,
    blue: &'static str,
}

impl Theme {
    /// Resolve explicit contrast-aware values rather than inverting the dark image.
    fn palette(self) -> Palette {
        match self {
            Self::Dark => Palette {
                background: "#050911",
                secondary: "#0c1222",
                surface: "#0d1425",
                text: "#edf2ff",
                muted: "#a2acc8",
                quiet: "#93a3bf",
                grid: "#3f4e6a",
                mint: "#4de7c2",
                purple: "#a898ff",
                amber: "#ffbd7a",
                blue: "#7acfff",
            },
            Self::Light => Palette {
                background: "#f5f7fc",
                secondary: "#e9eef8",
                surface: "#ffffff",
                text: "#11182b",
                muted: "#47536d",
                quiet: "#53617b",
                grid: "#8592aa",
                mint: "#006c59",
                purple: "#6243b5",
                amber: "#92500b",
                blue: "#17628e",
            },
        }
    }
}

/// Render the approved field composition from public state.
///
/// Labels never move; motion is restricted to decorative rings and ownership traces.
/// `motion = false` omits animation rules entirely. Configuration controls optional modules.
/// State produced by `sourcefield_core` supplies validated coordinates and HTTPS links.
pub fn render_svg(state: &ProfileState, theme: Theme, motion: bool) -> String {
    PreparedPresentation::new(state).render(theme, motion)
}

/// Borrowed immutable presentation, reusable across theme and animation variants.
///
/// Preparation indexes node references once; it does not clone graph nodes or approved text.
pub struct PreparedPresentation<'a> {
    state: &'a ProfileState,
    nodes: BTreeMap<&'a str, &'a Node>,
    policy: Presentation,
}

impl<'a> PreparedPresentation<'a> {
    /// Prepare a validated state for repeated rendering without rebuilding its node index.
    pub fn new(state: &'a ProfileState) -> Self {
        Self {
            state,
            nodes: state
                .nodes
                .iter()
                .map(|node| (node.id.as_str(), node))
                .collect(),
            policy: Presentation::new(state),
        }
    }

    /// Render a deterministic, self-contained SVG using this immutable prepared state.
    pub fn render(&self, theme: Theme, motion: bool) -> String {
        render_prepared(self, theme, motion)
    }
}

/// Emit one flavor while reusing the prepared variant selection and borrowed graph index.
fn render_prepared(prepared: &PreparedPresentation<'_>, theme: Theme, motion: bool) -> String {
    let state = prepared.state;
    let policy = &prepared.policy;
    let nodes = &prepared.nodes;
    let palette = theme.palette();
    let mut output = String::with_capacity(48_000);
    let width = state.canvas.width;
    let height = state.canvas.height;
    let sx = policy.scale;
    let sy = policy.scale;

    let _ = write!(
        output,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" \
         height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" \
         aria-labelledby=\"title description\"><title id=\"title\">{} - \
         SOURCEFIELD</title><desc id=\"description\">{}. Project connections \
         group ownership, not implementation dependencies.</desc>",
        xml(&state.profile.username),
        xml(&state.profile.tagline)
    );

    definitions(&mut output, palette, motion, state);
    let _ = write!(
        output,
        "<g clip-path=\"url(#frame)\"><rect width=\"{width}\" \
         height=\"{height}\" fill=\"url(#bg)\"/><rect width=\"{width}\" \
         height=\"{height}\" fill=\"url(#grid)\"/>"
    );

    // Normalize decorative typography only; semantic node coordinates remain in canvas units.
    let _ = write!(output, r#"<g transform="scale({sx} {sy})">"#);
    for index in 0..policy.stars {
        let x = 35 + (index * 193 + 89) % 1730;
        let y = 235 + (index * 137 + 31) % 665;
        let _ = write!(
            output,
            r#"<circle cx="{x}" cy="{y}" r=".8" fill="{}" opacity=".22"/>"#,
            palette.quiet
        );
    }

    header(&mut output, state, palette);
    output.push_str("</g>");
    field(&mut output, state, nodes, palette, policy);
    let _ = write!(output, r#"<g transform="scale({sx} {sy})">"#);
    publications(&mut output, state, nodes, palette, policy);
    let footer_offset = policy.footer_offset;
    let _ = write!(output, r#"<g transform="translate(0 {footer_offset})">"#);
    match policy.footer {
        Footer::Personal(layout) => personal(&mut output, state, palette, layout),
        Footer::Organization => organization_footer(&mut output, state, palette),
    }

    output.push_str("</g>");
    output.push_str("</g></g></svg>");

    output
}

/// Emit self-contained SVG resources; no network fonts or executable SVG content are required.
fn definitions(output: &mut String, p: Palette, motion: bool, state: &ProfileState) {
    let width = state.canvas.width;
    let height = state.canvas.height;
    let seconds = state.canvas.motion_seconds;

    let _ = write!(
        output,
        "<defs><linearGradient id=\"bg\" x2=\"1\" y2=\"1\"><stop \
         stop-color=\"{}\"/><stop offset=\".48\" stop-color=\"{}\"/><stop \
         offset=\"1\" stop-color=\"{}\"/></linearGradient><pattern id=\"grid\" \
         width=\"32\" height=\"32\" patternUnits=\"userSpaceOnUse\"><path \
         d=\"M32 0H0V32\" fill=\"none\" stroke=\"{}\" stroke-opacity=\".16\" \
         stroke-width=\".6\"/></pattern><filter id=\"glow\" x=\"-100%\" \
         y=\"-100%\" width=\"300%\" height=\"300%\"><feGaussianBlur \
         stdDeviation=\"2.4\" result=\"b\"/><feMerge><feMergeNode in=\"b\"/>\
         <feMergeNode in=\"SourceGraphic\"/></feMerge></filter><clipPath \
         id=\"frame\"><rect width=\"{width}\" height=\"{height}\" rx=\"28\"/>\
         </clipPath>",
        p.background, p.secondary, p.background, p.grid
    );

    for (name, color) in [("mint", p.mint), ("purple", p.purple), ("amber", p.amber)] {
        let _ = write!(
            output,
            "<radialGradient id=\"{name}\"><stop stop-color=\"{color}\" \
             stop-opacity=\".13\"/><stop offset=\"1\" stop-color=\"{color}\" \
             stop-opacity=\"0\"/></radialGradient>"
        );
    }

    for (id, first, second) in [
        ("personal-gradient", p.mint, p.purple),
        ("organization-gradient", p.amber, p.blue),
    ] {
        let _ = write!(
            output,
            "<linearGradient id=\"{id}\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\">\
             <stop offset=\"0\" stop-color=\"{first}\"/>\
             <stop offset=\"1\" stop-color=\"{second}\"/></linearGradient>"
        );
    }

    // The bridge intentionally keeps the original artwork's four color stops.
    output.push_str(concat!(
        "<linearGradient id=\"bridge-gradient\"><stop offset=\"0\" ",
        "stop-color=\"#4DE7C2\" stop-opacity=\".15\"/><stop offset=\".44\" ",
        "stop-color=\"#8B7CFF\"/><stop offset=\".55\" stop-color=\"#D582FF\"/>",
        "<stop offset=\"1\" stop-color=\"#FFB86B\"/></linearGradient>"
    ));

    let _ = write!(
        output,
        "</defs><style>text{{font-family:-apple-system,BlinkMacSystemFont,'Segoe \
         UI',sans-serif}}.mono{{font-family:Menlo,Consolas,\
         monospace}}.project:focus,.package:focus{{outline:2px solid {};\
         outline-offset:4px}}.project:hover text,.package:hover \
         text{{fill:{}}}.paused \
         *{{animation-play-state:paused!important}}</style>",
        p.purple, p.text
    );

    if motion {
        let duration = seconds.max(12);
        let orbit = duration + 26;

        // Keep standalone delays in a style block: Pages removes it before DOM parsing,
        // then restores numeric timing from data attributes without CSP inline-style violations.
        output.push_str("<style>");
        for (kind, satellite, step) in [
            (NodeKind::Project, "personal-project", 0.8),
            (NodeKind::Package, "nuget-package", 0.65),
        ] {
            let count = state
                .nodes
                .iter()
                .filter(|node| node.kind == kind && node.show_in_readme)
                .count();

            for index in 0..count {
                let delay = -(index as f64 * step);
                let value = if kind == NodeKind::Project {
                    format!("{delay:.1}")
                } else {
                    format!("{delay:.2}")
                };

                let _ = write!(
                    output,
                    ".signal[data-satellite=\"{satellite}\"][data-signal-delay=\"{value}\"]\
                     {{animation-delay:{value}s}}"
                );
            }
        }

        output.push_str("</style>");

        let _ = write!(
            output,
            "<style>.rotate{{animation:orbit {orbit}s linear infinite;\
             transform-origin:0 0}}.reverse{{animation-direction:reverse}}.flow{{stro\
             ke-dasharray:3 21;animation:flow {duration}s linear \
             infinite}}.pulse{{animation:pulse 6s ease-in-out infinite}}\
             .scan{{animation-duration:24s}}.signal{{animation:signal 3.6s ease-in-out infinite}}\
             @keyframes signal{{0%,100%{{opacity:.18}}45%{{opacity:1}}}}@keyframes \
             orbit{{to{{transform:rotate(360deg)}}}}@keyframes \
             flow{{to{{stroke-dashoffset:-240}}}}@keyframes pulse{{0%,\
             100%{{opacity:.25}}50%{{opacity:.85}}}}@media(prefers-reduced-motion:red\
             uce){{.rotate,.flow,.pulse,.signal{{animation:none!important}}}}</style>"
        );
    }
}

/// Layout chrome uses normalized design coordinates so canvas resizing cannot clip its footer.
fn header(output: &mut String, state: &ProfileState, p: Palette) {
    text(
        output,
        (64.0, 56.0),
        "SOURCEFIELD",
        12,
        p.quiet,
        "start",
        "mono",
    );
    text(
        output,
        (64.0, 120.0),
        &state.profile.username,
        52,
        p.text,
        "start",
        "identity",
    );
    text(
        output,
        (66.0, 156.0),
        &state.profile.tagline,
        19,
        p.muted,
        "start",
        "",
    );
    let status = match state.mode {
        SnapshotMode::Live => "LIVE",
        SnapshotMode::Preview => "PREVIEW",
        SnapshotMode::Partial => "PARTIAL",
        SnapshotMode::Fallback => "FALLBACK",
    };

    text(output, (1736.0, 57.0), status, 11, p.quiet, "end", "mono");
    if state.canvas.show_technology_labels {
        text(
            output,
            (1736.0, 112.0),
            &state.presentation.main_stack.join(" / "),
            18,
            p.text,
            "end",
            "mono",
        );
        text(
            output,
            (1736.0, 143.0),
            &state.presentation.supporting_stack.join(" / "),
            13,
            p.muted,
            "end",
            "mono",
        );
    }

    if state.canvas.show_state_hash {
        text(
            output,
            (1736.0, 179.0),
            &state.semantic_hash,
            10,
            p.quiet,
            "end",
            "mono",
        );
    }

    path(output, "M64 194H1736", p.grid, ".45", "");
}

/// Draw ownership relationships using the same coordinates exposed to browser selection.
fn field(
    output: &mut String,
    state: &ProfileState,
    nodes: &BTreeMap<&str, &Node>,
    p: Palette,
    policy: &Presentation,
) {
    let scale = state.canvas.width as f32 / 1800.0;
    let organization_profile = matches!(policy.curve, OwnershipCurve::Sweeping);
    let domains_count = state
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Domain && node.show_in_readme)
        .count();

    for domain in state
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Domain && node.show_in_readme)
    {
        let color = node_color(domain, p);
        let organization = domain.scope.as_deref() == Some("organization");

        if let Some(position) = policy.heading(domain, domains_count, state.canvas.width) {
            let heading = if organization {
                domain.label.to_uppercase()
            } else {
                "PERSONAL PROJECTS".to_string()
            };

            text(output, position, &heading, 11, color, "start", "mono");
        }

        let gradient = if organization { "amber" } else { "mint" };
        let center_y = domain.y + policy.orbit_y_offset * scale;
        let [glow_x, glow_y] = policy.glow;
        let [orbit_x, orbit_y] = policy.orbit;
        let opacity = policy.orbit_opacity;

        let _ = write!(
            output,
            r#"<ellipse cx="{}" cy="{center_y}" rx="{}" ry="{}" fill="url(#{gradient})"/>"#,
            domain.x,
            glow_x * scale,
            glow_y * scale
        );

        if state.canvas.show_activity_orbit {
            let _ = write!(
                output,
                "<ellipse cx=\"{}\" cy=\"{center_y}\" rx=\"{}\" ry=\"{}\" fill=\"none\" \
                 stroke=\"{color}\" stroke-opacity=\"{opacity}\" class=\"flow\"/>",
                domain.x,
                orbit_x * scale,
                orbit_y * scale
            );
        }
    }

    let mut domains = state
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Domain && node.show_in_readme)
        .collect::<Vec<_>>();

    domains.sort_by(|a, b| a.x.total_cmp(&b.x));

    let ports = departure_ports(state, &domains, nodes);
    for pair in domains.windows(2) {
        let left = pair[0];
        let right = pair[1];
        let departure = ports.get(&(left.id.as_str(), right.id.as_str())).copied();
        let d = connection_path(left, right, state, departure, true, policy.curve);

        if let Some(d) = d {
            // This bridge joins ownership fields; it does not assert a code dependency.
            let _ = write!(
                output,
                "<g data-connection=\"domain-bridge\" data-from=\"{}\" data-to=\"{}\">",
                xml(&left.id),
                xml(&right.id)
            );

            path(output, &d, "url(#bridge-gradient)", ".20", "");
            path(output, &d, "url(#bridge-gradient)", ".65", "flow");
            output.push_str("</g>");
        }
    }

    let mut edges = state
        .edges
        .iter()
        .filter(|edge| edge.show_in_readme && edge.kind == EdgeKind::Contains)
        .collect::<Vec<_>>();

    edges.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    for edge in edges {
        let (Some(from), Some(to)) = (nodes.get(edge.from.as_str()), nodes.get(edge.to.as_str()))
        else {
            continue;
        };

        if from.kind != NodeKind::Domain
            || to.kind != NodeKind::Project
            || !from.show_in_readme
            || !to.show_in_readme
        {
            continue;
        }

        let departure = ports.get(&(from.id.as_str(), to.id.as_str())).copied();
        let Some(d) = connection_path(from, to, state, departure, false, policy.curve) else {
            continue;
        };

        let _ = write!(
            output,
            r#"<g data-edge-id="{}:{}" data-from="{}" data-to="{}">"#,
            xml(&edge.from),
            xml(&edge.to),
            xml(&edge.from),
            xml(&edge.to)
        );

        let color = if organization_profile && to.visual.as_deref() == Some("hierarchy") {
            p.mint
        } else {
            node_color(from, p)
        };

        let [base_opacity, flow_opacity] = policy.edge_opacity;

        path(output, &d, color, base_opacity, "");
        path(output, &d, color, flow_opacity, "flow");
        output.push_str("</g>");
    }

    for node in nodes.values().filter(|node| {
        node.show_in_readme && matches!(node.kind, NodeKind::Domain | NodeKind::Project)
    }) {
        project(output, node, state, p);
    }
}

/// Fit account departure ports to visible ownership neighbors and decorative bridges.
fn departure_ports<'a>(
    state: &'a ProfileState,
    domains: &[&'a Node],
    nodes: &BTreeMap<&str, &'a Node>,
) -> BTreeMap<(&'a str, &'a str), f64> {
    let sx = state.canvas.width as f64 / 1800.0;
    let sy = state.canvas.width as f64 / 1800.0;
    let mut result = BTreeMap::new();

    for domain in domains
        .iter()
        .filter(|node| node.scope.as_deref() != Some("organization"))
    {
        let mut neighbors = state
            .edges
            .iter()
            .filter(|edge| {
                edge.show_in_readme && edge.kind == EdgeKind::Contains && edge.from == domain.id
            })
            .filter_map(|edge| nodes.get(edge.to.as_str()).copied())
            .filter(|node| node.show_in_readme && node.kind == NodeKind::Project)
            .collect::<Vec<_>>();

        for pair in domains.windows(2).filter(|pair| pair[0].id == domain.id) {
            neighbors.push(pair[1]);
        }

        let mut angles = neighbors
            .into_iter()
            .map(|node| {
                let angle =
                    ((node.y - domain.y) as f64 / sy).atan2((node.x - domain.x) as f64 / sx);

                (angle, node.id.as_str())
            })
            .collect::<Vec<_>>();

        angles.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)));
        let fitted = geometry::ports(&angles.iter().map(|entry| entry.0).collect::<Vec<_>>());

        for ((_, target), angle) in angles.into_iter().zip(fitted) {
            result.insert((domain.id.as_str(), target), angle);
        }
    }

    result
}

/// Work in design coordinates so radial connections also match scaled elliptical ornaments.
fn connection_path(
    from: &Node,
    to: &Node,
    state: &ProfileState,
    departure: Option<f64>,
    bridge: bool,
    curve: OwnershipCurve,
) -> Option<String> {
    let sx = state.canvas.width as f64 / 1800.0;
    let sy = state.canvas.width as f64 / 1800.0;
    let a = Point(from.x as f64 / sx, from.y as f64 / sy);
    let b = Point(to.x as f64 / sx, to.y as f64 / sy);
    let controls = if !bridge && matches!(curve, OwnershipCurve::Sweeping) {
        let dx = b.0 - a.0;
        let angle = (b.1 - a.1).atan2(dx);
        let turn = if dx < 0.0 { 0.45 } else { -0.45 };
        let length = dx.hypot(b.1 - a.1) * 0.42;
        let source_angle = angle + turn;
        let target_angle = angle - turn;

        // Opposite endpoint turns preserve the approved organization branch sweep.
        [
            Point(
                a.0 + (from.radius as f64 / sx + length) * source_angle.cos(),
                a.1 + (from.radius as f64 / sx + length) * source_angle.sin(),
            ),
            Point(
                b.0 - (to.radius as f64 / sx + length) * target_angle.cos(),
                b.1 - (to.radius as f64 / sx + length) * target_angle.sin(),
            ),
        ]
    } else if bridge {
        [
            Point(a.0 + 304.0, a.1 - 197.0),
            Point(b.0 - 267.0, b.1 + 219.0),
        ]
    } else {
        [
            Point(a.0 + (b.0 - a.0) * 0.55, a.1 - 80.0),
            Point(b.0 - 40.0, b.1 + 65.0),
        ]
    };

    Curve::between(
        (a, from.radius as f64 / sx),
        (b, to.radius as f64 / sx),
        controls,
        departure,
    )
    .map(|curve| curve.path(sx, sy))
}

/// Assign glyph colors from declared visual roles, never from project identities.
fn node_color(node: &Node, p: Palette) -> &'static str {
    match node
        .scope
        .as_deref()
        .filter(|scope| *scope == "organization")
        .or(node.visual.as_deref())
    {
        Some("spatial" | "dotfiles" | "provider") => p.blue,
        Some("trace" | "theme" | "lab") => p.purple,
        Some("organization" | "migrations" | "packages") => p.amber,
        _ => p.mint,
    }
}

/// Keep ornaments inside a translated child, leaving labels stable during rotation.
fn project(output: &mut String, node: &Node, state: &ProfileState, p: Palette) {
    let sx = state.canvas.width as f32 / 1800.0;
    let sy = state.canvas.width as f32 / 1800.0;
    let actual = (node.x, node.y);
    let color = node_color(node, p);
    let domain = node.kind == NodeKind::Domain;
    let kind = if domain { "domain" } else { "project" };
    let _ = write!(
        output,
        "<g class=\"project\" tabindex=\"0\" role=\"group\" aria-label=\"{}: \
         {}\" data-node-id=\"{}\" data-node-kind=\"{kind}\" data-domain=\"{}\" \
         data-x=\"{}\" data-y=\"{}\">",
        xml(&node.label),
        xml(&node.summary),
        xml(&node.id),
        xml(node.domain.as_deref().unwrap_or("")),
        actual.0,
        actual.1
    );

    let _ = write!(
        output,
        r#"<g transform="translate({} {}) scale({sx} {sy})">"#,
        actual.0, actual.1
    );

    if state.canvas.show_details && !node.details.is_empty() {
        let _ = write!(output, "<title>{}</title>", xml(&node.details.join("; ")));
    }

    let linked = open_link(output, node.url.as_deref());
    let radius = node.radius / sx;
    output.push_str(
        "<g data-node-decoration=\"true\" transform=\"translate(0 0)\" aria-hidden=\"true\">",
    );
    ornaments::render(output, node, radius, state, p);

    output.push_str("</g>");
    let mut y = if domain { 91.0 } else { radius + 25.0 };
    if let Some(prefix) = &node.label_prefix {
        text(output, (0.0, y), prefix, 12, p.quiet, "middle", "mono");
        y += 26.0;
    }

    text(
        output,
        (0.0, y),
        &node.surface_label,
        if domain { 21 } else { 23 },
        p.text,
        "middle",
        "node-label",
    );
    if domain {
        let projects = state
            .nodes
            .iter()
            .filter(|other| {
                other.show_in_readme
                    && other.kind == NodeKind::Project
                    && other.domain == node.domain
            })
            .fold((0, 0), |(public, private), other| {
                if other.visibility == Some(Visibility::Public) {
                    (public + 1, private)
                } else {
                    (public, private + 1)
                }
            });

        let (public, private) = projects;
        let summary = if public == 0 {
            format!("{private} private projects")
        } else {
            format!("{public} public / {private} private")
        };

        text(
            output,
            (0.0, y + 23.0),
            &summary,
            12,
            p.muted,
            "middle",
            "mono",
        );
    } else {
        for (index, line) in node.summary.lines().enumerate() {
            y += if index == 0 { 24.0 } else { 20.0 };
            text(
                output,
                (0.0, y),
                line,
                14,
                p.muted,
                "middle",
                "node-summary",
            );
        }

        if state.canvas.show_technology_labels {
            text(
                output,
                (0.0, y + 23.0),
                &node.display_stack.join(" / "),
                12,
                color,
                "middle",
                "mono",
            );
        }
    }

    if linked {
        output.push_str("</a>");
    }

    output.push_str("</g></g>");
}

/// Group linked package rows by their publication parent rather than inferred name prefixes.
fn publications(
    output: &mut String,
    state: &ProfileState,
    nodes: &BTreeMap<&str, &Node>,
    p: Palette,
    policy: &Presentation,
) {
    let mut groups = nodes
        .values()
        .filter(|node| node.kind == NodeKind::Publication && node.show_in_readme)
        .collect::<Vec<_>>();

    groups.sort_by(|a, b| {
        a.y.total_cmp(&b.y)
            .then(a.x.total_cmp(&b.x))
            .then(a.id.cmp(&b.id))
    });

    let packages = nodes
        .values()
        .filter(|node| node.kind == NodeKind::Package && node.show_in_readme)
        .count();

    output.push_str("<g data-module=\"publications\">");
    let scale = state.canvas.width as f32 / 1800.0;
    let section_top = groups
        .iter()
        .map(|group| group.y / scale - 80.0)
        .min_by(f32::total_cmp)
        .unwrap_or(policy.publication_top);

    path(output, &format!("M64 {section_top}H1736"), p.grid, ".5", "");
    text(
        output,
        (66.0, section_top + 40.0),
        &format!("NUGET / {packages} PACKAGES"),
        11,
        p.quiet,
        "start",
        "mono",
    );
    let sx = state.canvas.width as f32 / 1800.0;
    let sy = state.canvas.width as f32 / 1800.0;

    for (index, group) in groups.iter().enumerate() {
        let x = group.x / sx;
        let color = [p.blue, p.amber, p.mint][index % 3];
        let group_linked = open_link(output, group.url.as_deref());
        text(
            output,
            (x, group.y / sy),
            &group.label,
            18,
            color,
            "start",
            "",
        );
        if group_linked {
            output.push_str("</a>");
        }

        let mut children = state
            .edges
            .iter()
            .filter(|edge| edge.from == group.id && edge.kind == EdgeKind::Publishes)
            .filter_map(|edge| nodes.get(edge.to.as_str()).copied())
            .filter(|node| node.kind == NodeKind::Package && node.show_in_readme)
            .collect::<Vec<_>>();

        children.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.id.cmp(&b.id)));
        let mut previous = None;
        for node in &children {
            let x = node.x / sx;
            let y = node.y / sy;
            let start = previous.unwrap_or(group.y / sy + 17.0);

            if start < y - 16.0 {
                let d = format!("M{x} {start}V{}", y - 16.0);
                path(output, &d, p.quiet, ".28", "package-connector");
            }

            previous = Some(y + 16.0);
        }

        for (row, node) in children.into_iter().enumerate() {
            let direction = if row % 2 == 1 {
                "rotate reverse"
            } else {
                "rotate"
            };

            let x = node.x / sx - 19.0;
            let y = node.y / sy;
            let _ = write!(
                output,
                "<g class=\"package\" data-node-id=\"{}\" data-node-kind=\"package\" \
                 data-domain=\"{}\" tabindex=\"0\" role=\"group\" aria-label=\"{} on \
                 NuGet\">",
                xml(&node.id),
                xml(node.domain.as_deref().unwrap_or("")),
                xml(&node.label)
            );

            let linked = open_link(output, node.url.as_deref());
            let _ = write!(output, "<title>{}</title>", xml(&node.label));
            let _ = write!(
                output,
                "<g transform=\"translate({} {y})\" aria-hidden=\"true\"><g \
                 class=\"{direction}\" data-package-row=\"{row}\"><circle r=\"16\" fill=\"none\" \
                 stroke=\"{color}\" stroke-dasharray=\"2 6\"/></g>",
                x + 19.0
            );

            path(output, "M0 -7L6 -3V4L0 8L-6 4V-3Z", color, ".9", "");
            output.push_str("</g>");
            text(
                output,
                (x + 47.0, y + 5.0),
                &node.surface_label,
                15,
                p.text,
                "start",
                "mono",
            );
            text(
                output,
                (x + 47.0, y + 29.0),
                &node.summary,
                14,
                p.muted,
                "start",
                "",
            );
            if let Some(version) = node
                .details
                .iter()
                .find_map(|detail| detail.strip_prefix("Observed version: "))
            {
                text(
                    output,
                    (x + 47.0, y + 51.0),
                    version,
                    12,
                    color,
                    "start",
                    "mono",
                );
            }

            if linked {
                output.push_str("</a>");
            }

            output.push_str("</g>");
        }
    }

    output.push_str("</g>");
}

/// Render curated personal context without exposing repository implementation metadata.
fn personal(
    output: &mut String,
    state: &ProfileState,
    p: Palette,
    layout: sourcefield_core::PersonalFooterLayout,
) {
    use sourcefield_core::{
        FOOTER_DETAIL_COLUMNS, FOOTER_HARDWARE_COLUMNS, FOOTER_LABEL_COLUMNS,
        FOOTER_LEARNING_COLUMNS, next_interest_y,
    };

    path(output, "M64 1337H1736", p.grid, ".5", "");
    let columns_bottom = layout.separator_y - 24.0;
    path(
        output,
        &format!("M615 1373V{columns_bottom}M1201 1373V{columns_bottom}"),
        p.grid,
        ".4",
        "",
    );
    output.push_str("<g data-module=\"interests\">");
    if state.canvas.show_interests_in_readme {
        text(
            output,
            (66.0, 1381.0),
            "OTHER INTERESTS",
            11,
            p.quiet,
            "start",
            "mono",
        );
        let mut y = 1422.0;

        for (row, interest) in state
            .interests
            .iter()
            .filter(|interest| interest.show_in_readme)
            .enumerate()
        {
            let lines = footer_text(
                output,
                (66.0, y),
                &interest.label,
                20,
                if row == 0 { p.text } else { p.purple },
                (FOOTER_LABEL_COLUMNS, 24.0),
                "",
            );
            let summary_y = y + lines.saturating_sub(1) as f32 * 24.0 + 25.0;

            footer_text(
                output,
                (66.0, summary_y),
                &interest.summary,
                15,
                p.muted,
                (FOOTER_DETAIL_COLUMNS, 22.0),
                "",
            );
            y = next_interest_y(y, interest);
        }
    }

    footer_text(
        output,
        (66.0, layout.platforms_y),
        &state.presentation.platforms.join(" / "),
        19,
        p.text,
        (FOOTER_LABEL_COLUMNS, 30.0),
        "",
    );
    footer_text(
        output,
        (66.0, layout.platform_note_y),
        &state.presentation.platform_note,
        15,
        p.muted,
        (FOOTER_DETAIL_COLUMNS, 22.0),
        "",
    );
    output.push_str("</g><g data-module=\"learning\">");
    text(
        output,
        (647.0, 1381.0),
        "LEARNING",
        11,
        p.quiet,
        "start",
        "mono",
    );
    let mut learning_y = 1424.0;

    for learning in &state.learning {
        let linked = open_link(output, Some(&learning.url));
        let lines = footer_text(
            output,
            (647.0, learning_y),
            &learning.label,
            22,
            p.purple,
            (FOOTER_LEARNING_COLUMNS, 27.0),
            "",
        );
        learning_y += lines.max(1) as f32 * 27.0 + 13.0;
        if linked {
            output.push_str("</a>");
        }
    }

    footer_text(
        output,
        (647.0, layout.learning_note_y),
        &state.presentation.learning_note,
        15,
        p.muted,
        (FOOTER_DETAIL_COLUMNS, 30.0),
        "",
    );
    output.push_str("</g><g data-module=\"hardware\">");
    text(
        output,
        (1233.0, 1381.0),
        "AT HOME",
        11,
        p.quiet,
        "start",
        "mono",
    );
    let mut hardware_y = 1422.0;

    for hardware in &state.presentation.hardware {
        let labels = footer_text(
            output,
            (1233.0, hardware_y),
            &hardware.label,
            20,
            p.text,
            (FOOTER_LABEL_COLUMNS, 24.0),
            "",
        );
        let detail_y = hardware_y + labels.saturating_sub(1) as f32 * 24.0 + 27.0;
        let details = footer_text(
            output,
            (1233.0, detail_y),
            &hardware.detail,
            15,
            p.muted,
            (FOOTER_HARDWARE_COLUMNS, 22.0),
            "mono",
        );
        hardware_y +=
            67.0 + labels.saturating_sub(1) as f32 * 24.0 + details.saturating_sub(1) as f32 * 22.0;
    }

    output.push_str("</g>");
    path(
        output,
        &format!("M64 {}H1736", layout.separator_y),
        p.grid,
        ".4",
        "",
    );
    text(
        output,
        (64.0, layout.separator_y + 31.0),
        &format!("github.com/{}", state.profile.username),
        12,
        p.quiet,
        "start",
        "mono",
    );
    text(
        output,
        (1736.0, layout.separator_y + 31.0),
        &format!("github.com/{}", state.profile.organization),
        12,
        p.quiet,
        "end",
        "mono",
    );
}

/// Wrap borrowed text with the core's exact measurement rule to prevent capacity drift.
fn footer_text(
    output: &mut String,
    at: (f32, f32),
    value: &str,
    size: u32,
    color: &str,
    wrapping: (usize, f32),
    class: &str,
) -> usize {
    let mut count = 0;

    for (index, line) in sourcefield_core::wrapped_lines(value, wrapping.0).enumerate() {
        text(
            output,
            (at.0, at.1 + index as f32 * wrapping.1),
            line,
            size,
            color,
            "start",
            class,
        );
        count += 1;
    }

    count
}

/// Keep both organization and maintainer destinations available in standalone SVG viewers.
fn organization_footer(output: &mut String, state: &ProfileState, p: Palette) {
    path(output, "M64 1455H1736", p.grid, ".45", "");
    let organization_url = format!("https://github.com/{}", state.profile.organization);
    let linked = open_link(output, Some(&organization_url));

    text(
        output,
        (64.0, 1490.0),
        &format!("github.com/{}", state.profile.organization),
        12,
        p.quiet,
        "start",
        "mono",
    );

    if linked {
        output.push_str("</a>");
    }

    let Some(maintainer) = &state.profile.maintainer else {
        return;
    };

    let maintainer_url = &maintainer.url;
    let linked = open_link(output, Some(maintainer_url));

    text(
        output,
        (1736.0, 1490.0),
        &format!("{} / {}", maintainer.username, maintainer.role),
        12,
        p.muted,
        "end",
        "mono",
    );

    if linked {
        output.push_str("</a>");
    }
}

/// Encode text and attributes through one escaping boundary.
fn text(
    output: &mut String,
    at: (f32, f32),
    value: &str,
    size: u32,
    color: &str,
    anchor: &str,
    class: &str,
) {
    let weight = if matches!(class, "identity" | "node-label") {
        600
    } else {
        400
    };

    let _ = write!(
        output,
        "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" font-size=\"{size}\" \
         fill=\"{color}\" font-weight=\"{weight}\" class=\"{class}\">{}</text>",
        at.0,
        at.1,
        xml(value)
    );
}

/// Emit decorative paths whose geometry is generated internally.
fn path(output: &mut String, d: &str, color: &str, opacity: &str, class: &str) {
    let _ = write!(
        output,
        "<path d=\"{d}\" fill=\"none\" stroke=\"{color}\" \
         stroke-opacity=\"{opacity}\" stroke-width=\"1.2\" stroke-linecap=\"butt\" class=\"{class}\"/>"
    );
}

/// Refuse executable or protocol-relative links even for manually constructed state.
fn open_link(output: &mut String, url: Option<&str>) -> bool {
    let Some(url) =
        url.filter(|url| url.starts_with("https://") && !url.chars().any(char::is_control))
    else {
        return false;
    };

    let _ = write!(
        output,
        r#"<a href="{}" target="_blank" rel="noopener noreferrer">"#,
        xml(url)
    );

    true
}

/// Borrow XML content rather than allocate an escaped copy for every label and attribute.
fn xml(value: &str) -> Escaped<'_> {
    Escaped(value)
}

/// Streaming XML encoder; unchanged UTF-8 spans go directly into the destination formatter.
struct Escaped<'a>(&'a str);

impl std::fmt::Display for Escaped<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut start = 0;

        for (index, byte) in self.0.bytes().enumerate() {
            let replacement = match byte {
                b'&' => "&amp;",
                b'<' => "&lt;",
                b'>' => "&gt;",
                b'"' => "&quot;",
                b'\'' => "&apos;",
                _ => continue,
            };

            // XML metacharacters are ASCII, so both slice boundaries are UTF-8 boundaries.
            formatter.write_str(&self.0[start..index])?;
            formatter.write_str(replacement)?;
            start = index + 1;
        }

        formatter.write_str(&self.0[start..])
    }
}

#[cfg(test)]
mod tests;
