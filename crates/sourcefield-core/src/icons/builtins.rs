//! Approved artwork expressed through the same geometry used by consumer definitions.

use std::sync::OnceLock;

use super::*;

static SOURCEFIELD: OnceLock<IconDefinition> = OnceLock::new();
static DATABASE_SAFE: OnceLock<IconDefinition> = OnceLock::new();

/// Borrow a fixed built-in definition by its complete reserved reference.
///
/// Definitions initialize once and remain shared for all nodes and render calls.
pub fn builtin_icon(reference: &str) -> Option<&'static IconDefinition> {
    match reference {
        "builtin:sourcefield" => Some(SOURCEFIELD.get_or_init(sourcefield)),
        "builtin:database-safe" => Some(DATABASE_SAFE.get_or_init(database_safe)),
        _ => None,
    }
}

/// Preserve the approved network's trimmed links and three staggered signal dots.
fn sourcefield() -> IconDefinition {
    let mut elements = Vec::with_capacity(14);
    let points = [
        ([-2.0, 2.0], 4.1, IconPaint::Mint),
        ([-18.0, -12.0], 3.0, IconPaint::Blue),
        ([13.0, -18.0], 3.2, IconPaint::Purple),
        ([20.0, 8.0], 3.0, IconPaint::Blue),
        ([-12.0, 20.0], 2.8, IconPaint::Mint),
    ];

    // Fixed preview endpoints avoid target-dependent floating-point trimming of approved art.
    for (start, end) in [
        ([-5.086, -0.7], [-15.742, -10.024]),
        ([0.46, -1.28], [11.08, -15.44]),
        ([1.956, 3.079], [17.106, 7.211]),
        ([-3.991, 5.584], [-10.64, 17.552]),
        ([-15.055, -12.57], [9.858, -17.392]),
        ([13.832, -14.91], [19.22, 5.103]),
    ] {
        let mut link = path(vec![
            IconPathCommand::Move { to: start },
            IconPathCommand::Line { to: end },
        ]);

        link.stroke = IconPaint::Mint;
        link.stroke_width = 1.25;
        link.opacity = 0.65;
        elements.push(link);
    }

    let mut phase = 0;

    for (index, (center, radius, color)) in points.into_iter().enumerate() {
        let mut node = IconElement::new(IconGeometry::Circle { center, radius });
        node.fill = IconPaint::Surface;
        node.stroke = color;
        node.stroke_width = 1.45;
        elements.push(node);

        if matches!(index, 0 | 2 | 4) {
            let mut dot = IconElement::new(IconGeometry::Circle {
                center,
                radius: radius * 0.45,
            });

            dot.fill = color;
            dot.stroke = IconPaint::None;
            dot.motion = Some(IconMotion::Signal { phase });
            elements.push(dot);
            phase += 1;
        }
    }

    IconDefinition {
        radius: 31.0,
        elements,
    }
}

/// Preserve the accepted open safe and the reference cylinder's three front details.
fn database_safe() -> IconDefinition {
    let mut body = IconElement::new(IconGeometry::Rect {
        origin: [-23.0, -21.0],
        size: [31.0, 42.0],
        corner_radius: 3.0,
    });

    body.fill = IconPaint::Recess;
    body.stroke = IconPaint::Amber;
    body.stroke_width = 1.45;

    let cylinder = path(vec![
        IconPathCommand::Move { to: [-17.7, -10.0] },
        IconPathCommand::Vertical { y: 8.9 },
        cylinder_arc(8.9),
        IconPathCommand::Vertical { y: -10.0 },
    ]);

    let mut lid = IconElement::new(IconGeometry::Ellipse {
        center: [-7.5, -10.0],
        radii: [10.2, 4.2],
    });

    lid.stroke = IconPaint::Amber;
    lid.stroke_width = 1.1;
    let mut elements = vec![body, cylinder, lid];

    for y in [-3.7, 2.6] {
        elements.push(path(vec![
            IconPathCommand::Move { to: [-17.7, y] },
            cylinder_arc(y),
        ]));
    }

    for (dot_y, slot_y, control_y, end_y) in [
        (-3.95, -2.95, -2.1, -3.25),
        (2.35, 3.35, 4.2, 3.05),
        (8.65, 9.65, 10.5, 9.35),
    ] {
        let mut dot = IconElement::new(IconGeometry::Circle {
            center: [-14.9, dot_y],
            radius: 0.53,
        });

        dot.fill = IconPaint::Amber;
        dot.stroke = IconPaint::None;
        elements.push(dot);

        let mut slot = path(vec![
            IconPathCommand::Move {
                to: [-10.4, slot_y],
            },
            IconPathCommand::Quadratic {
                control: [-6.1, control_y],
                to: [-2.0, end_y],
            },
        ]);

        slot.stroke_width = 1.0;
        elements.push(slot);
    }

    let mut door = path(vec![
        IconPathCommand::Move { to: [8.0, -18.0] },
        IconPathCommand::Line { to: [23.0, -22.0] },
        IconPathCommand::Vertical { y: 22.0 },
        IconPathCommand::Line { to: [8.0, 18.0] },
        IconPathCommand::Close {},
    ]);

    door.fill = IconPaint::Surface;
    door.stroke_width = 1.45;
    elements.push(door);

    let mut handle = path(vec![
        IconPathCommand::Move { to: [18.0, -3.0] },
        IconPathCommand::Vertical { y: 4.0 },
    ]);

    handle.stroke_width = 1.45;
    elements.push(handle);

    IconDefinition {
        radius: 35.0,
        elements,
    }
}

/// Keep the reference cylinder's lower half-ellipse consistent across its connected tiers.
fn cylinder_arc(y: f32) -> IconPathCommand {
    IconPathCommand::Arc {
        radii: [10.2, 4.2],
        rotation: 0.0,
        large_arc: false,
        sweep: false,
        to: [2.7, y],
    }
}

/// Most safe strokes share the thin amber treatment; callers override the outer silhouette.
fn path(commands: Vec<IconPathCommand>) -> IconElement {
    let mut element = IconElement::new(IconGeometry::Path { commands });
    element.stroke = IconPaint::Amber;
    element.stroke_width = 1.1;

    element
}
