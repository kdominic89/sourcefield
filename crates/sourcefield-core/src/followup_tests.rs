use crate::*;

fn config() -> Config {
    toml::from_str(include_str!("../../../config/profile.toml")).unwrap()
}

fn cache_case() -> (Config, Snapshot, Snapshot) {
    let config = config();
    let package = &config.publications[0].packages[0];
    let source = format!("nuget:{}", package.id.to_ascii_lowercase());
    let current = Snapshot {
        sources: vec![SourceStatus {
            source: source.clone(),
            status: DataStatus::Missing,
        }],
        ..Snapshot::default()
    };
    let cache = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T12:30:00Z".into(),
        sources: vec![SourceStatus {
            source,
            status: DataStatus::Live,
        }],
        packages: vec![PackageSnapshot {
            id: package.id.clone(),
            version: Some("1.0.0".into()),
            ..PackageSnapshot::default()
        }],
        ..Snapshot::default()
    };

    (config, current, cache)
}

#[test]
fn preview_package_cache_is_not_a_live_observation() {
    // Arrange
    let (config, mut current, mut cache) = cache_case();
    cache.mode = SnapshotMode::Preview;

    // Act
    restore_package_cache(&mut current, &cache, &config);

    // Assert
    assert!(current.packages.is_empty());
}

#[test]
fn undated_package_cache_is_not_restored() {
    // Arrange
    let (config, mut current, mut cache) = cache_case();
    cache.fetched_at.clear();

    // Act
    restore_package_cache(&mut current, &cache, &config);

    // Assert
    assert!(current.packages.is_empty());
}

#[test]
fn missing_package_source_is_not_restored_from_a_dated_capture() {
    // Arrange
    let (config, mut current, mut cache) = cache_case();
    cache.sources[0].status = DataStatus::Missing;

    // Act
    restore_package_cache(&mut current, &cache, &config);

    // Assert
    assert!(current.packages.is_empty());
}

#[test]
fn wrapping_trailing_spaces_does_not_create_a_line() {
    // Arrange
    let input = "abcdef   ";

    // Act
    let lines = wrapped_lines(input, 6).collect::<Vec<_>>();

    // Assert
    assert_eq!(lines, ["abcdef"]);
}

