use super::*;

/// Keep validation fixtures small so failures identify the changed admission boundary.
fn circle() -> IconDefinition {
    IconDefinition {
        radius: 16.0,
        elements: vec![IconElement::new(IconGeometry::Circle {
            center: [0.0, 0.0],
            radius: 4.0,
        })],
    }
}

/// Construct a single-definition catalog without duplicating production admission logic.
fn catalog(definition: IconDefinition) -> IconCatalog {
    BTreeMap::from([("beacon".into(), definition)])
}

/// Build a valid open contour with an exact resource count.
fn line_path(count: usize) -> IconElement {
    let mut commands = vec![IconPathCommand::Move { to: [0.0, 0.0] }];
    commands.extend((1..count).map(|_| IconPathCommand::Line { to: [1.0, 1.0] }));

    IconElement::new(IconGeometry::Path { commands })
}

#[test]
fn icon_defaults_match_constructor_and_round_trip() {
    let input =
        r#"{"radius":16,"elements":[{"geometry":{"shape":"circle","center":[0,0],"radius":4}}]}"#;
    let expected = circle();

    let decoded: IconDefinition = serde_json::from_str(input).unwrap();
    let encoded = serde_json::to_string(&decoded).unwrap();
    let repeated: IconDefinition = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, expected);
    assert_eq!(repeated, expected);
    assert!(!encoded.contains("motion"));
}

#[test]
fn icon_input_rejects_unknown_fields_at_every_object_boundary() {
    let input = serde_json::to_value(circle()).unwrap();
    let mut cases = Vec::new();

    for pointer in ["", "/elements/0", "/elements/0/geometry"] {
        let mut value = input.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("onload".into(), serde_json::json!("alert(1)"));
        cases.push(value);
    }

    for geometry in [
        serde_json::json!({"shape":"path","commands":[],"href":"injected"}),
        serde_json::json!({"shape":"ellipse","center":[0,0],"radii":[1,1],"href":"injected"}),
        serde_json::json!({"shape":"rect","origin":[0,0],"size":[1,1],"href":"injected"}),
    ] {
        let mut value = input.clone();
        value["elements"][0]["geometry"] = geometry;
        cases.push(value);
    }

    for mut command in [
        serde_json::json!({"command":"move","to":[0,0]}),
        serde_json::json!({"command":"line","to":[0,0]}),
        serde_json::json!({"command":"horizontal","x":0}),
        serde_json::json!({"command":"vertical","y":0}),
        serde_json::json!({"command":"quadratic","control":[1,1],"to":[0,0]}),
        serde_json::json!({"command":"cubic","control1":[1,1],"control2":[1,1],"to":[0,0]}),
        serde_json::json!({"command":"arc","radii":[1,1],"rotation":0,"large_arc":false,"sweep":false,"to":[0,0]}),
        serde_json::json!({"command":"close"}),
    ] {
        command["script"] = serde_json::json!("alert(1)");
        let mut value = input.clone();
        value["elements"][0]["geometry"] = serde_json::json!({"shape":"path","commands":[command]});
        cases.push(value);
    }

    let mut motion = input;
    motion["elements"][0]["motion"] = serde_json::json!({"kind":"signal","phase":0,"duration":1});
    cases.push(motion);

    let results = cases
        .into_iter()
        .map(serde_json::from_value::<IconDefinition>)
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err), "{results:?}");
}

#[test]
fn icon_input_rejects_raw_markup_paths_paints_and_motion() {
    let cases = [
        serde_json::json!({"geometry":{"shape":"path","d":"M0 0<script/>"}}),
        serde_json::json!({"geometry":{"shape":"image","href":"https://invalid.test/a.svg"}}),
        serde_json::json!({"geometry":{"shape":"circle","center":[0,0],"radius":4},"fill":"url(#evil)"}),
        serde_json::json!({"geometry":{"shape":"circle","center":[0,0],"radius":4},"line_cap":"inherit"}),
        serde_json::json!({"geometry":{"shape":"circle","center":[0,0],"radius":4},"line_join":"arcs"}),
        serde_json::json!({"geometry":{"shape":"circle","center":[0,0],"radius":4},"motion":{"kind":"script","phase":0}}),
    ];

    let results = cases
        .into_iter()
        .map(serde_json::from_value::<IconElement>)
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err));
}

