//! Synthetic rendering contract tests shared across profile variants.

use super::*;
use sourcefield_core::ProfileVariant;

/// Synthetic identities keep reusable generator tests independent of consumer repositories.
fn fixture() -> (sourcefield_core::Config, ProfileState) {
    let projects = (0..3)
        .map(|index| {
            serde_json::json!({
                "id": format!("tool-{index}"), "label": format!("Tool {index}"),
                "surface_label": format!("Tool {index}"), "domain": "example",
                "visibility": "public", "status": "active", "visual": "provider",
                "anchor": [300.0 + index as f32 * 450.0, 700.0],
                "summary": "Synthetic public tool", "show_in_readme": true,
                "display_stack": ["Rust"]
            })
        })
        .collect::<Vec<_>>();

    let publications = (0..3).map(|index| serde_json::json!({
        "id": format!("family-{index}"), "label": format!("Example.Family{index}"),
        "surface_label": format!("Family {index}"), "domain": "example", "registry": "nuget",
        "anchor": [76.0 + index as f32 * 574.0, 1017.0], "summary": "Published packages",
        "show_in_readme": true, "packages": [{
            "id": format!("Example.Family{index}"), "family": "core",
            "url": format!("https://www.nuget.org/packages/Example.Family{index}"),
            "summary": "Public package"
        }]
    })).collect::<Vec<_>>();

    let config = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "profile": {"variant": "personal", "username": "example-user", "display_name": "Example User",
            "organization": "example-org", "headline": "Developer tools", "tagline": "Example public profile",
            "pages_url": "https://example.com/", "source_url": "https://github.com/example-user/profile"},
        "collection": {"github_user": "example-user"},
        "render": {"width": 1800, "height": 1795, "show_interests_in_readme": true},
        "domains": [{"id": "example", "label": "Example organization", "owner": "example-org",
            "kind": "organization", "anchor": [1260.0, 519.0], "summary": "Public projects"}],
        "projects": projects, "publications": publications
    })).unwrap();

    let state =
        sourcefield_core::build_state(&config, &sourcefield_core::Snapshot::default(), "fixture")
            .unwrap();

    (config, state)
}

#[test]
fn approved_inventory_and_copy_are_rendered() {
    let (_, state) = fixture();

    let svg = render_svg(&state, Theme::Dark, true);

    assert_eq!(svg.matches("data-node-kind=\"project\"").count(), 3);
    assert_eq!(svg.matches("data-node-kind=\"package\"").count(), 3);
    assert!(svg.contains("Synthetic public tool"));
    assert!(svg.contains("Example public profile"));
    assert!(!svg.contains("SOURCEFIELD / 03"));
}

#[test]
fn package_labels_versions_and_growth_keep_project_geometry_stable() {
    let (_, mut state) = fixture();
    let package = state
        .nodes
        .iter_mut()
        .find(|node| node.id == "package:Example.Family1")
        .unwrap();

    package.surface_label = "Sqlite".into();
    package.details.push("Observed version: 10.4.5".into());

    let mut expanded_state = state.clone();
    expanded_state.canvas.height += 90;

    let baseline = render_svg(&state, Theme::Dark, false);
    let expanded = render_svg(&expanded_state, Theme::Dark, false);

    let baseline_field = baseline
        .split("<g data-module=\"publications\">")
        .next()
        .unwrap();

    let expanded_field = expanded
        .split("<g data-module=\"publications\">")
        .next()
        .unwrap();

    let marker = "data-node-id=\"project:tool-0\"";
    let baseline_geometry = baseline_field.split(marker).nth(1);
    let expanded_geometry = expanded_field.split(marker).nth(1);
    let first = baseline.find(">Example.Family0</text>").unwrap();
    let second = baseline.find(">Example.Family1</text>").unwrap();
    let third = baseline.find(">Example.Family2</text>").unwrap();

    assert!(baseline.contains("<title>Example.Family1</title>"));
    assert!(baseline.contains(">Sqlite</text>"));
    assert!(baseline.contains(">10.4.5</text>"));
    assert!(baseline.contains("translate(0 115)"));
    assert!(expanded.contains("translate(0 205)"));
    assert_eq!(baseline_geometry, expanded_geometry);
    assert!(first < second && second < third);
}

#[test]
fn hidden_renamed_added_and_reordered_projects_are_data_driven() {
    let (mut config, _) = fixture();
    config.projects[0].show_in_readme = false;
    let hidden = config.projects[0].surface_label.clone();
    config.projects[1].surface_label = "A <renamed> project".into();
    let mut added = config.projects[1].clone();
    added.id = "additional-project".into();
    added.label = "Additional project".into();
    added.surface_label = "Additional project".into();
    added.anchor = [300.0, 400.0];
    config.projects.push(added);
    let state =
        sourcefield_core::build_state(&config, &sourcefield_core::Snapshot::default(), "fixture")
            .unwrap();

    config.projects.reverse();
    let reversed =
        sourcefield_core::build_state(&config, &sourcefield_core::Snapshot::default(), "fixture")
            .unwrap();

    let svg = render_svg(&state, Theme::Dark, false);
    let reversed_svg = render_svg(&reversed, Theme::Dark, false);

    assert!(!svg.contains(&format!(">{hidden}</text>")));
    assert!(svg.contains("A &lt;renamed&gt; project"));
    assert!(svg.contains("Additional project"));
    assert_eq!(svg.matches("data-node-kind=\"project\"").count(), 3);
    assert_eq!(svg, reversed_svg);
}

