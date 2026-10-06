//! Streaming SVG emission for shared, validated icon definitions.

use std::fmt::{self, Write as _};

use sourcefield_core::{
    IconDefinition, IconElement, IconGeometry, IconLineCap, IconLineJoin, IconMotion, IconPaint,
    IconPathCommand, Node, NodeKind, ProfileState, resolve_icon,
};

use super::{Palette, node_color, xml};

/// Limit motion rules to icons that actually appear in the rendered project field.
pub(super) fn has_signals(state: &ProfileState) -> bool {
    state
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Project && node.show_in_readme)
        .filter_map(|node| node.icon.as_deref())
        .filter_map(|id| resolve_icon(&state.icons, id))
        .any(|icon| icon.elements.iter().any(|element| element.motion.is_some()))
}

/// Keep authored signal timing identical in standalone SVG and the browser stylesheet.
pub(super) fn motion_styles(output: &mut String) {
    output.push_str(concat!(
        "<style>.icon-signal{animation:icon-signal 5.4s ease-in-out infinite}",
        ".icon-signal-phase-1{animation-delay:-1.8s}",
        ".icon-signal-phase-2{animation-delay:-3.6s}",
        "@keyframes icon-signal{0%,100%{opacity:.35}45%{opacity:1}}",
        "@media(prefers-reduced-motion:reduce){.icon-signal{animation:none!important}}",
        "</style>"
    ));
}

/// Clip in node coordinates before scaling so large authored geometry cannot cover node rings.
pub(super) fn render(
    output: &mut String,
    node: &Node,
    definition: &IconDefinition,
    inner_radius: f32,
    palette: Palette,
    motion: bool,
) {
    let id = ClipId(&node.id);
    let scale = (inner_radius / definition.radius).min(1.0);
    let accent = node_color(node, palette);

    let _ = write!(
        output,
        "<defs><clipPath id=\"{id}\"><circle r=\"{inner_radius}\"/></clipPath></defs>\
         <g clip-path=\"url(#{id})\"><g data-icon=\"{}\" transform=\"scale({scale})\">",
        xml(node.icon.as_deref().unwrap_or_default())
    );

    for element in &definition.elements {
        geometry(output, &element.geometry);
        style(output, element, palette, accent, motion);
        output.push_str("/>");
    }

    output.push_str("</g></g>");
}

/// Encode every byte rather than interpolate authored IDs into a resource URL.
struct ClipId<'a>(&'a str);

impl fmt::Display for ClipId<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("icon-clip-")?;

        for byte in self.0.bytes() {
            write!(formatter, "{byte:02x}")?;
        }

        Ok(())
    }
}

/// Stream geometry directly into the final buffer without allocating intermediate path strings.
fn geometry(output: &mut String, shape: &IconGeometry) {
    match shape {
        IconGeometry::Path { commands } => {
            output.push_str("<path d=\"");

            for command in commands {
                path_command(output, command);
            }

            output.push('"');
        }
        IconGeometry::Circle { center, radius } => {
            let [x, y] = center;
            let _ = write!(output, "<circle cx=\"{x}\" cy=\"{y}\" r=\"{radius}\"");
        }
        IconGeometry::Ellipse { center, radii } => {
            let [x, y] = center;
            let [rx, ry] = radii;
            let _ = write!(
                output,
                "<ellipse cx=\"{x}\" cy=\"{y}\" rx=\"{rx}\" ry=\"{ry}\""
            );
        }
        IconGeometry::Rect {
            origin,
            size,
            corner_radius,
        } => {
            let [x, y] = origin;
            let [width, height] = size;
            let _ = write!(
                output,
                "<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" rx=\"{corner_radius}\""
            );
        }
    }
}

/// Keep path syntax renderer-owned; typed coordinates are the only authored substitutions.
fn path_command(output: &mut String, command: &IconPathCommand) {
    match command {
        IconPathCommand::Move { to: [x, y] } => {
            let _ = write!(output, "M{x} {y}");
        }
        IconPathCommand::Line { to: [x, y] } => {
            let _ = write!(output, "L{x} {y}");
        }
        IconPathCommand::Horizontal { x } => {
            let _ = write!(output, "H{x}");
        }
        IconPathCommand::Vertical { y } => {
            let _ = write!(output, "V{y}");
        }
        IconPathCommand::Quadratic {
            control: [cx, cy],
            to: [x, y],
        } => {
            let _ = write!(output, "Q{cx} {cy} {x} {y}");
        }
        IconPathCommand::Cubic {
            control1: [cx1, cy1],
            control2: [cx2, cy2],
            to: [x, y],
        } => {
            let _ = write!(output, "C{cx1} {cy1} {cx2} {cy2} {x} {y}");
        }
        IconPathCommand::Arc {
            radii: [rx, ry],
            rotation,
            large_arc,
            sweep,
            to: [x, y],
        } => {
            let _ = write!(
                output,
                "A{rx} {ry} {rotation} {} {} {x} {y}",
                u8::from(*large_arc),
                u8::from(*sweep)
            );
        }
        IconPathCommand::Close {} => output.push('Z'),
    }
}

/// Resolve semantic colors and bounded stroke styles without exposing arbitrary SVG attributes.
fn style(output: &mut String, element: &IconElement, palette: Palette, accent: &str, motion: bool) {
    let fill = paint(&element.fill, palette, accent);
    let stroke = paint(&element.stroke, palette, accent);
    let width = element.stroke_width;
    let opacity = element.opacity;
    let cap = match element.line_cap {
        IconLineCap::Butt => "butt",
        IconLineCap::Round => "round",
        IconLineCap::Square => "square",
    };
    let join = match element.line_join {
        IconLineJoin::Miter => "miter",
        IconLineJoin::Round => "round",
        IconLineJoin::Bevel => "bevel",
    };

    let _ = write!(
        output,
        " fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{width}\" opacity=\"{opacity}\" \
         stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\""
    );

    if motion && let Some(IconMotion::Signal { phase }) = element.motion {
        let _ = write!(output, " class=\"icon-signal icon-signal-phase-{phase}\"");
    }
}

/// Borrow colors from the selected theme and the existing visual accent policy.
fn paint<'a>(paint: &IconPaint, palette: Palette, accent: &'a str) -> &'a str {
    match paint {
        IconPaint::None => "none",
        IconPaint::Accent => accent,
        IconPaint::Surface => palette.surface,
        IconPaint::Recess => palette.recess,
        IconPaint::Mint => palette.mint,
        IconPaint::Purple => palette.purple,
        IconPaint::Amber => palette.amber,
        IconPaint::Blue => palette.blue,
    }
}