#[test]
fn icon_all_geometry_commands_styles_and_motion_are_supported() {
    let commands = vec![
        IconPathCommand::Move { to: [-16.0, 0.0] },
        IconPathCommand::Line { to: [0.0, -16.0] },
        IconPathCommand::Horizontal { x: 16.0 },
        IconPathCommand::Vertical { y: 16.0 },
        IconPathCommand::Quadratic {
            control: [16.0, -16.0],
            to: [0.0, 0.0],
        },
        IconPathCommand::Cubic {
            control1: [-16.0, 16.0],
            control2: [16.0, -16.0],
            to: [4.0, 0.0],
        },
        IconPathCommand::Arc {
            radii: [32.0, 0.1],
            rotation: -360.0,
            large_arc: true,
            sweep: true,
            to: [0.0, 0.0],
        },
        IconPathCommand::Close {},
        IconPathCommand::Move { to: [0.0, 0.0] },
        IconPathCommand::Line { to: [16.0, 16.0] },
    ];

    let mut definition = circle();
    definition.elements.extend([
        IconElement::new(IconGeometry::Ellipse {
            center: [0.0, 0.0],
            radii: [16.0, 1.0],
        }),
        IconElement::new(IconGeometry::Rect {
            origin: [-16.0, -16.0],
            size: [32.0, 32.0],
            corner_radius: 16.0,
        }),
        IconElement::new(IconGeometry::Path { commands }),
    ]);

    for (index, paint) in [
        IconPaint::None,
        IconPaint::Accent,
        IconPaint::Surface,
        IconPaint::Recess,
        IconPaint::Mint,
        IconPaint::Purple,
        IconPaint::Amber,
        IconPaint::Blue,
    ]
    .into_iter()
    .enumerate()
    {
        let mut element = definition.elements[0].clone();
        element.fill = paint;
        element.stroke = paint;
        element.line_cap = [IconLineCap::Butt, IconLineCap::Round, IconLineCap::Square][index % 3];
        element.line_join = [
            IconLineJoin::Miter,
            IconLineJoin::Round,
            IconLineJoin::Bevel,
        ][index % 3];
        element.motion = Some(IconMotion::Signal {
            phase: (index % 3) as u8,
        });
        definition.elements.push(element);
    }

    let serialized = serde_json::to_string(&definition).unwrap();
    let round_trip: IconDefinition = serde_json::from_str(&serialized).unwrap();
    let validated = validate_icon_catalog(&catalog(round_trip.clone()));

    assert_eq!(round_trip, definition);
    assert!(validated.is_ok());
}

#[test]
fn icon_nonfinite_numbers_fail_before_generic_serialization() {
    let mut cases = Vec::new();

    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut radius = circle();
        radius.radius = value;
        cases.push(radius);

        let mut width = circle();
        width.elements[0].stroke_width = value;
        cases.push(width);

        let mut opacity = circle();
        opacity.elements[0].opacity = value;
        cases.push(opacity);

        for geometry in [
            IconGeometry::Circle {
                center: [value, 0.0],
                radius: 1.0,
            },
            IconGeometry::Circle {
                center: [0.0, 0.0],
                radius: value,
            },
            IconGeometry::Ellipse {
                center: [0.0, value],
                radii: [1.0, 1.0],
            },
            IconGeometry::Ellipse {
                center: [0.0, 0.0],
                radii: [value, 1.0],
            },
            IconGeometry::Rect {
                origin: [value, 0.0],
                size: [1.0, 1.0],
                corner_radius: 0.0,
            },
            IconGeometry::Rect {
                origin: [0.0, 0.0],
                size: [1.0, value],
                corner_radius: 0.0,
            },
            IconGeometry::Rect {
                origin: [0.0, 0.0],
                size: [1.0, 1.0],
                corner_radius: value,
            },
        ] {
            let mut definition = circle();
            definition.elements[0].geometry = geometry;
            cases.push(definition);
        }

        for command in [
            IconPathCommand::Move { to: [value, 0.0] },
            IconPathCommand::Line { to: [0.0, value] },
            IconPathCommand::Horizontal { x: value },
            IconPathCommand::Vertical { y: value },
            IconPathCommand::Quadratic {
                control: [value, 0.0],
                to: [1.0, 1.0],
            },
            IconPathCommand::Cubic {
                control1: [0.0, 0.0],
                control2: [value, 0.0],
                to: [1.0, 1.0],
            },
            IconPathCommand::Arc {
                radii: [value, 1.0],
                rotation: 0.0,
                large_arc: false,
                sweep: false,
                to: [1.0, 1.0],
            },
            IconPathCommand::Arc {
                radii: [1.0, 1.0],
                rotation: value,
                large_arc: false,
                sweep: false,
                to: [1.0, 1.0],
            },
        ] {
            let mut definition = circle();
            let commands = if matches!(command, IconPathCommand::Move { .. }) {
                vec![command, IconPathCommand::Line { to: [1.0, 1.0] }]
            } else {
                vec![IconPathCommand::Move { to: [0.0, 0.0] }, command]
            };

            definition.elements[0].geometry = IconGeometry::Path { commands };
            cases.push(definition);
        }
    }

    let results = cases
        .into_iter()
        .map(|definition| validate_icon_catalog(&catalog(definition)))
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err));
}

