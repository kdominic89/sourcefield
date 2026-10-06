//! Typed icon rendering contracts at the public SVG boundary.

use serde_json::json;
use sourcefield_core::{IconDefinition, NodeKind, ProfileState};
use sourcefield_render::{Theme, render_svg};

/// Construct two independently positioned projects without consumer-specific profile data.
fn fixture() -> ProfileState {
    let projects = (0..2)
        .map(|index| {
            json!({
                "id": format!("tool-{index}"), "label": format!("Tool {index}"),
                "surface_label": format!("Tool {index}"), "domain": "example",
                "visibility": "public", "status": "active", "visual": "provider",
                "anchor": [300.0 + index as f32 * 450.0, 700.0],
                "summary": "Synthetic public tool", "show_in_readme": true,
                "display_stack": ["Rust"]
            })
        })
        .collect::<Vec<_>>();

    let config = serde_json::from_value(json!({
        "schema_version": 1,
        "profile": {"variant": "personal", "username": "example-user", "display_name": "Example User",
            "organization": "example-org", "headline": "Developer tools", "tagline": "Example public profile",
            "pages_url": "https://example.com/", "source_url": "https://github.com/example-user/profile"},
        "collection": {"github_user": "example-user"},
        "render": {"width": 1800, "height": 1795, "show_interests_in_readme": true},
        "domains": [{"id": "example", "label": "Example organization", "owner": "example-org",
            "kind": "organization", "anchor": [1260.0, 519.0], "summary": "Public projects"}],
        "projects": projects
    })).unwrap();

    sourcefield_core::build_state(&config, &sourcefield_core::Snapshot::default(), "fixture")
        .unwrap()
}

/// Apply one shared definition to both projects without changing their palette selection.
fn select(state: &mut ProfileState, reference: &str, radius: f32) {
    for node in state
        .nodes
        .iter_mut()
        .filter(|node| node.kind == NodeKind::Project)
    {
        node.icon = Some(reference.to_owned());
        node.radius = radius;
    }
}

/// Exercise every geometric command and semantic paint through the same typed catalog.
fn custom_icon() -> IconDefinition {
    let shapes = [
        json!({"shape":"path", "commands":[
            {"command":"move", "to":[-12,-8]},
            {"command":"line", "to":[12,-8]},
            {"command":"horizontal", "x":14},
            {"command":"vertical", "y":8},
            {"command":"quadratic", "control":[10,12], "to":[8,10]},
            {"command":"cubic", "control1":[6,8], "control2":[4,6], "to":[2,4]},
            {"command":"arc", "radii":[4,6], "rotation":30,
             "large_arc":true, "sweep":false, "to":[-12,-8]},
            {"command":"close"}
        ]}),
        json!({"shape":"circle", "center":[1,2], "radius":3}),
        json!({"shape":"ellipse", "center":[-1,-2], "radii":[3,4]}),
        json!({"shape":"rect", "origin":[-6,-4], "size":[12,8], "corner_radius":2}),
    ];
    let elements = [
        "accent", "surface", "recess", "mint", "purple", "amber", "blue", "none",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, paint)| {
        let cap = ["butt", "round", "square"][index % 3];
        let join = ["miter", "round", "bevel"][index % 3];

        json!({
            "geometry":shapes[index % shapes.len()], "fill":paint, "stroke":"accent",
            "stroke_width":1.25, "opacity":0.75,
            "line_cap":cap,
            "line_join":join
        })
    })
    .collect::<Vec<_>>();

    serde_json::from_value(json!({"radius":32, "elements":elements})).unwrap()
}

#[test]
fn custom_geometry_streams_all_typed_commands_and_styles() {
    let mut state = fixture();
    state.icons.insert("custom".into(), custom_icon());
    select(&mut state, "custom", 48.0);

    let svg = render_svg(&state, Theme::Dark, false);

    assert!(svg.contains("d=\"M-12 -8L12 -8H14V8Q10 12 8 10C6 8 4 6 2 4A4 6 30 1 0 -12 -8Z\""));
    assert!(svg.contains("<circle cx=\"1\" cy=\"2\" r=\"3\""));
    assert!(svg.contains("<ellipse cx=\"-1\" cy=\"-2\" rx=\"3\" ry=\"4\""));
    assert!(svg.contains("<rect x=\"-6\" y=\"-4\" width=\"12\" height=\"8\" rx=\"2\""));
    assert!(svg.contains("stroke-width=\"1.25\" opacity=\"0.75\""));
    assert!(svg.contains("stroke-linecap=\"square\" stroke-linejoin=\"bevel\""));
    assert_eq!(svg.matches("data-icon=\"custom\"").count(), 2);
    assert!(!svg.contains("<use"));
}

