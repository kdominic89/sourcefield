//! Radial cubic connections in the normalized design coordinate system.

use std::f64::consts::TAU;

/// A two-dimensional design coordinate.
#[derive(Clone, Copy, Debug)]
pub(super) struct Point(pub f64, pub f64);

impl Point {
    /// Euclidean distance in the unscaled design plane.
    fn distance(self, other: Self) -> f64 {
        (self.0 - other.0).hypot(self.1 - other.1)
    }
}

/// One cubic whose endpoint handles are radial to the connected circles.
#[derive(Debug)]
pub(super) struct Curve(pub [Point; 4]);

impl Curve {
    /// Clip a cubic to circle boundaries; overlapping circles have no visible connection.
    pub(super) fn between(
        source: (Point, f64),
        target: (Point, f64),
        controls: [Point; 2],
        departure: Option<f64>,
    ) -> Option<Self> {
        let (a, ra) = source;
        let (b, rb) = target;
        let distance = a.distance(b);

        if !distance.is_finite()
            || !ra.is_finite()
            || !rb.is_finite()
            || controls
                .iter()
                .any(|point| !point.0.is_finite() || !point.1.is_finite())
            || departure.is_some_and(|angle| !angle.is_finite())
            || ra <= 0.0
            || rb <= 0.0
            || distance <= ra + rb
        {
            return None;
        }

        let mut c1 = orient(controls[0], a, b);
        let c2 = orient(controls[1], b, a);

        if let Some(angle) = departure {
            let length = (ra + 24.0).max(a.distance(c1).min(distance * 0.45));
            c1 = Point(a.0 + length * angle.cos(), a.1 + length * angle.sin());
        }

        // A radial tangent requires a handle outside its disk, even for close neighbors.
        let c1 = extend(a, c1, ra);
        let c2 = extend(b, c2, rb);
        let curve = Self([boundary(a, c1, ra), c1, c2, boundary(b, c2, rb)]);

        // Cubics stay inside their control polygon. A separating tangent plane proves
        // the entire curve clears both disks without sampling the common simple case.
        let separated = curve.separated_from(a, curve.0[0]) && curve.separated_from(b, curve.0[3]);

        if separated
            || (0..=1000).all(|index| {
                let point = curve.point(index as f64 / 1000.0);
                (point.0 - a.0).powi(2) + (point.1 - a.1).powi(2) >= (ra - 1e-6).powi(2)
                    && (point.0 - b.0).powi(2) + (point.1 - b.1).powi(2) >= (rb - 1e-6).powi(2)
            })
        {
            return Some(curve);
        }

        // Arbitrary custom layouts can make a fitted port curve reenter a disk.
        // The line between disjoint circle centers is the exact safe radial solution.
        Some(Self([
            boundary(a, b, ra),
            boundary(a, b, ra),
            boundary(b, a, rb),
            boundary(b, a, rb),
        ]))
    }

    /// Prove all control points lie beyond an endpoint's outward tangent plane.
    fn separated_from(&self, center: Point, boundary: Point) -> bool {
        let normal = Point(boundary.0 - center.0, boundary.1 - center.1);

        self.0.iter().all(|point| {
            (point.0 - boundary.0) * normal.0 + (point.1 - boundary.1) * normal.1 >= 0.0
        })
    }

    /// Evaluate the cubic for boundary verification and deterministic regression tests.
    fn point(&self, t: f64) -> Point {
        let [a, b, c, d] = self.0;
        let u = 1.0 - t;

        Point(
            u.powi(3) * a.0 + 3.0 * u * u * t * b.0 + 3.0 * u * t * t * c.0 + t.powi(3) * d.0,
            u.powi(3) * a.1 + 3.0 * u * u * t * b.1 + 3.0 * u * t * t * c.1 + t.powi(3) * d.1,
        )
    }

    /// Scale the complete curve so non-square canvases retain ellipse-boundary alignment.
    pub(super) fn path(&self, sx: f64, sy: f64) -> String {
        let [a, b, c, d] = self.0.map(|point| Point(point.0 * sx, point.1 * sy));

        format!(
            "M{:.6} {:.6}C{:.6} {:.6} {:.6} {:.6} {:.6} {:.6}",
            a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1
        )
    }
}

