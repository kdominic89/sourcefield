//! Shared typed icon definitions and bounded admission for authored vector artwork.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ValidationError;

mod builtins;
mod validation;

#[cfg(test)]
mod tests;

pub use builtins::builtin_icon;
pub(crate) use validation::validate_icon_id;
pub use validation::{
    validate_icon_catalog, validate_icon_reference, validate_icon_usage, validate_local_icon_id,
};

/// Maximum consumer-authored definitions in one composed catalog.
pub const MAX_ICON_DEFINITIONS: usize = 128;
/// Maximum flat elements in one icon.
pub const MAX_ICON_ELEMENTS: usize = 64;
/// Maximum absolute commands in one path.
pub const MAX_ICON_PATH_COMMANDS: usize = 256;
/// Maximum elements across all definitions in one composed catalog.
pub const MAX_CATALOG_ICON_ELEMENTS: usize = 2048;
/// Maximum path commands across all definitions in one composed catalog.
pub const MAX_CATALOG_ICON_PATH_COMMANDS: usize = 16384;
/// Maximum elements emitted by all selected icon references, including built-ins.
pub const MAX_RENDERED_ICON_ELEMENTS: usize = 8192;
/// Maximum path commands emitted by all selected icon references, including built-ins.
pub const MAX_RENDERED_ICON_PATH_COMMANDS: usize = 32768;
/// Largest authored radius in design coordinates.
pub const MAX_ICON_RADIUS: f32 = 64.0;

/// Deterministically ordered definitions shared by project and state icon references.
pub type IconCatalog = BTreeMap<String, IconDefinition>;

/// A flat, bounded vector motif centered at the origin of its authored coordinate system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconDefinition {
    /// Authored radius; rendering may shrink this coordinate system but never enlarge it.
    pub radius: f32,
    /// Ordered shapes; later elements paint over earlier elements.
    pub elements: Vec<IconElement>,
}

/// One vector primitive with semantic, renderer-owned paint and motion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IconElement {
    /// Typed geometry, without raw SVG path text or arbitrary attributes.
    pub geometry: IconGeometry,
    /// Interior paint; defaults to no fill.
    #[serde(default)]
    pub fill: IconPaint,
    /// Outline paint; defaults to the node's accent color.
    #[serde(default = "accent_paint")]
    pub stroke: IconPaint,
    /// Outline width in authored coordinates, greater than zero and at most eight.
    #[serde(default = "default_stroke_width")]
    pub stroke_width: f32,
    /// Element opacity in the inclusive range zero through one.
    #[serde(default = "full_opacity")]
    pub opacity: f32,
    /// Outline endpoint shape; defaults to round.
    #[serde(default)]
    pub line_cap: IconLineCap,
    /// Outline corner shape; defaults to round.
    #[serde(default)]
    pub line_join: IconLineJoin,
    /// Optional fixed animation preset; omitted elements remain static.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<IconMotion>,
}

impl IconElement {
    /// Wrap a primitive with the same style defaults used for deserialization.
    pub fn new(geometry: IconGeometry) -> Self {
        Self {
            geometry,
            fill: IconPaint::None,
            stroke: IconPaint::Accent,
            stroke_width: default_stroke_width(),
            opacity: full_opacity(),
            line_cap: IconLineCap::Round,
            line_join: IconLineJoin::Round,
            motion: None,
        }
    }
}

/// Supported non-recursive primitives in authored coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "kebab-case", deny_unknown_fields)]
pub enum IconGeometry {
    /// An absolute path consisting only of typed commands.
    Path {
        /// Ordered contours; each contour starts with a move and contains a drawing command.
        commands: Vec<IconPathCommand>,
    },
    /// A circle.
    Circle {
        /// Center coordinates.
        center: [f32; 2],
        /// Positive radius.
        radius: f32,
    },
    /// An axis-aligned ellipse.
    Ellipse {
        /// Center coordinates.
        center: [f32; 2],
        /// Positive horizontal and vertical radii.
        radii: [f32; 2],
    },
    /// An axis-aligned rectangle with optional uniformly rounded corners.
    Rect {
        /// Top-left coordinates.
        origin: [f32; 2],
        /// Positive width and height.
        size: [f32; 2],
        /// Corner radius, at most half the smaller dimension; defaults to zero.
        #[serde(default)]
        corner_radius: f32,
    },
}

