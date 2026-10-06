//! Admission checks for the shared icon catalog, including composed resource budgets.

use super::*;

/// Validate all consumer definitions before serializing or rendering numeric data.
///
/// Counts apply across a composed catalog so individually valid organization imports cannot
/// collectively exceed the rendering budget. Built-ins have a separate fixed, tested budget.
pub fn validate_icon_catalog(catalog: &IconCatalog) -> Result<(), ValidationError> {
    if catalog.len() > MAX_ICON_DEFINITIONS {
        return Err(invalid("catalog exceeds 128 definitions"));
    }

    let mut element_count = 0_usize;
    let mut command_count = 0_usize;

    for (id, definition) in catalog {
        validate_icon_id(id)?;
        validate_definition(definition)
            .map_err(|error| with_context(error, &format!("icon '{id}'")))?;
        element_count = element_count.saturating_add(definition.elements.len());

        for element in &definition.elements {
            if let IconGeometry::Path { commands } = &element.geometry {
                command_count = command_count.saturating_add(commands.len());
            }
        }

        if element_count > MAX_CATALOG_ICON_ELEMENTS
            || command_count > MAX_CATALOG_ICON_PATH_COMMANDS
        {
            return Err(invalid("catalog exceeds aggregate geometry budget"));
        }
    }

    Ok(())
}

/// Require a syntactically valid reference to a known built-in or consumer definition.
pub fn validate_icon_reference(
    catalog: &IconCatalog,
    reference: &str,
) -> Result<(), ValidationError> {
    if !reference.starts_with("builtin:") {
        validate_icon_id(reference)?;
    }

    if resolve_icon(catalog, reference).is_none() {
        return Err(invalid("unresolved icon reference"));
    }

    Ok(())
}