#[test]
fn icon_primitive_bounds_reject_negative_zero_and_excessive_dimensions() {
    let geometries = [
        IconGeometry::Circle {
            center: [13.0, 0.0],
            radius: 4.0,
        },
        IconGeometry::Circle {
            center: [0.0, 0.0],
            radius: 0.0,
        },
        IconGeometry::Circle {
            center: [0.0, 0.0],
            radius: -1.0,
        },
        IconGeometry::Ellipse {
            center: [0.0, 0.0],
            radii: [1.0, 17.0],
        },
        IconGeometry::Ellipse {
            center: [0.0, 15.5],
            radii: [1.0, 1.0],
        },
        IconGeometry::Ellipse {
            center: [0.0, 0.0],
            radii: [0.0, 1.0],
        },
        IconGeometry::Rect {
            origin: [-17.0, 0.0],
            size: [1.0, 1.0],
            corner_radius: 0.0,
        },
        IconGeometry::Rect {
            origin: [0.0, 0.0],
            size: [17.0, 1.0],
            corner_radius: 0.0,
        },
        IconGeometry::Rect {
            origin: [0.0, 0.0],
            size: [0.0, 1.0],
            corner_radius: 0.0,
        },
        IconGeometry::Rect {
            origin: [0.0, 0.0],
            size: [1.0, -1.0],
            corner_radius: 0.0,
        },
        IconGeometry::Rect {
            origin: [0.0, 0.0],
            size: [1.0, 1.0],
            corner_radius: -0.1,
        },
        IconGeometry::Rect {
            origin: [0.0, 0.0],
            size: [1.0, 1.0],
            corner_radius: 0.6,
        },
    ];

    let results = geometries
        .into_iter()
        .map(|geometry| {
            let mut definition = circle();
            definition.elements[0].geometry = geometry;

            validate_icon_catalog(&catalog(definition))
        })
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err));
}

#[test]
fn icon_path_bounds_cover_every_coordinate_and_arc_dimension() {
    let commands = [
        IconPathCommand::Line { to: [16.1, 0.0] },
        IconPathCommand::Horizontal { x: -16.1 },
        IconPathCommand::Vertical { y: 16.1 },
        IconPathCommand::Quadratic {
            control: [0.0, 16.1],
            to: [0.0, 0.0],
        },
        IconPathCommand::Cubic {
            control1: [16.1, 0.0],
            control2: [0.0, 0.0],
            to: [0.0, 0.0],
        },
        IconPathCommand::Cubic {
            control1: [0.0, 0.0],
            control2: [0.0, -16.1],
            to: [0.0, 0.0],
        },
        IconPathCommand::Arc {
            radii: [0.0, 1.0],
            rotation: 0.0,
            large_arc: false,
            sweep: false,
            to: [0.0, 0.0],
        },
        IconPathCommand::Arc {
            radii: [1.0, 32.1],
            rotation: 0.0,
            large_arc: false,
            sweep: false,
            to: [0.0, 0.0],
        },
        IconPathCommand::Arc {
            radii: [1.0, 1.0],
            rotation: 360.1,
            large_arc: false,
            sweep: false,
            to: [0.0, 0.0],
        },
        IconPathCommand::Arc {
            radii: [1.0, 1.0],
            rotation: -360.1,
            large_arc: false,
            sweep: false,
            to: [0.0, 0.0],
        },
        IconPathCommand::Arc {
            radii: [1.0, 1.0],
            rotation: 0.0,
            large_arc: false,
            sweep: false,
            to: [0.0, 16.1],
        },
    ];

    let results = commands
        .into_iter()
        .map(|command| {
            let definition = IconDefinition {
                radius: 16.0,
                elements: vec![IconElement::new(IconGeometry::Path {
                    commands: vec![IconPathCommand::Move { to: [0.0, 0.0] }, command],
                })],
            };

            validate_icon_catalog(&catalog(definition))
        })
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err));
}