/// One absolute path instruction; no instruction contains unparsed SVG syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum IconPathCommand {
    /// Start a new contour.
    Move {
        /// Absolute starting coordinates.
        to: [f32; 2],
    },
    /// Draw a straight segment.
    Line {
        /// Absolute endpoint.
        to: [f32; 2],
    },
    /// Draw a horizontal segment.
    Horizontal {
        /// Absolute endpoint abscissa.
        x: f32,
    },
    /// Draw a vertical segment.
    Vertical {
        /// Absolute endpoint ordinate.
        y: f32,
    },
    /// Draw a quadratic Bezier curve.
    Quadratic {
        /// Absolute control point.
        control: [f32; 2],
        /// Absolute endpoint.
        to: [f32; 2],
    },
    /// Draw a cubic Bezier curve.
    Cubic {
        /// Absolute control point adjacent to the starting point.
        control1: [f32; 2],
        /// Absolute control point adjacent to the endpoint.
        control2: [f32; 2],
        /// Absolute endpoint.
        to: [f32; 2],
    },
    /// Draw an elliptical arc; the renderer clips any SVG radius correction to the icon disk.
    Arc {
        /// Positive horizontal and vertical radii, at most twice the authored radius.
        radii: [f32; 2],
        /// Ellipse rotation in degrees in the inclusive range -360 through 360.
        rotation: f32,
        /// Select the larger angular arc when true.
        large_arc: bool,
        /// Select the positive angular direction when true.
        sweep: bool,
        /// Absolute endpoint.
        to: [f32; 2],
    },
    /// Close a nonempty contour; another contour must start with a move.
    Close {},
}

/// Semantic paint resolved by the renderer's theme; arbitrary colors and URLs are unsupported.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IconPaint {
    /// No paint.
    #[default]
    None,
    /// The surrounding node's accent.
    Accent,
    /// The theme's inner node surface.
    Surface,
    /// The theme's recessed surface.
    Recess,
    /// The theme's mint accent.
    Mint,
    /// The theme's purple accent.
    Purple,
    /// The theme's amber accent.
    Amber,
    /// The theme's blue accent.
    Blue,
}

/// Allowed stroke endpoint treatment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IconLineCap {
    /// End at the endpoint with no extension.
    Butt,
    /// Extend with a semicircular cap.
    #[default]
    Round,
    /// Extend with a half-width square cap.
    Square,
}

/// Allowed stroke corner treatment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IconLineJoin {
    /// Extend edges to their intersection, subject to the renderer's fixed miter limit.
    Miter,
    /// Join with a circular arc.
    #[default]
    Round,
    /// Join with a straight bevel.
    Bevel,
}

/// Fixed animation presets shared by native SVG and the interactive runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum IconMotion {
    /// A 5.4-second opacity cycle from 0.35 to 1 at 45 percent and back to 0.35.
    Signal {
        /// Phase zero, one, or two; delays are 0, -1.8, and -3.6 seconds respectively.
        phase: u8,
    },
}

/// Borrow one custom definition or a process-wide built-in without cloning its elements.
///
/// The reserved built-in namespace is resolved first so invalid catalogs cannot override it.
pub fn resolve_icon<'a>(catalog: &'a IconCatalog, reference: &str) -> Option<&'a IconDefinition> {
    if reference.starts_with("builtin:") {
        builtin_icon(reference)
    } else {
        catalog.get(reference)
    }
}

/// Default outline paint for authored elements.
fn accent_paint() -> IconPaint {
    IconPaint::Accent
}

/// Default width preserves the surrounding diagram's thin-line treatment.
fn default_stroke_width() -> f32 {
    1.2
}

/// Unanimated elements are fully visible unless explicitly faded.
fn full_opacity() -> f32 {
    1.0
}
