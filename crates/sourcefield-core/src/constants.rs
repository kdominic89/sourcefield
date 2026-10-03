//! Shared admission and geometry rules with multiple production consumers.

/// Maximum number of retained semantic history entries accepted by configuration and storage.
pub const MAX_HISTORY_ENTRIES: usize = 256;
/// Maximum graph nodes accepted by generation and interactive simulation.
pub const MAX_GRAPH_NODES: usize = 512;
/// Maximum graph edges accepted by generation and interactive simulation.
pub const MAX_GRAPH_EDGES: usize = 8192;
/// Vertical distance between package rows without explicit authored positions, in design pixels.
pub const PACKAGE_ROW_SPACING: f32 = 90.0;
/// Radius of an ownership-domain glyph, in design pixels.
pub const DOMAIN_RADIUS: f32 = 61.0;

/// Resolve the same authored or weight-derived project radius in graph and layout validation.
pub fn project_radius(project: &crate::ProjectConfig) -> f32 {
    project.radius.unwrap_or(42.0 + project.weight * 24.0)
}

/// Resolve a package glyph center using its explicit anchor or canonical package ordinal.
/// Callers use the same ID ordering as graph normalization; explicit coordinates are unchanged.
pub fn package_anchor(
    group_anchor: [f32; 2],
    package: &crate::PackageConfig,
    index: usize,
) -> [f32; 2] {
    package
        .anchor
        .unwrap_or_else(|| default_package_anchor(group_anchor, index))
}

/// Position an unauthored package center using the approved publication-column geometry.
pub fn default_package_anchor(group_anchor: [f32; 2], index: usize) -> [f32; 2] {
    [
        group_anchor[0] + 16.0,
        group_anchor[1] + 50.0 + index as f32 * PACKAGE_ROW_SPACING,
    ]
}