/// Fit equally spaced ports to sorted neighbor angles without changing their circular order.
pub(super) fn ports(angles: &[f64]) -> Vec<f64> {
    if angles.is_empty() {
        return Vec::new();
    }

    let spacing = TAU / angles.len() as f64;
    let offsets = angles
        .iter()
        .enumerate()
        .map(|(index, angle)| angle - index as f64 * spacing);

    let (sine, cosine) = offsets.fold((0.0, 0.0), |(sine, cosine), angle| {
        (sine + angle.sin(), cosine + angle.cos())
    });

    let rotation = sine.atan2(cosine);

    (0..angles.len())
        .map(|index| rotation + index as f64 * spacing)
        .collect()
}

/// Turn backward handles toward the neighbor while preserving their bend side.
fn orient(control: Point, center: Point, other: Point) -> Point {
    let Point(dx, dy) = Point(other.0 - center.0, other.1 - center.1);
    let Point(vx, vy) = Point(control.0 - center.0, control.1 - center.1);

    if vx * dx + vy * dy > 0.0 {
        return control;
    }

    let side = if -dy * vx + dx * vy >= 0.0 { 1.0 } else { -1.0 };

    Point(
        center.0 + 0.35 * dx - side * 0.18 * dy,
        center.1 + 0.35 * dy + side * 0.18 * dx,
    )
}

/// Preserve the handle ray while guaranteeing a nonzero outward endpoint tangent.
fn extend(center: Point, control: Point, radius: f64) -> Point {
    if center.distance(control) > radius {
        control
    } else {
        boundary(center, control, radius + 1.0)
    }
}

/// Intersect a center-to-handle ray with its circle.
fn boundary(center: Point, control: Point, radius: f64) -> Point {
    let factor = radius / center.distance(control);

    Point(
        center.0 + (control.0 - center.0) * factor,
        center.1 + (control.1 - center.1) * factor,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_even_and_empty_input_is_safe() {
        let angles = [-2.2, -0.8, 0.0, 0.9, 2.0, 3.0];

        let fitted = ports(&angles);

        assert!(ports(&[]).is_empty());
        assert_eq!(fitted.len(), 6);

        for pair in fitted.windows(2) {
            assert!((pair[1] - pair[0] - TAU / 6.0).abs() < 1e-12);
        }
    }

    #[test]
    fn boundary_tangents_and_samples_stay_outside_circles() {
        let a = Point(451.0, 522.0);
        let b = Point(315.0, 298.0);

        let curve = Curve::between(
            (a, 61.0),
            (b, 55.0),
            [Point(376.2, 442.0), Point(275.0, 363.0)],
            Some(-115.6974_f64.to_radians()),
        )
        .unwrap();

        assert!((curve.0[0].distance(a) - 61.0).abs() < 1e-9);
        assert!((curve.0[3].distance(b) - 55.0).abs() < 1e-9);

        for index in 0..=1000 {
            let p = curve.point(index as f64 / 1000.0);
            assert!(p.distance(a) >= 61.0 - 1e-9 && p.distance(b) >= 55.0 - 1e-9);
        }
    }

    #[test]
    fn nonfinite_radius_is_rejected() {
        let controls = [Point(20.0, 10.0), Point(80.0, 10.0)];

        let curve = Curve::between(
            (Point(0.0, 0.0), f64::NAN),
            (Point(100.0, 0.0), 10.0),
            controls,
            None,
        );

        assert!(curve.is_none());
    }

    #[test]
    fn nonfinite_control_is_rejected() {
        let controls = [Point(f64::INFINITY, 10.0), Point(80.0, 10.0)];

        let curve = Curve::between(
            (Point(0.0, 0.0), 10.0),
            (Point(100.0, 0.0), 10.0),
            controls,
            None,
        );

        assert!(curve.is_none());
    }

    #[test]
    fn nonfinite_departure_is_rejected() {
        let controls = [Point(20.0, 10.0), Point(80.0, 10.0)];

        let curve = Curve::between(
            (Point(0.0, 0.0), 10.0),
            (Point(100.0, 0.0), 10.0),
            controls,
            Some(f64::NAN),
        );

        assert!(curve.is_none());
    }

    #[test]
    fn overlapping_and_coincident_circles_do_not_emit_invalid_paths() {
        let a = (Point(0.0, 0.0), 20.0);
        let controls = [Point(0.0, 0.0), Point(10.0, 10.0)];

        let coincident = Curve::between(a, a, controls, None);
        let overlapping = Curve::between(a, (Point(10.0, 0.0), 20.0), controls, None);

        assert!(coincident.is_none());
        assert!(overlapping.is_none());
    }
}