/// Bound emitted geometry across all selected references, including repeated and built-in icons.
///
/// Validate the catalog separately before calling this function. Every selected reference counts,
/// even when a renderer might hide its node, so the limit remains safe across views and filters.
/// The pass borrows definitions and stops at the first missing reference or exceeded budget.
pub fn validate_icon_usage<'a>(
    catalog: &IconCatalog,
    references: impl IntoIterator<Item = &'a str>,
) -> Result<(), ValidationError> {
    let mut element_count = 0_usize;
    let mut command_count = 0_usize;

    for reference in references {
        validate_icon_reference(catalog, reference)?;
        let definition =
            resolve_icon(catalog, reference).ok_or_else(|| invalid("unresolved icon reference"))?;

        element_count = element_count.saturating_add(definition.elements.len());

        if element_count > MAX_RENDERED_ICON_ELEMENTS {
            return Err(invalid("selected references exceed 8192 rendered elements"));
        }

        for element in &definition.elements {
            if let IconGeometry::Path { commands } = &element.geometry {
                command_count = command_count.saturating_add(commands.len());

                if command_count > MAX_RENDERED_ICON_PATH_COMMANDS {
                    return Err(invalid(
                        "selected references exceed 32768 rendered path commands",
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Validate one lowercase ASCII local identifier, limited to 64 bytes.
///
/// The first character must be a letter; subsequent characters may also be digits or hyphens.
pub fn validate_local_icon_id(id: &str) -> Result<(), ValidationError> {
    if id.is_empty()
        || id.len() > 64
        || !id.as_bytes()[0].is_ascii_lowercase()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(invalid(
            "local identifier must match [a-z][a-z0-9-]* with at most 64 bytes",
        ));
    }

    Ok(())
}

/// Share qualified icon syntax between resolved catalogs and deferred authored references.
pub(crate) fn validate_icon_id(id: &str) -> Result<(), ValidationError> {
    if let Some((organization, local)) = id.split_once('/') {
        // Existing organization scopes keep their original grammar when icons are added.
        crate::organizations::validate_local_id(organization).map_err(|_| {
            invalid(
                "organization namespace must use ASCII letters, digits, '-' or '_' and be nonempty",
            )
        })?;
        validate_local_icon_id(local)?;
    } else {
        validate_local_icon_id(id)?;
    }

    Ok(())
}

/// Validate one motif independently; built-in tests use the same admission path as custom art.
fn validate_definition(definition: &IconDefinition) -> Result<(), ValidationError> {
    let radius = definition.radius;

    if !positive_bounded(radius, MAX_ICON_RADIUS) {
        return Err(invalid(
            "authored radius must be finite, positive, and at most 64",
        ));
    }

    if definition.elements.is_empty() || definition.elements.len() > MAX_ICON_ELEMENTS {
        return Err(invalid("definition must contain 1 through 64 elements"));
    }

    for (index, element) in definition.elements.iter().enumerate() {
        validate_element(element, radius)
            .map_err(|error| with_context(error, &format!("element {index}")))?;
    }

    Ok(())
}

/// Keep element-specific errors scoped to their position in the authored definition.
fn validate_element(element: &IconElement, radius: f32) -> Result<(), ValidationError> {
    if !positive_bounded(element.stroke_width, 8.0)
        || !element.opacity.is_finite()
        || !(0.0..=1.0).contains(&element.opacity)
        || matches!(element.motion, Some(IconMotion::Signal { phase: 3.. }))
    {
        return Err(invalid("element width, opacity, or signal phase"));
    }

    validate_geometry(&element.geometry, radius)
}

/// Check primitive coordinates and dimensions before any renderer arithmetic occurs.
fn validate_geometry(geometry: &IconGeometry, radius: f32) -> Result<(), ValidationError> {
    let valid = match geometry {
        IconGeometry::Path { commands } => return validate_path(commands, radius),
        IconGeometry::Circle { center, radius: r } => ellipse_fits(*center, [*r, *r], radius),
        IconGeometry::Ellipse { center, radii } => ellipse_fits(*center, *radii, radius),
        IconGeometry::Rect {
            origin,
            size,
            corner_radius,
        } => {
            point_fits(*origin, radius)
                && size
                    .iter()
                    .all(|value| positive_bounded(*value, radius * 2.0))
                && point_fits([origin[0] + size[0], origin[1] + size[1]], radius)
                && corner_radius.is_finite()
                && *corner_radius >= 0.0
                && *corner_radius <= size[0].min(size[1]) * 0.5
        }
    };

    if !valid {
        return Err(invalid("primitive geometry exceeds authored bounds"));
    }

    Ok(())
}

/// Track contours explicitly so close and drawing commands cannot precede a valid move.
fn validate_path(commands: &[IconPathCommand], radius: f32) -> Result<(), ValidationError> {
    if commands.is_empty() || commands.len() > MAX_ICON_PATH_COMMANDS {
        return Err(invalid("path must contain 1 through 256 commands"));
    }

    let mut contour_open = false;
    let mut contour_drawn = false;

    for command in commands {
        match command {
            IconPathCommand::Move { .. } => {
                if contour_open && !contour_drawn {
                    return Err(invalid("path contains an empty contour"));
                }

                contour_open = true;
                contour_drawn = false;
            }
            IconPathCommand::Close {} => {
                if !contour_open || !contour_drawn {
                    return Err(invalid("close requires a nonempty open contour"));
                }

                contour_open = false;
            }
            _ => {
                if !contour_open {
                    return Err(invalid("path contour must start with a move"));
                }

                contour_drawn = true;
            }
        }

        if !command_fits(command, radius) {
            return Err(invalid(
                "path coordinates, radii, or rotation exceed authored bounds",
            ));
        }
    }

    if !contour_drawn {
        return Err(invalid("path contains an empty contour"));
    }

    Ok(())
}

/// Bound every numeric path field, including controls that do not lie on the visible curve.
fn command_fits(command: &IconPathCommand, radius: f32) -> bool {
    match command {
        IconPathCommand::Move { to } | IconPathCommand::Line { to } => point_fits(*to, radius),
        IconPathCommand::Horizontal { x } => coordinate_fits(*x, radius),
        IconPathCommand::Vertical { y } => coordinate_fits(*y, radius),
        IconPathCommand::Quadratic { control, to } => {
            point_fits(*control, radius) && point_fits(*to, radius)
        }
        IconPathCommand::Cubic {
            control1,
            control2,
            to,
        } => {
            point_fits(*control1, radius)
                && point_fits(*control2, radius)
                && point_fits(*to, radius)
        }
        IconPathCommand::Arc {
            radii,
            rotation,
            to,
            ..
        } => {
            radii
                .iter()
                .all(|value| positive_bounded(*value, radius * 2.0))
                && rotation.is_finite()
                && (-360.0..=360.0).contains(rotation)
                && point_fits(*to, radius)
        }
        IconPathCommand::Close {} => true,
    }
}

/// Prevent primitive extents from exceeding the authored square before circular clipping.
fn ellipse_fits(center: [f32; 2], radii: [f32; 2], radius: f32) -> bool {
    point_fits(center, radius)
        && radii.iter().all(|value| positive_bounded(*value, radius))
        && center
            .iter()
            .zip(radii)
            .all(|(coordinate, extent)| coordinate.abs() + extent <= radius)
}

/// Validate both coordinates together without allocating a temporary geometry object.
fn point_fits(point: [f32; 2], radius: f32) -> bool {
    point.iter().all(|value| coordinate_fits(*value, radius))
}

/// Require finite coordinates even when callers construct Rust values without deserialization.
fn coordinate_fits(value: f32, radius: f32) -> bool {
    value.is_finite() && (-radius..=radius).contains(&value)
}

/// Keep dimension arithmetic finite and bounded at admission.
fn positive_bounded(value: f32, maximum: f32) -> bool {
    value.is_finite() && value > 0.0 && value <= maximum
}

/// Use the public validation error type without exposing untrusted document contents in errors.
fn invalid(detail: &str) -> ValidationError {
    ValidationError::InvalidValue(format!("icon {detail}"))
}

/// Add only validated identifiers and numeric indexes to geometry diagnostics.
fn with_context(error: ValidationError, context: &str) -> ValidationError {
    match error {
        ValidationError::InvalidValue(detail) => {
            ValidationError::InvalidValue(format!("{context}: {detail}"))
        }
        other => other,
    }
}
