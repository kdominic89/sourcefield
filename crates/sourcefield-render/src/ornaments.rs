//! Approved SOURCEFIELD node ornaments, independent from labels and graph geometry.

use std::fmt::Write as _;

use sourcefield_core::{Node, NodeKind, ProfileState};

use super::{Palette, icons, node_color, path};

/// Render local ornaments without introducing a transform around stable labels.
pub(super) fn render(
    output: &mut String,
    node: &Node,
    radius: f32,
    state: &ProfileState,
    p: Palette,
    motion: bool,
) {
    if node.kind == NodeKind::Domain {
        if node.scope.as_deref() == Some("organization") {
            organization(output, node, radius, state, p);
        } else {
            personal(output, node, radius, state, p);
        }

        return;
    }

    let color = node_color(node, p);

    // Sibling transforms preserve opposite motion instead of canceling nested rotations.
    let _ = write!(
        output,
        "<g class=\"rotate\" data-project-ring=\"outer\"><circle r=\"{radius}\" \
         fill=\"none\" stroke=\"{color}\" stroke-opacity=\".4\" stroke-dasharray=\"3 11\"/>\
         <circle cx=\"{radius}\" r=\"3\" fill=\"{color}\" filter=\"url(#glow)\"/></g>\
         <g class=\"rotate reverse\" data-project-ring=\"middle\"><circle r=\"{}\" \
         fill=\"none\" stroke=\"{color}\" stroke-opacity=\".4\" stroke-dasharray=\"34 120\"/></g>",
        (radius - 8.0).max(1.0)
    );

    let icon = node
        .icon
        .as_deref()
        .and_then(|id| sourcefield_core::resolve_icon(&state.icons, id));

    if icon.is_none() && node.visual.as_deref() == Some("finance") {
        vault(output, color, p);
        return;
    }

    let _ = write!(
        output,
        "<circle r=\"{}\" fill=\"{}\" stroke=\"{color}\" stroke-opacity=\".65\"/>",
        (radius - 16.0).max(1.0),
        p.surface
    );

    if let Some(icon) = icon {
        icons::render(output, node, icon, (radius - 16.0).max(1.0), p, motion);
        return;
    }

    if node.visual.as_deref() == Some("trace") {
        radar(output, color);
        return;
    }

    let glyph = match node.visual.as_deref() {
        Some("database" | "provider" | "migrations" | "packages") => {
            "M0 -23L20 -11V12L0 24L-20 12V-11ZM-20 -11L0 0L20 -11M0 0V24"
        }
        Some("hierarchy") => {
            "M-5 -22H5V-12H-5ZM0 -12V-2M-17 -2H17M-17 -2V8M17 -2V8M-22 8H-12V18H-22ZM12 8H22V18H12Z"
        }
        Some("theme" | "lab") => "M-17 -12H17V14H-17ZM-17 -4H17M-8 -12V14",
        _ => "M-15 -10L-25 0L-15 10M15 -10L25 0L15 10M5 -17L-5 17",
    };

    let scale = match node.visual.as_deref() {
        Some("dotfiles") => Some("0.58"),
        Some("theme") => Some("0.72"),
        _ => None,
    };

    if let Some(scale) = scale {
        // Preserve line weight while fitting the glyph inside its existing inner disk.
        let _ = write!(
            output,
            "<path d=\"{glyph}\" fill=\"none\" stroke=\"{color}\" stroke-opacity=\".95\" \
             stroke-width=\"1.2\" transform=\"scale({scale})\" \
             vector-effect=\"non-scaling-stroke\"/>"
        );
    } else {
        path(output, glyph, color, ".95", "");
    }
}

/// Replace the finance disk with the approved vault, retaining its original proportions.
fn vault(output: &mut String, color: &str, p: Palette) {
    let _ = write!(
        output,
        "<g transform=\"scale(0.8)\" data-glyph=\"vault\"><path \
         d=\"M0 -35 L31 -17 L31 18 L0 36 L-31 18 L-31 -17Z\" fill=\"{}\" \
         fill-opacity=\".92\" stroke=\"{color}\" stroke-width=\"1.5\"/>\
         <rect x=\"-13\" y=\"-2\" width=\"26\" height=\"19\" rx=\"5\" fill=\"none\" \
         stroke=\"{color}\" stroke-width=\"1.8\"/><path \
         d=\"M-8 -2 V-8 A8 8 0 0 1 8 -8 V-2\" fill=\"none\" stroke=\"{color}\" \
         stroke-width=\"1.8\"/><circle cy=\"7\" r=\"2.3\" fill=\"{color}\"/></g>",
        p.surface
    );
}

/// Move only the radar wedge so its crosshair and signal locations remain readable.
fn radar(output: &mut String, color: &str) {
    for (radius, opacity) in [(12, ".23"), (23, ".30")] {
        let _ = write!(
            output,
            "<circle r=\"{radius}\" fill=\"none\" stroke=\"{color}\" \
             stroke-opacity=\"{opacity}\" stroke-width=\".8\"/>"
        );
    }

    let _ = write!(
        output,
        "<g class=\"rotate scan\"><path d=\"M0 0 L27 -11 A29 29 0 0 0 23 -18Z\" \
         fill=\"{color}\" fill-opacity=\".23\"/></g>"
    );

    path(output, "M-25 0H25M0 -25V25M0 0L25 -15", color, ".68", "");

    for (x, y, radius) in [(17, -12, "2.5"), (-19, 15, "2")] {
        let _ = write!(
            output,
            "<circle cx=\"{x}\" cy=\"{y}\" r=\"{radius}\" fill=\"{color}\" \
             filter=\"url(#glow)\"/>"
        );
    }
}