#[test]
fn semantic_paints_resolve_to_explicit_dark_and_light_palettes() {
    let mut state = fixture();
    state.icons.insert("custom".into(), custom_icon());
    select(&mut state, "custom", 48.0);

    let dark = render_svg(&state, Theme::Dark, false);
    let light = render_svg(&state, Theme::Light, false);

    for color in [
        "#0d1425", "#090f1c", "#4de7c2", "#a898ff", "#ffbd7a", "#7acfff",
    ] {
        assert!(dark.contains(&format!("fill=\"{color}\"")));
    }

    for color in [
        "#ffffff", "#e2e8f3", "#006c59", "#6243b5", "#92500b", "#17628e",
    ] {
        assert!(light.contains(&format!("fill=\"{color}\"")));
    }
}

#[test]
fn fitting_never_upscales_and_clip_ids_encode_qualified_node_ids() {
    let mut state = fixture();
    state.icons.insert("custom".into(), custom_icon());
    select(&mut state, "custom", 32.0);
    let mut nodes = state
        .nodes
        .iter_mut()
        .filter(|node| node.kind == NodeKind::Project);
    let first = nodes.next().unwrap();
    first.id = "project:org/tool".into();
    let second = nodes.next().unwrap();
    second.id = "project:org-tool".into();
    second.radius = 80.0;

    let svg = render_svg(&state, Theme::Dark, true);

    assert!(svg.contains(
        "<clipPath id=\"icon-clip-70726f6a6563743a6f72672f746f6f6c\"><circle r=\"16\"/>"
    ));
    assert!(svg.contains("clip-path=\"url(#icon-clip-70726f6a6563743a6f72672f746f6f6c)\""));
    assert!(svg.contains(
        "<clipPath id=\"icon-clip-70726f6a6563743a6f72672d746f6f6c\"><circle r=\"64\"/>"
    ));
    assert!(svg.contains("data-icon=\"custom\" transform=\"scale(0.5)\""));
    assert!(svg.contains("data-icon=\"custom\" transform=\"scale(1)\""));
    assert_eq!(svg.matches("<clipPath id=\"icon-clip-").count(), 2);
}

#[test]
fn both_builtins_preserve_approved_scale_and_existing_project_rings() {
    let mut state = fixture();
    select(&mut state, "builtin:sourcefield", 47.0);
    let safe = state
        .nodes
        .iter_mut()
        .find(|node| node.id == "project:tool-1")
        .unwrap();
    safe.icon = Some("builtin:database-safe".into());
    safe.visual = Some("finance".into());
    safe.radius = 51.0;

    let svg = render_svg(&state, Theme::Dark, true);

    assert!(svg.contains("data-icon=\"builtin:sourcefield\" transform=\"scale(1)\""));
    assert!(svg.contains("data-icon=\"builtin:database-safe\" transform=\"scale(1)\""));
    assert_eq!(svg.matches("data-project-ring=\"outer\"").count(), 2);
    assert_eq!(svg.matches("data-project-ring=\"middle\"").count(), 2);
    assert!(svg.contains("<circle r=\"35\" fill=\"#0d1425\""));
    assert!(!svg.contains("data-glyph=\"vault\""));
    assert!(svg.contains("<rect x=\"-23\" y=\"-21\" width=\"31\" height=\"42\" rx=\"3\""));
    assert!(svg.contains("<ellipse cx=\"-7.5\" cy=\"-10\" rx=\"10.2\" ry=\"4.2\""));
    assert!(svg.contains("d=\"M8 -18L23 -22V22L8 18Z\""));
    assert!(svg.contains("d=\"M-10.4 -2.95Q-6.1 -2.1 -2 -3.25\""));
    assert!(svg.contains("<circle cx=\"-14.9\" cy=\"8.65\" r=\"0.53\""));
    assert!(svg.contains("<circle cx=\"-2\" cy=\"2\" r=\"4.1\""));
    assert!(svg.contains("<circle cx=\"13\" cy=\"-18\" r=\"3.2\""));
}