#[test]
fn static_output_has_no_animation_rules_and_light_labels_are_dark() {
    let (_, state) = fixture();

    let static_svg = render_svg(&state, Theme::Dark, false);
    let animated_svg = render_svg(&state, Theme::Dark, true);
    let light_svg = render_svg(&state, Theme::Light, false);

    assert!(!static_svg.contains("@keyframes"));
    assert!(!static_svg.contains("animation:"));
    assert!(animated_svg.contains("prefers-reduced-motion"));
    assert!(animated_svg.contains("animation-play-state:paused"));
    assert!(light_svg.contains("fill=\"#11182b\""));
    assert!(light_svg.contains("stop-color=\"#f5f7fc\""));
}

#[test]
fn optional_content_and_canvas_dimensions_are_consumed() {
    let (_, mut state) = fixture();
    state.canvas.width = 2400;
    state.canvas.height = 2000;
    state.canvas.show_state_hash = false;
    state.canvas.show_interests_in_readme = false;
    state.canvas.show_technology_labels = false;
    state.canvas.show_activity_orbit = false;

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(svg.contains("viewBox=\"0 0 2400 2000\""));
    assert!(!svg.contains("OTHER INTERESTS"));
    assert!(!svg.contains(">Rust</text>"));
    assert!(!svg.contains(&state.semantic_hash));
    assert!(!svg.contains("stroke-opacity=\".25\" class=\"flow\""));
}

#[test]
fn details_require_explicit_rendering_flag() {
    let (_, mut state) = fixture();
    let project = state
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Project)
        .unwrap();

    project.details = vec!["Explicit public detail".into()];
    state.canvas.show_details = false;

    let mut detailed_state = state.clone();
    detailed_state.canvas.show_details = true;

    let hidden = render_svg(&state, Theme::Dark, false);
    let shown = render_svg(&detailed_state, Theme::Dark, false);

    assert!(!hidden.contains("Explicit public detail"));
    assert!(shown.contains("<title>Explicit public detail</title>"));
}

#[test]
fn organization_variant_has_linked_maintainer_without_personal_sections() {
    let (_, mut state) = fixture();
    state.profile.variant = ProfileVariant::Organization;
    state.profile.maintainer = Some(sourcefield_core::MaintainerConfig {
        username: "maintainer".into(),
        role: "Core maintainer".into(),
        url: "https://github.com/maintainer".into(),
    });

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(svg.contains("href=\"https://github.com/maintainer\""));
    assert!(svg.contains("maintainer / Core maintainer"));
    assert!(!svg.contains("OTHER INTERESTS"));
    assert!(!svg.contains("AT HOME"));
    assert!(!svg.contains("data-module=\"learning\""));
}

#[test]
fn missing_optional_maintainer_does_not_emit_empty_link() {
    let (_, mut state) = fixture();
    state.profile.variant = ProfileVariant::Organization;
    state.profile.maintainer = None;

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(!svg.contains("href=\"https://github.com/\""));
    assert!(svg.contains("github.com/example-org"));
}

#[test]
fn multiple_organizations_keep_satellite_counts_scoped() {
    let (_, mut state) = fixture();
    let mut domain = state
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Domain)
        .unwrap()
        .clone();

    domain.id = "domain:second".into();
    domain.domain = Some("second".into());
    domain.label = "Second organization".into();
    domain.surface_label = "Second organization".into();
    domain.y += 800.0;
    state.nodes.push(domain);
    state.canvas.height += 800;

    let svg = render_svg(&state, Theme::Dark, true);

    assert!(svg.contains("Second organization"));
    assert_eq!(
        svg.matches("data-satellite=\"nuget-package\"><path")
            .count(),
        3
    );
    assert!(svg.contains("data-node-id=\"domain:second\""));
}

#[test]
fn fourth_publication_renders_in_next_row_without_reordering_columns() {
    let (_, mut state) = fixture();
    let mut group = state
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Publication)
        .unwrap()
        .clone();

    group.id = "publication:fourth".into();
    group.label = "Example.Fourth".into();
    group.x = 76.0;
    group.y += 500.0;
    state.nodes.push(group);
    state.canvas.height += 500;

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(
        svg.find(">Example.Family2</text>").unwrap() < svg.find(">Example.Fourth</text>").unwrap()
    );
    assert!(svg.contains("x=\"76\" y=\"1517\""));
    assert!(svg.contains("M64 937H1736"));
}