#[test]
fn icon_path_structure_requires_nonempty_contours() {
    let start = IconPathCommand::Move { to: [0.0, 0.0] };
    let line = IconPathCommand::Line { to: [1.0, 1.0] };
    let close = IconPathCommand::Close {};
    let commands = vec![
        vec![],
        vec![line.clone()],
        vec![start.clone()],
        vec![close.clone()],
        vec![start.clone(), close.clone()],
        vec![start.clone(), start.clone(), line.clone()],
        vec![start.clone(), line.clone(), close.clone(), close.clone()],
        vec![start.clone(), line.clone(), close, line.clone()],
        vec![start.clone(), line, start],
    ];

    let results = commands
        .into_iter()
        .map(|commands| {
            let definition = IconDefinition {
                radius: 16.0,
                elements: vec![IconElement::new(IconGeometry::Path { commands })],
            };

            validate_icon_catalog(&catalog(definition))
        })
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err));
}

#[test]
fn icon_style_and_radius_limits_reject_out_of_range_values() {
    let mut cases = Vec::new();

    for value in [-1.0, 0.0, 64.1] {
        let mut definition = circle();
        definition.radius = value;
        cases.push(definition);
    }

    for value in [-1.0, 0.0, 8.1] {
        let mut definition = circle();
        definition.elements[0].stroke_width = value;
        cases.push(definition);
    }

    for value in [-0.1, 1.1] {
        let mut definition = circle();
        definition.elements[0].opacity = value;
        cases.push(definition);
    }

    for phase in [3, 255] {
        let mut definition = circle();
        definition.elements[0].motion = Some(IconMotion::Signal { phase });
        cases.push(definition);
    }

    let results = cases
        .into_iter()
        .map(|definition| validate_icon_catalog(&catalog(definition)))
        .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_err));
}

#[test]
fn icon_style_and_radius_accept_exact_boundaries() {
    let mut definition = circle();
    definition.radius = MAX_ICON_RADIUS;
    definition.elements[0].opacity = 0.0;
    definition.elements[0].stroke_width = 8.0;
    let mut opaque = definition.elements[0].clone();
    opaque.opacity = 1.0;
    opaque.stroke_width = f32::MIN_POSITIVE;
    definition.elements.push(opaque);

    let result = validate_icon_catalog(&catalog(definition));

    assert!(result.is_ok());
}

#[test]
fn icon_definition_and_path_counts_enforce_exact_budgets() {
    let mut many_elements = circle();
    many_elements.elements = vec![many_elements.elements[0].clone(); MAX_ICON_ELEMENTS];
    let mut excess_elements = many_elements.clone();
    excess_elements
        .elements
        .push(excess_elements.elements[0].clone());
    let path = IconDefinition {
        radius: 16.0,
        elements: vec![line_path(MAX_ICON_PATH_COMMANDS)],
    };

    let excessive_path = IconDefinition {
        radius: 16.0,
        elements: vec![line_path(MAX_ICON_PATH_COMMANDS + 1)],
    };

    let empty = IconDefinition {
        radius: 16.0,
        elements: vec![],
    };

    let at_limit = (0..MAX_ICON_DEFINITIONS)
        .map(|index| (format!("icon-{index}"), circle()))
        .collect::<IconCatalog>();
    let mut over_limit = at_limit.clone();
    over_limit.insert("excess".into(), circle());

    let results = [
        catalog(many_elements),
        catalog(path),
        at_limit,
        catalog(excess_elements),
        catalog(excessive_path),
        catalog(empty),
        over_limit,
    ]
    .iter()
    .map(validate_icon_catalog)
    .collect::<Vec<_>>();

    assert!(results[..3].iter().all(Result::is_ok));
    assert!(results[3..].iter().all(Result::is_err));
}