#[test]
fn discovered_package_follows_all_implicit_package_rows() {
    // Arrange
    let mut config = config();
    let group = &mut config.publications[0];
    for package in &mut group.packages {
        package.anchor = None;
    }
    let id = format!("{}.Added", group.discovery_prefixes[0]);
    let owner = group
        .owner
        .clone()
        .or(config.collection.nuget_owner.clone())
        .unwrap();
    let expected_y = group.anchor[1] + 50.0 + group.packages.len() as f32 * PACKAGE_ROW_SPACING;
    let snapshot = Snapshot {
        packages: vec![PackageSnapshot {
            id: id.clone(),
            owner: Some(owner),
            version: Some("1.0.0".into()),
            ..PackageSnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let expanded = expanded_config(&config, &snapshot);

    // Assert
    let added = expanded.publications[0]
        .packages
        .iter()
        .find(|package| package.id == id)
        .unwrap();
    assert_eq!(added.anchor.unwrap()[1], expected_y);
}

#[test]
fn usable_observation_checks_real_dates_and_preserves_non_preview_modes() {
    // Arrange
    let cases = [
        (SnapshotMode::Live, "2026-10-01T12:30:00Z", true),
        (SnapshotMode::Partial, "2026-10-01T12:30:00+02:00", true),
        (SnapshotMode::Fallback, "2024-02-29T12:30:00.123Z", true),
        (SnapshotMode::Preview, "2026-10-01T12:30:00Z", false),
        (SnapshotMode::Live, "2026-02-31T12:30:00Z", false),
        (SnapshotMode::Live, "2026-10-01", false),
        (SnapshotMode::Live, "", false),
    ];

    // Act
    let results = cases.map(|(mode, date, expected)| {
        let capture = Snapshot {
            mode,
            fetched_at: date.into(),
            ..Snapshot::default()
        };
        (usable_observation(&capture), expected)
    });

    // Assert
    assert!(results.iter().all(|(actual, expected)| actual == expected));
}

#[test]
fn package_cache_admission_respects_each_source_status() {
    // Arrange
    let cases = [
        (DataStatus::Live, true),
        (DataStatus::Fallback, true),
        (DataStatus::Partial, true),
        (DataStatus::Preview, false),
        (DataStatus::Missing, false),
    ];

    // Act
    let results = cases.map(|(status, expected)| {
        let (config, mut current, mut cache) = cache_case();
        cache.mode = SnapshotMode::Partial;
        cache.sources[0].status = status;
        restore_package_cache(&mut current, &cache, &config);
        (!current.packages.is_empty(), expected, current)
    });

    // Assert
    for (actual, expected, current) in results {
        assert_eq!(actual, expected);
        if expected {
            assert_eq!(current.mode, SnapshotMode::Partial);
            assert!(
                current
                    .sources
                    .iter()
                    .any(|source| source.status == DataStatus::Fallback)
            );
        }
    }
}

#[test]
fn wrapping_preserves_physical_blank_lines_unicode_and_empty_input() {
    // Arrange
    let cases = [
        ("", vec![]),
        ("\n", vec![""]),
        ("ab\n\ncd", vec!["ab", "", "cd"]),
        (
            "\u{00e9}\u{00e9}   \n\nxy",
            vec!["\u{00e9}\u{00e9}", "", "xy"],
        ),
        ("ab\r\ncd", vec!["ab", "cd"]),
    ];

    // Act
    let results =
        cases.map(|(input, expected)| (wrapped_lines(input, 2).collect::<Vec<_>>(), expected));

    // Assert
    for (actual, expected) in results {
        assert_eq!(actual, expected);
    }
}

#[test]
fn mixed_anchors_append_below_actual_max_without_moving_existing_packages() {
    // Arrange
    let mut config = config();
    let group = &mut config.publications[0];
    group.packages.sort_by(|left, right| left.id.cmp(&right.id));
    for package in &mut group.packages {
        package.anchor = None;
    }
    let explicit = [group.anchor[0] + 33.0, group.anchor[1] + 333.0];
    group.packages[0].anchor = Some(explicit);
    let expected = group
        .packages
        .iter()
        .enumerate()
        .map(|(index, package)| {
            (
                package.id.clone(),
                package_anchor(group.anchor, package, index),
            )
        })
        .collect::<Vec<_>>();
    let id = format!("{}.AAA", group.discovery_prefixes[0]);
    let owner = group
        .owner
        .clone()
        .or(config.collection.nuget_owner.clone())
        .unwrap();
    let snapshot = Snapshot {
        packages: vec![PackageSnapshot {
            id: id.clone(),
            owner: Some(owner),
            version: Some("1.0.0".into()),
            ..PackageSnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let expanded = expanded_config(&config, &snapshot);
    let state = build_state(&expanded, &snapshot, "same").unwrap();

    // Assert
    let actual = &expanded.publications[0];
    for (id, anchor) in expected {
        let package = actual
            .packages
            .iter()
            .find(|package| package.id == id)
            .unwrap();
        assert_eq!(package.anchor, Some(anchor));
        let node = state
            .nodes
            .iter()
            .find(|node| node.id == format!("package:{id}"))
            .unwrap();
        assert_eq!([node.x, node.y], anchor);
    }
    let added = actual
        .packages
        .iter()
        .find(|package| package.id == id)
        .unwrap();
    assert_eq!(added.anchor.unwrap()[1], explicit[1] + PACKAGE_ROW_SPACING);
}

#[test]
fn generated_state_uses_shared_schema_version() {
    // Arrange
    let config = config();

    // Act
    let state = build_state(&config, &Snapshot::default(), "test").unwrap();

    // Assert
    assert_eq!(state.schema, STATE_SCHEMA_VERSION);
}

#[test]
fn third_review_implicit_package_uses_approved_horizontal_offset() {
    let config = config();
    let mut package = config.publications[0].packages[0].clone();
    package.anchor = None;

    let actual = package_anchor([100.0, 200.0], &package, 2);

    assert_eq!(actual, [116.0, 430.0]);
}

#[test]
fn third_review_explicit_package_anchor_is_preserved() {
    let config = config();
    let mut package = config.publications[0].packages[0].clone();
    package.anchor = Some([71.0, 83.0]);

    let actual = package_anchor([100.0, 200.0], &package, 2);

    assert_eq!(actual, [71.0, 83.0]);
}

#[test]
fn third_review_discovery_into_empty_group_uses_approved_offset() {
    let mut config = config();
    let group = &config.publications[0];
    let owner = publication_owner(&config, group).unwrap().to_string();
    let snapshot = Snapshot {
        packages: vec![PackageSnapshot {
            id: group.packages[0].id.clone(),
            owner: Some(owner),
            version: Some("1.0.0".into()),
            ..PackageSnapshot::default()
        }],
        ..Snapshot::default()
    };
    let anchor = group.anchor;
    config.publications[0].packages.clear();

    let expanded = expanded_config(&config, &snapshot);

    assert_eq!(expanded.publications[0].packages.len(), 1);
    assert_eq!(
        expanded.publications[0].packages[0].anchor,
        Some([anchor[0] + 16.0, anchor[1] + 50.0])
    );
}

#[test]
fn supported_observation_snapshot_schema_builds_public_state() {
    // Arrange
    let config = config();
    let snapshot = Snapshot::default();

    // Act
    let result = build_state(&config, &snapshot, "test");

    // Assert
    let state = result.unwrap();
    assert_eq!(state.schema, STATE_SCHEMA_VERSION);
    assert!(!state.nodes.is_empty());
}

#[test]
fn zero_observation_snapshot_schema_reports_actual_version() {
    // Arrange
    let config = config();
    let snapshot = Snapshot {
        schema_version: 0,
        ..Snapshot::default()
    };

    // Act
    let result = build_state(&config, &snapshot, "test");

    // Assert
    let error = result.unwrap_err();
    assert!(matches!(error, GraphError::UnsupportedSnapshotVersion(0)));
    assert_eq!(
        error.to_string(),
        "unsupported observation snapshot schema: 0; expected 1"
    );
}

#[test]
fn unknown_observation_snapshot_schema_reports_actual_version() {
    // Arrange
    let config = config();
    let snapshot = Snapshot {
        schema_version: 999,
        ..Snapshot::default()
    };

    // Act
    let result = build_state(&config, &snapshot, "test");

    // Assert
    let error = result.unwrap_err();
    assert!(matches!(error, GraphError::UnsupportedSnapshotVersion(999)));
    assert_eq!(
        error.to_string(),
        "unsupported observation snapshot schema: 999; expected 1"
    );
}

#[test]
fn unsupported_configuration_keeps_configuration_diagnosis() {
    // Arrange
    let mut config = config();
    config.version = 999;
    let snapshot = Snapshot::default();

    // Act
    let result = build_state(&config, &snapshot, "test");

    // Assert
    let error = result.unwrap_err();
    assert!(matches!(
        error,
        GraphError::Validation(ValidationError::UnsupportedVersion)
    ));
    assert_eq!(error.to_string(), "configuration version must be 1");
}