#[test]
fn organization_connections_have_curved_radial_boundary_tangents() {
    let (_, mut state) = fixture();
    state.profile.variant = ProfileVariant::Organization;
    let source = state
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Domain)
        .unwrap();

    let target = state
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Project)
        .unwrap();

    let path = connection_path(
        source,
        target,
        &state,
        None,
        false,
        OwnershipCurve::Sweeping,
    )
    .unwrap();
    let coordinates = path
        .trim_start_matches('M')
        .replace('C', " ")
        .split_whitespace()
        .map(|value| value.parse::<f64>().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(coordinates.len(), 8);
    for (node, endpoint, control) in [(source, 0, 2), (target, 6, 4)] {
        let dx = coordinates[endpoint] - node.x as f64;
        let dy = coordinates[endpoint + 1] - node.y as f64;
        let tx = coordinates[control] - coordinates[endpoint];
        let ty = coordinates[control + 1] - coordinates[endpoint + 1];

        assert!((dx.hypot(dy) - node.radius as f64).abs() < 0.0001);
        assert!((dx * ty - dy * tx).abs() < 0.001);
        assert!(dx * tx + dy * ty > 0.0);
    }

    assert!(coordinates[1] != coordinates[3]);
}

/// WCAG relative luminance for the fixed six-digit palette colors.
fn luminance(color: &str) -> f64 {
    let channels = [1, 3, 5].map(|start| {
        let value = u8::from_str_radix(&color[start..start + 2], 16).unwrap() as f64 / 255.0;

        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });

    channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
}

#[test]
fn text_palettes_meet_normal_text_contrast_on_both_gradient_stops() {
    for theme in [Theme::Dark, Theme::Light] {
        let palette = theme.palette();

        for background in [palette.background, palette.secondary] {
            for foreground in [
                palette.text,
                palette.muted,
                palette.quiet,
                palette.mint,
                palette.purple,
                palette.amber,
                palette.blue,
            ] {
                let a = luminance(background);
                let b = luminance(foreground);
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);

                assert!(ratio >= 4.5, "{foreground} on {background}: {ratio}");
            }
        }
    }
}

#[test]
fn xml_preserves_text_without_markup() {
    let value = "<&>\"'";

    let escaped = xml(value).to_string();

    assert_eq!(escaped, "&lt;&amp;&gt;&quot;&apos;");
}

#[test]
fn links_reject_executable_schemes() {
    let mut output = String::new();

    let accepted = open_link(&mut output, Some("javascript:alert(1)"));

    assert!(!accepted);
    assert!(output.is_empty());
}

#[test]
fn links_accept_https_and_escape_attributes() {
    let mut output = String::new();

    let accepted = open_link(&mut output, Some("https://example.com/?a=1&b=2"));

    assert!(accepted);
    assert!(output.contains("a=1&amp;b=2"));
}

#[test]
fn prepared_rendering_matches_one_shot_flavors() {
    let (_, state) = fixture();
    let prepared = PreparedPresentation::new(&state);

    let actual = [
        (Theme::Dark, true),
        (Theme::Light, true),
        (Theme::Dark, false),
    ]
    .map(|(theme, motion)| prepared.render(theme, motion));

    let expected = [
        (Theme::Dark, true),
        (Theme::Light, true),
        (Theme::Dark, false),
    ]
    .map(|(theme, motion)| render_svg(&state, theme, motion));

    assert_eq!(actual, expected);
}

#[test]
fn lead_interest_keeps_its_label_without_the_redundant_description() {
    let (_, mut state) = fixture();
    state.interests.push(sourcefield_core::InterestConfig {
        id: "first".into(),
        label: "First interest".into(),
        summary: "Previously omitted description".into(),
        show_in_readme: true,
    });

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(svg.contains(">First interest</text>"));
    assert!(!svg.contains(">Previously omitted description</text>"));
}

#[test]
fn expanded_footer_keeps_every_interest_and_bottom_links_inside_canvas() {
    let (mut config, _) = fixture();
    config.interests = (0..10)
        .map(|index| sourcefield_core::InterestConfig {
            id: format!("interest-{index}"),
            label: format!("Interest {index}"),
            summary: format!("Description {index}"),
            show_in_readme: true,
        })
        .collect();
    let state =
        sourcefield_core::build_state(&config, &sourcefield_core::Snapshot::default(), "fixture")
            .unwrap();
    let prepared = PreparedPresentation::new(&state);

    let Footer::Personal(layout) = prepared.policy.footer else {
        panic!("personal footer expected")
    };

    let svg = prepared.render(Theme::Dark, false);

    assert!(state.canvas.height > config.render.height);
    assert!(!svg.contains(">Description 0</text>"));
    assert!(svg.contains(">Description 9</text>"));
    assert!(layout.separator_y + prepared.policy.footer_offset + 31.0 < state.canvas.height as f32);
}

#[test]
fn unicode_public_text_preserves_content_and_escapes_markup() {
    let (_, mut state) = fixture();
    state.profile.tagline = "Caf\u{00e9} <tools> & \u{03bb}".into();

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(svg.contains("Caf\u{00e9} &lt;tools&gt; &amp; \u{03bb}"));
    assert!(!svg.contains("<tools>"));
}
