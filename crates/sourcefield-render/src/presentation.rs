//! Immutable variant policy prepared once for shared SVG rendering stages.

use sourcefield_core::{
    Node, PersonalFooterLayout, ProfileState, ProfileVariant, personal_footer_layout,
};

/// Algorithms differ only where the approved compositions have different ownership curves.
#[derive(Clone, Copy)]
pub(super) enum OwnershipCurve {
    /// Account departures are distributed around their domain ring.
    Distributed,
    /// Organization branches use opposing curved endpoint handles.
    Sweeping,
}

/// One footer strategy selected before SVG emission.
#[derive(Clone, Copy)]
pub(super) enum Footer {
    /// Three measured personal context columns.
    Personal(PersonalFooterLayout),
    /// Organization and optional maintainer links.
    Organization,
}

/// Small immutable policy; node scope still determines each domain's local semantics.
pub(super) struct Presentation {
    /// Uniform scale from design coordinates to SVG coordinates.
    pub scale: f32,
    /// Number of decorative background points for the selected composition.
    pub stars: usize,
    /// Footer algorithm and its measured capacity.
    pub footer: Footer,
    /// Translation retaining the footer start when its content grows.
    pub footer_offset: f32,
    /// Fallback section separator when no publication heading is present.
    pub publication_top: f32,
    /// Ownership curve algorithm selected for this composition.
    pub curve: OwnershipCurve,
    /// Domain glow radii in design coordinates.
    pub glow: [f32; 2],
    /// Domain activity orbit radii in design coordinates.
    pub orbit: [f32; 2],
    /// Vertical activity-orbit offset relative to its domain center.
    pub orbit_y_offset: f32,
    /// Approved activity-orbit opacity.
    pub orbit_opacity: &'static str,
    /// Approved base and animated ownership-edge opacities.
    pub edge_opacity: [&'static str; 2],
}

impl Presentation {
    /// Exhaustively resolve the supported variants without copying profile or node data.
    pub fn new(state: &ProfileState) -> Self {
        let scale = state.canvas.width as f32 / 1800.0;
        let height = state.canvas.height as f32 / scale;

        match state.profile.variant {
            ProfileVariant::Personal => {
                let footer = personal_footer_layout(
                    &state.presentation,
                    &state.interests,
                    &state.learning,
                    state.canvas.show_interests_in_readme,
                );

                Self {
                    scale,
                    stars: 85,
                    footer: Footer::Personal(footer),
                    footer_offset: height - 1680.0 - footer.extra_height,
                    publication_top: 937.0,
                    curve: OwnershipCurve::Distributed,
                    glow: [480.0, 400.0],
                    orbit: [373.0, 307.0],
                    orbit_y_offset: 0.0,
                    orbit_opacity: ".25",
                    edge_opacity: [".16", ".55"],
                }
            }
            ProfileVariant::Organization => Self {
                scale,
                stars: 0,
                footer: Footer::Organization,
                footer_offset: height - 1520.0,
                publication_top: 882.0,
                curve: OwnershipCurve::Sweeping,
                glow: [650.0, 315.0],
                orbit: [510.0, 250.0],
                orbit_y_offset: 30.0,
                orbit_opacity: ".16",
                edge_opacity: [".28", ".62"],
            },
        }
    }

    /// Position optional domain headings while retaining domain-local account/organization meaning.
    pub fn heading(&self, domain: &Node, count: usize, width: u32) -> Option<(f32, f32)> {
        let organization = domain.scope.as_deref() == Some("organization");
        let personal = matches!(self.footer, Footer::Personal(_));
        if !personal && count == 1 {
            return None;
        }

        let x = if organization && personal {
            width as f32 - 266.0 * self.scale
        } else {
            68.0 * self.scale
        };
        let origin = if organization { 519.0 } else { 522.0 };

        Some((x, domain.y + (251.0 - origin) * self.scale))
    }
}