#[test]
fn explicit_icons_override_every_legacy_visual_glyph() {
    let visuals = [
        "finance",
        "trace",
        "database",
        "provider",
        "migrations",
        "packages",
        "hierarchy",
        "theme",
        "lab",
        "dotfiles",
        "custom",
    ];
    let mut state = fixture();
    select(&mut state, "builtin:sourcefield", 47.0);

    let rendered = visuals.map(|visual| {
        for node in state
            .nodes
            .iter_mut()
            .filter(|node| node.kind == NodeKind::Project)
        {
            node.visual = Some(visual.into());
        }

        render_svg(&state, Theme::Dark, false)
    });

    for svg in rendered {
        assert_eq!(svg.matches("data-icon=\"builtin:sourcefield\"").count(), 2);
        assert!(!svg.contains("data-glyph=\"vault\""));
        assert!(!svg.contains("class=\"rotate scan\""));
        assert!(!svg.contains("d=\"M0 -23L20 -11V12L0 24L-20 12V-11Z"));
        assert!(!svg.contains("d=\"M-17 -12H17V14H-17Z"));
        assert!(!svg.contains("d=\"M-15 -10L-25 0L-15 10"));
    }
}

#[test]
fn signals_have_fixed_native_timing_and_static_output_has_no_signal_classes() {
    let mut state = fixture();
    select(&mut state, "builtin:sourcefield", 47.0);

    let animated = render_svg(&state, Theme::Dark, true);
    let static_svg = render_svg(&state, Theme::Dark, false);

    assert!(animated.contains(".icon-signal{animation:icon-signal 5.4s ease-in-out infinite}"));
    assert!(animated.contains(".icon-signal-phase-1{animation-delay:-1.8s}"));
    assert!(animated.contains(".icon-signal-phase-2{animation-delay:-3.6s}"));
    assert!(animated.contains("@keyframes icon-signal{0%,100%{opacity:.35}45%{opacity:1}}"));
    assert!(
        animated.contains(
            "@media(prefers-reduced-motion:reduce){.icon-signal{animation:none!important}}"
        )
    );
    assert!(animated.contains("animation-play-state:paused!important"));
    assert_eq!(
        animated
            .matches("class=\"icon-signal icon-signal-phase-")
            .count(),
        6
    );
    assert!(!static_svg.contains("icon-signal"));
    assert!(!static_svg.contains("@keyframes"));
    assert!(!static_svg.contains("animation:"));
}

#[test]
fn unselected_catalog_entries_leave_legacy_svg_bytes_unchanged() {
    let state = fixture();
    let mut with_catalog = state.clone();
    with_catalog.icons.insert("unused".into(), custom_icon());
    with_catalog.icons.insert(
        "unused-signal".into(),
        sourcefield_core::builtin_icon("builtin:sourcefield")
            .unwrap()
            .clone(),
    );

    let comparisons = [
        (Theme::Dark, false),
        (Theme::Dark, true),
        (Theme::Light, false),
        (Theme::Light, true),
    ]
    .map(|(theme, motion)| {
        (
            render_svg(&state, theme, motion),
            render_svg(&with_catalog, theme, motion),
        )
    });

    for (legacy, added) in comparisons {
        assert_eq!(legacy, added);
        assert!(!added.contains("icon-clip-"));
        assert!(!added.contains("icon-signal"));
    }
}

#[test]
fn custom_signal_motion_uses_the_same_preset_and_hidden_icons_add_no_styles() {
    let mut state = fixture();
    let mut icon = custom_icon();
    icon.elements[0].motion = Some(sourcefield_core::IconMotion::Signal { phase: 2 });
    state.icons.insert("custom".into(), icon);
    select(&mut state, "custom", 48.0);
    let mut hidden = state.clone();

    for node in hidden
        .nodes
        .iter_mut()
        .filter(|node| node.kind == NodeKind::Project)
    {
        node.show_in_readme = false;
    }

    let visible_svg = render_svg(&state, Theme::Dark, true);
    let hidden_svg = render_svg(&hidden, Theme::Dark, true);

    assert_eq!(
        visible_svg
            .matches("class=\"icon-signal icon-signal-phase-2\"")
            .count(),
        2
    );
    assert!(visible_svg.contains("@keyframes icon-signal"));
    assert!(!hidden_svg.contains("icon-signal"));
    assert!(!hidden_svg.contains("icon-clip-"));
}
