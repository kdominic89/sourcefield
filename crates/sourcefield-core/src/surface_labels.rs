//! Conservative text envelopes matching the SVG's title positions and font sizes.

use crate::{Config, ValidationError};

struct LabelBox<'a> {
    id: &'a str,
    project: bool,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

struct Circle<'a> {
    id: &'a str,
    project: bool,
    x: f32,
    y: f32,
    radius: f32,
}

/// Reject text envelopes that clip the canvas or overlap other primary labels and circles.
pub(crate) fn validate_surface_labels(config: &Config) -> Result<(), ValidationError> {
    let scale = config.render.width as f32 / 1800.0;
    let mut labels = Vec::with_capacity(config.projects.len() * 2 + config.domains.len());
    let mut circles = Vec::with_capacity(config.projects.len() + config.domains.len());

    for domain in &config.domains {
        labels.push(label_box(
            &domain.id,
            false,
            &domain.label,
            domain.anchor,
            91.0 * scale,
            21.0 * scale,
        ));
        circles.push(Circle {
            id: &domain.id,
            project: false,
            x: domain.anchor[0],
            y: domain.anchor[1],
            radius: crate::DOMAIN_RADIUS,
        });
    }

    for project in &config.projects {
        let radius = crate::project_radius(project);
        let mut baseline = radius + 25.0 * scale;

        if let Some(prefix) = &project.label_prefix {
            labels.push(label_box(
                &project.id,
                true,
                prefix,
                project.anchor,
                baseline,
                12.0 * scale,
            ));
            baseline += 26.0 * scale;
        }

        labels.push(label_box(
            &project.id,
            true,
            &project.surface_label,
            project.anchor,
            baseline,
            23.0 * scale,
        ));
        circles.push(Circle {
            id: &project.id,
            project: true,
            x: project.anchor[0],
            y: project.anchor[1],
            radius,
        });
    }

    labels.sort_by(|left, right| left.top.total_cmp(&right.top));
    circles.sort_by(|left, right| (left.y - left.radius).total_cmp(&(right.y - right.radius)));
    let maximum_diameter = circles
        .iter()
        .map(|circle| circle.radius * 2.0)
        .fold(0.0_f32, f32::max);

    for (index, label) in labels.iter().enumerate() {
        if label.left < 0.0
            || label.top < 0.0
            || label.right > config.render.width as f32
            || label.bottom > config.render.height as f32
        {
            return Err(ValidationError::InvalidValue(format!(
                "surface label cannot fit canvas: {}; shorten its surface label or change its layout",
                label.id
            )));
        }

        for other in labels[index + 1..]
            .iter()
            .take_while(|other| other.top < label.bottom)
        {
            if (label.id != other.id || label.project != other.project)
                && label.left < other.right
                && label.right > other.left
            {
                return Err(ValidationError::InvalidValue(format!(
                    "surface label collision: {} and {}; adjust their layout or compact labels",
                    label.id, other.id
                )));
            }
        }

        // Sorting by vertical envelopes bounds the candidate window. A full pairwise
        // scan here would make every preparation quadratic even for sparse tall layouts.
        let start = circles
            .partition_point(|circle| circle.y - circle.radius < label.top - maximum_diameter);

        for circle in circles[start..]
            .iter()
            .take_while(|circle| circle.y - circle.radius <= label.bottom)
        {
            if label.id == circle.id && label.project == circle.project {
                continue;
            }

            let x = circle.x.clamp(label.left, label.right);
            let y = circle.y.clamp(label.top, label.bottom);

            if (circle.x - x).hypot(circle.y - y) < circle.radius {
                return Err(ValidationError::InvalidValue(format!(
                    "surface label overlaps circle: {} and {}; adjust their layout",
                    label.id, circle.id
                )));
            }
        }
    }

    Ok(())
}

/// One em per scalar deliberately reserves more width than typical proportional glyphs.
/// This is a deterministic safety envelope, not a claim to replace browser font measurement.
fn label_box<'a>(
    id: &'a str,
    project: bool,
    text: &str,
    anchor: [f32; 2],
    baseline: f32,
    size: f32,
) -> LabelBox<'a> {
    let half_width = text.chars().count() as f32 * size / 2.0;
    let baseline = anchor[1] + baseline;

    LabelBox {
        id,
        project,
        left: anchor[0] - half_width,
        top: baseline - size,
        right: anchor[0] + half_width,
        bottom: baseline + size * 0.25,
    }
}