#[test]
fn icon_composed_catalog_aggregate_element_budget_is_enforced() {
    let mut definition = circle();
    definition.elements = vec![definition.elements[0].clone(); MAX_ICON_ELEMENTS];
    let at_limit = (0..MAX_CATALOG_ICON_ELEMENTS / MAX_ICON_ELEMENTS)
        .map(|index| (format!("org-{index}/beacon"), definition.clone()))
        .collect::<IconCatalog>();
    let mut over_limit = at_limit.clone();
    over_limit.insert("personal".into(), circle());

    let valid = validate_icon_catalog(&at_limit);
    let excessive = validate_icon_catalog(&over_limit);

    assert!(valid.is_ok());
    assert!(excessive.is_err());
}

#[test]
fn icon_composed_catalog_aggregate_command_budget_is_enforced() {
    let definition = IconDefinition {
        radius: 16.0,
        elements: vec![
            line_path(MAX_ICON_PATH_COMMANDS);
            MAX_CATALOG_ICON_PATH_COMMANDS / MAX_ICON_PATH_COMMANDS
        ],
    };

    let at_limit = catalog(definition);
    let mut over_limit = at_limit.clone();
    over_limit.insert(
        "other/icon".into(),
        IconDefinition {
            radius: 16.0,
            elements: vec![line_path(2)],
        },
    );

    let valid = validate_icon_catalog(&at_limit);
    let excessive = validate_icon_catalog(&over_limit);

    assert!(valid.is_ok());
    assert!(excessive.is_err());
}

#[test]
fn icon_identifiers_and_reference_namespaces_fail_closed() {
    let invalid_ids = [
        "",
        "A",
        "0icon",
        "-icon",
        "space icon",
        "a:b",
        "../icon",
        "a/b/c",
        "/icon",
        "org/",
        "org/Bad",
        "a<script>",
        "builtin:sourcefield",
    ];
    let mut candidates = invalid_ids
        .iter()
        .map(|id| ((*id).to_owned(), circle()))
        .collect::<Vec<_>>();
    candidates.push(("a".repeat(65), circle()));
    let definitions = BTreeMap::from([
        ("beacon".into(), circle()),
        ("org/beacon".into(), circle()),
        ("a".repeat(64), circle()),
    ]);

    let invalid = candidates
        .into_iter()
        .map(|(id, definition)| validate_icon_catalog(&BTreeMap::from([(id, definition)])))
        .collect::<Vec<_>>();
    let valid = validate_icon_catalog(&definitions);
    let references = [
        "beacon",
        "org/beacon",
        "builtin:sourcefield",
        "builtin:database-safe",
        "missing",
        "builtin:missing",
        "builtin:sourcefield/extra",
        "builtin:sourcefield\"",
        "org/beacon/extra",
    ]
    .into_iter()
    .map(|reference| validate_icon_reference(&definitions, reference))
    .collect::<Vec<_>>();

    let qualified_local = validate_local_icon_id("org/beacon");

    assert!(invalid.iter().all(Result::is_err));
    assert!(valid.is_ok());
    assert!(references[..4].iter().all(Result::is_ok));
    assert!(references[4..].iter().all(Result::is_err));
    assert!(qualified_local.is_err());
}

#[test]
fn icon_resolution_borrows_definitions_and_protects_builtins() {
    let mut definitions = catalog(circle());
    definitions.insert("builtin:sourcefield".into(), circle());
    let builtin = builtin_icon("builtin:sourcefield").unwrap();

    let custom = resolve_icon(&definitions, "beacon").unwrap();
    let resolved_builtin = resolve_icon(&definitions, "builtin:sourcefield").unwrap();
    let duplicate = resolve_icon(&definitions, "builtin:sourcefield").unwrap();
    let invalid_override = validate_icon_catalog(&definitions);

    assert!(std::ptr::eq(custom, &definitions["beacon"]));
    assert!(std::ptr::eq(resolved_builtin, builtin));
    assert!(std::ptr::eq(duplicate, builtin));
    assert!(invalid_override.is_err());
}