/// Keep project satellites stationary while their independent opacity signals stagger.
fn personal(output: &mut String, node: &Node, radius: f32, state: &ProfileState, p: Palette) {
    let _ = write!(
        output,
        "<g class=\"rotate\" data-ring=\"personal-outer\"><circle r=\"{radius}\" \
         fill=\"none\" stroke=\"{}\" stroke-opacity=\".4\" stroke-dasharray=\"3 8\"/></g>\
         <g class=\"rotate reverse\" data-ring=\"personal-inner\"><circle r=\"48.3\" \
         fill=\"none\" stroke=\"{}\" stroke-opacity=\".4\" stroke-dasharray=\"1 9\"/></g>\
         <circle r=\"40\" fill=\"{}\" fill-opacity=\".84\" \
         stroke=\"url(#personal-gradient)\" stroke-opacity=\".65\"/>\
         <circle r=\"26\" fill=\"none\" stroke=\"{}\" stroke-opacity=\".45\"/>\
         <circle r=\"7\" fill=\"{}\" class=\"pulse\" filter=\"url(#glow)\"/>",
        p.purple, p.mint, p.surface, p.purple, p.mint
    );

    let count = state
        .nodes
        .iter()
        .filter(|item| {
            item.kind == NodeKind::Project && item.show_in_readme && item.domain == node.domain
        })
        .count();

    for index in 0..count {
        let angle = std::f64::consts::TAU * index as f64 / count as f64 - 1.1;
        let x = angle.cos() * f64::from(radius);
        let y = angle.sin() * f64::from(radius);
        let delay = -(index as f64 * 0.8);
        let _ = write!(
            output,
            "<circle cx=\"{x:.3}\" cy=\"{y:.3}\" r=\"4\" fill=\"{}\" \
             fill-opacity=\".18\" stroke=\"{}\" stroke-width=\"1.1\" \
             data-signal-delay=\"{delay:.1}\" \
             class=\"signal\" data-satellite=\"personal-project\"/>",
            p.mint, p.purple
        );
    }
}

/// Center the inner orbit between the 40-unit core ring and the outer package orbit.
fn organization(output: &mut String, node: &Node, radius: f32, state: &ProfileState, p: Palette) {
    let inner = (40.0 + radius) / 2.0;
    let _ = write!(
        output,
        "<g class=\"rotate reverse\" data-ring=\"organization-outer\"><circle \
         r=\"{radius}\" fill=\"none\" stroke=\"{}\" stroke-opacity=\".4\" \
         stroke-dasharray=\"2 10\"/></g><g class=\"rotate\" \
         data-ring=\"organization-inner\"><circle r=\"{inner}\" fill=\"none\" \
         stroke=\"{}\" stroke-opacity=\".26\" stroke-dasharray=\"34 120\"/></g>\
         <circle r=\"40\" fill=\"none\" stroke=\"{}\" stroke-opacity=\".30\"/>\
         <g transform=\"scale(0.49)\" data-glyph=\"organization-data\"><path \
         d=\"M0 -65 L56 -32 L56 32 L0 65 L-56 32 L-56 -32Z\" fill=\"{}\" \
         fill-opacity=\".92\" stroke=\"url(#organization-gradient)\" stroke-width=\"2.4\"/>\
         <path d=\"M-30 -14 H30 M-30 0 H30 M-30 14 H30\" fill=\"none\" stroke=\"{}\" \
         stroke-opacity=\".85\" stroke-width=\"2\"/>",
        p.amber, p.blue, p.amber, p.surface, p.blue
    );

    for y in [-14, 0, 14] {
        let _ = write!(
            output,
            "<circle cx=\"-36\" cy=\"{y}\" r=\"2.7\" fill=\"{}\"/>",
            p.amber
        );
    }

    output.push_str("</g>");
    let count = state
        .nodes
        .iter()
        .filter(|item| {
            item.kind == NodeKind::Package && item.show_in_readme && item.domain == node.domain
        })
        .count();

    for index in 0..count {
        let angle = std::f64::consts::TAU * index as f64 / count as f64 - 1.05;
        let x = angle.cos() * f64::from(radius);
        let y = angle.sin() * f64::from(radius);
        let delay = -(index as f64 * 0.65);
        let color = if index * 2 < count { p.blue } else { p.amber };

        let _ = write!(
            output,
            "<g transform=\"translate({x:.3} {y:.3})\" \
             data-signal-delay=\"{delay:.2}\" \
             class=\"signal\" data-satellite=\"nuget-package\"><path \
             d=\"M0 -4 L3.5 -2 L3.5 2 L0 4 L-3.5 2 L-3.5 -2Z\" fill=\"{}\" \
             stroke=\"{color}\" stroke-width=\"1\"/><circle r=\"1\" fill=\"{color}\"/></g>",
            p.surface
        );
    }
}