#[test]
fn icon_builtins_are_shared_across_concurrent_consumers() {
    let names = ["builtin:sourcefield", "builtin:database-safe"];
    let expected = names.map(|name| builtin_icon(name).unwrap() as *const IconDefinition as usize);
    let threads = (0..16)
        .map(|_| {
            std::thread::spawn(move || {
                names.map(|name| builtin_icon(name).unwrap() as *const IconDefinition as usize)
            })
        })
        .collect::<Vec<_>>();

    let addresses = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();

    assert!(addresses.iter().all(|actual| *actual == expected));
}

#[test]
fn icon_builtins_pass_consumer_geometry_admission() {
    let definitions = BTreeMap::from([
        (
            "network".into(),
            builtin_icon("builtin:sourcefield").unwrap().clone(),
        ),
        (
            "safe".into(),
            builtin_icon("builtin:database-safe").unwrap().clone(),
        ),
    ]);

    let result = validate_icon_catalog(&definitions);

    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn icon_sourcefield_artwork_preserves_all_trimmed_edges_and_signal_phases() {
    let expected_edges = [
        ([-5.086, -0.7], [-15.742, -10.024]),
        ([0.46, -1.28], [11.08, -15.44]),
        ([1.956, 3.079], [17.106, 7.211]),
        ([-3.991, 5.584], [-10.64, 17.552]),
        ([-15.055, -12.57], [9.858, -17.392]),
        ([13.832, -14.91], [19.22, 5.103]),
    ]
    .map(|(start, end)| {
        let mut element = IconElement::new(IconGeometry::Path {
            commands: vec![
                IconPathCommand::Move { to: start },
                IconPathCommand::Line { to: end },
            ],
        });

        element.stroke = IconPaint::Mint;
        element.stroke_width = 1.25;
        element.opacity = 0.65;

        element
    });

    let definition = builtin_icon("builtin:sourcefield").unwrap();
    let phases = definition
        .elements
        .iter()
        .filter_map(|element| element.motion)
        .collect::<Vec<_>>();
    let nodes = definition
        .elements
        .iter()
        .filter(|element| element.fill == IconPaint::Surface)
        .map(|element| (&element.geometry, element.stroke, element.stroke_width))
        .collect::<Vec<_>>();

    assert_eq!(definition.radius, 31.0);
    assert_eq!(definition.elements.len(), 14);
    assert_eq!(definition.elements[..6], expected_edges);
    assert_eq!(
        phases,
        vec![
            IconMotion::Signal { phase: 0 },
            IconMotion::Signal { phase: 1 },
            IconMotion::Signal { phase: 2 }
        ]
    );
    assert_eq!(
        nodes,
        vec![
            (
                &IconGeometry::Circle {
                    center: [-2.0, 2.0],
                    radius: 4.1
                },
                IconPaint::Mint,
                1.45
            ),
            (
                &IconGeometry::Circle {
                    center: [-18.0, -12.0],
                    radius: 3.0
                },
                IconPaint::Blue,
                1.45
            ),
            (
                &IconGeometry::Circle {
                    center: [13.0, -18.0],
                    radius: 3.2
                },
                IconPaint::Purple,
                1.45
            ),
            (
                &IconGeometry::Circle {
                    center: [20.0, 8.0],
                    radius: 3.0
                },
                IconPaint::Blue,
                1.45
            ),
            (
                &IconGeometry::Circle {
                    center: [-12.0, 20.0],
                    radius: 2.8
                },
                IconPaint::Mint,
                1.45
            ),
        ]
    );
}

#[test]
fn icon_database_safe_artwork_preserves_reference_cylinder_and_open_door() {
    let expected_body = IconGeometry::Rect {
        origin: [-23.0, -21.0],
        size: [31.0, 42.0],
        corner_radius: 3.0,
    };

    let expected_cylinder = IconGeometry::Path {
        commands: vec![
            IconPathCommand::Move { to: [-17.7, -10.0] },
            IconPathCommand::Vertical { y: 8.9 },
            IconPathCommand::Arc {
                radii: [10.2, 4.2],
                rotation: 0.0,
                large_arc: false,
                sweep: false,
                to: [2.7, 8.9],
            },
            IconPathCommand::Vertical { y: -10.0 },
        ],
    };

    let expected_lid = IconGeometry::Ellipse {
        center: [-7.5, -10.0],
        radii: [10.2, 4.2],
    };

    let expected_door = IconGeometry::Path {
        commands: vec![
            IconPathCommand::Move { to: [8.0, -18.0] },
            IconPathCommand::Line { to: [23.0, -22.0] },
            IconPathCommand::Vertical { y: 22.0 },
            IconPathCommand::Line { to: [8.0, 18.0] },
            IconPathCommand::Close {},
        ],
    };

    let definition = builtin_icon("builtin:database-safe").unwrap();
    let dots = definition
        .elements
        .iter()
        .filter(|element| element.stroke == IconPaint::None)
        .map(|element| &element.geometry)
        .collect::<Vec<_>>();
    let slots = definition
        .elements
        .iter()
        .filter(|element| element.stroke_width == 1.0)
        .map(|element| &element.geometry)
        .collect::<Vec<_>>();

    assert_eq!(definition.radius, 35.0);
    assert_eq!(definition.elements.len(), 13);
    assert_eq!(definition.elements[0].geometry, expected_body);
    assert_eq!(definition.elements[0].fill, IconPaint::Recess);
    assert_eq!(definition.elements[1].geometry, expected_cylinder);
    assert_eq!(definition.elements[1].stroke_width, 1.1);
    assert_eq!(definition.elements[2].geometry, expected_lid);
    assert_eq!(definition.elements[11].geometry, expected_door);
    assert_eq!(definition.elements[11].fill, IconPaint::Surface);
    assert_eq!(
        dots,
        vec![
            &IconGeometry::Circle {
                center: [-14.9, -3.95],
                radius: 0.53
            },
            &IconGeometry::Circle {
                center: [-14.9, 2.35],
                radius: 0.53
            },
            &IconGeometry::Circle {
                center: [-14.9, 8.65],
                radius: 0.53
            },
        ]
    );
    assert_eq!(slots.len(), 3);
    assert_eq!(
        *slots[0],
        IconGeometry::Path {
            commands: vec![
                IconPathCommand::Move { to: [-10.4, -2.95] },
                IconPathCommand::Quadratic {
                    control: [-6.1, -2.1],
                    to: [-2.0, -3.25]
                },
            ]
        }
    );
}

#[test]
fn icon_toml_defaults_preserve_rect_and_style_contract() {
    let input = r#"
radius = 16
[[elements]]
[elements.geometry]
shape = "rect"
origin = [-8, -8]
size = [16, 16]
"#;

    let expected = IconDefinition {
        radius: 16.0,
        elements: vec![IconElement::new(IconGeometry::Rect {
            origin: [-8.0, -8.0],
            size: [16.0, 16.0],
            corner_radius: 0.0,
        })],
    };

    let decoded: IconDefinition = toml::from_str(input).unwrap();
    let result = validate_icon_catalog(&catalog(decoded.clone()));

    assert_eq!(decoded, expected);
    assert!(result.is_ok());
}

#[test]
fn icon_geometry_errors_identify_validated_catalog_id_and_element_index() {
    let mut definition = circle();
    definition.elements[0].opacity = f32::NAN;
    let definitions = BTreeMap::from([("org/beacon".into(), definition)]);

    let error = validate_icon_catalog(&definitions).unwrap_err().to_string();

    assert!(error.contains("org/beacon"));
    assert!(error.contains("element 0"));
    assert!(error.contains("opacity"));
}

#[test]
fn icon_usage_accepts_hundreds_of_small_shared_builtin_references() {
    let definitions = IconCatalog::new();

    let network = validate_icon_usage(
        &definitions,
        std::iter::repeat_n("builtin:sourcefield", 511),
    );

    let safe = validate_icon_usage(
        &definitions,
        std::iter::repeat_n("builtin:database-safe", 511),
    );

    assert!(network.is_ok());
    assert!(safe.is_ok());
}

#[test]
fn icon_usage_counts_every_selected_element_and_stops_at_first_excess() {
    let mut definition = circle();
    definition.elements = vec![definition.elements[0].clone(); MAX_ICON_ELEMENTS];
    let definitions = catalog(definition);
    let accepted = MAX_RENDERED_ICON_ELEMENTS / MAX_ICON_ELEMENTS;
    let visited = std::cell::Cell::new(0);
    let references =
        std::iter::repeat_n("beacon", 1000).inspect(|_| visited.set(visited.get() + 1));

    let at_limit = validate_icon_usage(&definitions, std::iter::repeat_n("beacon", accepted));
    let excessive = validate_icon_usage(&definitions, references);

    assert!(at_limit.is_ok());
    assert!(
        excessive
            .unwrap_err()
            .to_string()
            .contains("rendered elements")
    );
    assert_eq!(visited.get(), accepted + 1);
}

#[test]
fn icon_usage_rejects_repeated_large_paths_before_svg_expansion() {
    let definition = IconDefinition {
        radius: 16.0,
        elements: vec![line_path(MAX_ICON_PATH_COMMANDS); MAX_ICON_ELEMENTS],
    };

    let definitions = catalog(definition);
    let visited = std::cell::Cell::new(0);
    let references = std::iter::repeat_n("beacon", 511).inspect(|_| visited.set(visited.get() + 1));

    let stored = validate_icon_catalog(&definitions);
    let at_limit = validate_icon_usage(&definitions, ["beacon", "beacon"]);
    let excessive = validate_icon_usage(&definitions, references);

    assert!(stored.is_ok());
    assert!(at_limit.is_ok());
    assert!(
        excessive
            .unwrap_err()
            .to_string()
            .contains("rendered path commands")
    );
    assert_eq!(visited.get(), 3);
}

#[test]
fn icon_usage_rejects_missing_references_before_later_items() {
    let definitions = catalog(circle());
    let visited = std::cell::Cell::new(0);
    let references = ["missing", "beacon", "builtin:sourcefield"]
        .into_iter()
        .inspect(|_| visited.set(visited.get() + 1));

    let result = validate_icon_usage(&definitions, references);

    assert!(result.unwrap_err().to_string().contains("unresolved"));
    assert_eq!(visited.get(), 1);
}

#[test]
fn namespace_diagnostics_distinguish_organization_and_local_icon_rules() {
    // Arrange
    let cases = [
        ("/signal", "organization namespace"),
        ("bad.org/signal", "organization namespace"),
        ("bad org/signal", "organization namespace"),
        ("bad:org/signal", "organization namespace"),
        ("caf\u{00e9}/signal", "organization namespace"),
        ("Example_Labs/", "local identifier"),
        ("Example_Labs/Signal", "local identifier"),
        ("Example_Labs/signal_name", "local identifier"),
        ("Example_Labs/7signal", "local identifier"),
        ("Example_Labs/signal/extra", "local identifier"),
    ];

    let definitions = IconCatalog::new();

    // Act
    let results = cases
        .map(|(reference, expected)| (validate_icon_reference(&definitions, reference), expected));

    // Assert
    for (result, expected) in results {
        let message = result.unwrap_err().to_string();

        assert!(message.contains(expected), "expected {expected}: {message}");
    }
}

#[test]
fn namespace_length_does_not_relax_the_local_icon_length_limit() {
    // Arrange
    let namespace = "Organization_".repeat(12);
    let valid = format!("{namespace}/{}", "a".repeat(64));
    let invalid = format!("{namespace}/{}", "a".repeat(65));
    let definitions = BTreeMap::from([(valid.clone(), circle())]);

    // Act
    let catalog = validate_icon_catalog(&definitions);
    let reference = validate_icon_reference(&definitions, &valid);
    let too_long = validate_icon_reference(&definitions, &invalid);

    // Assert
    assert!(catalog.is_ok(), "{catalog:?}");
    assert!(reference.is_ok(), "{reference:?}");
    assert!(
        too_long
            .unwrap_err()
            .to_string()
            .contains("local identifier")
    );
}
