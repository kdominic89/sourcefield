//! Shared icon catalog integration across authored configuration, graph state, and imports.

use sourcefield_core::{
    Config, IconDefinition, LayoutAssignments, NodeKind, OrganizationManifest, Snapshot,
    build_state, compose_organizations, parse_organization, validate_config, validate_state,
};

fn config() -> Config {
    toml::from_str(include_str!("../../../config/profile.toml")).unwrap()
}

fn icon() -> IconDefinition {
    serde_json::from_value(serde_json::json!({
        "radius": 16,
        "elements": [{"geometry": {"shape": "circle", "center": [0, 0], "radius": 4}}]
    }))
    .unwrap()
}

fn empty_config() -> Config {
    let mut config = config();
    config.domains.clear();
    config.projects.clear();
    config.publications.clear();
    config.technologies.clear();
    config.collection.github_organizations.clear();

    config
}

fn organization(id: &str) -> OrganizationManifest {
    let mut manifest: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();
    manifest.id = id.into();
    manifest.publications.clear();
    manifest.icons.insert("signal".into(), icon());
    manifest.projects[0].icon = Some("signal".into());

    manifest
}

fn hash(config: &Config) -> String {
    build_state(config, &Snapshot::default(), "same-time")
        .unwrap()
        .semantic_hash
}

#[test]
fn legacy_input_omits_icon_fields_and_preserves_semantic_hash() {
    // Arrange
    let config = config();
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../config/offline-snapshot.json")).unwrap();

    // Act
    let state = build_state(&config, &snapshot, "1970-01-01T00:00:00Z").unwrap();
    let config_json = serde_json::to_value(&config).unwrap();
    let state_json = serde_json::to_value(&state).unwrap();

    // Assert
    assert_eq!(state.semantic_hash, "5B663867C0693D1D");
    assert!(config_json.get("icons").is_none());
    assert!(state_json.get("icons").is_none());
    assert!(
        config_json["projects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|project| project.get("icon").is_none())
    );
    assert!(
        state_json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node.get("icon").is_none())
    );
}

#[test]
fn many_projects_share_one_state_definition() {
    // Arrange
    let mut config = config();
    let mut project = config.projects[0].clone();
    project.components.clear();
    project.implemented_with.clear();
    project.integrates.clear();
    project.targets.clear();
    project.icon = Some("signal".into());
    config.projects = (0..96)
        .map(|index| {
            let mut project = project.clone();
            project.id = format!("shared-{index}");
            project.anchor = [
                300.0 + (index % 3) as f32 * 500.0,
                4000.0 + (index / 3) as f32 * 240.0,
            ];

            project
        })
        .collect();
    config.icons.insert("signal".into(), icon());
    config.render.height = 13000;

    // Act
    let state = build_state(&config, &Snapshot::default(), "same-time").unwrap();
    let encoded = serde_json::to_string(&state).unwrap();

    // Assert
    assert_eq!(state.icons.len(), 1);
    assert_eq!(
        state
            .nodes
            .iter()
            .filter(|node| node.icon.as_deref() == Some("signal"))
            .count(),
        96
    );
    assert_eq!(encoded.matches("\"elements\"").count(), 1);
}

#[test]
fn builtin_references_need_no_authored_definition() {
    // Arrange
    let mut config = config();
    config.projects[0].icon = Some("builtin:sourcefield".into());
    config.projects[1].icon = Some("builtin:database-safe".into());

    // Act
    let state = build_state(&config, &Snapshot::default(), "same-time").unwrap();

    // Assert
    assert!(state.icons.is_empty());
    assert_eq!(
        state
            .nodes
            .iter()
            .filter(|node| node.icon.is_some())
            .count(),
        2
    );
}

#[test]
fn unknown_config_reference_is_rejected() {
    // Arrange
    let mut config = config();
    config.projects[0].icon = Some("absent".into());

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(result.is_err());
}

#[test]
fn invalid_config_catalog_fails_before_generic_serialization() {
    // Arrange
    let mut config = config();
    let mut definition = icon();
    definition.radius = f32::NAN;
    config.icons.insert("signal".into(), definition);

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(result.is_err());
}

#[test]
fn standalone_state_rejects_invalid_catalog_and_reference() {
    // Arrange
    let mut missing = build_state(&config(), &Snapshot::default(), "same-time").unwrap();
    missing
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Project)
        .unwrap()
        .icon = Some("absent".into());
    let mut invalid = missing.clone();
    let mut definition = icon();
    definition.radius = f32::INFINITY;
    invalid.icons.insert("absent".into(), definition);

    // Act
    let results = [validate_state(&missing), validate_state(&invalid)];

    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn standalone_state_rejects_icons_on_unrendered_node_kinds() {
    // Arrange
    let mut state = build_state(&config(), &Snapshot::default(), "same-time").unwrap();
    state
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Domain)
        .unwrap()
        .icon = Some("builtin:sourcefield".into());

    // Act
    let result = validate_state(&state);

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("only on project nodes")
    );
}

#[test]
fn definition_insertion_order_does_not_change_identity() {
    // Arrange
    let mut first = config();
    first.icons.insert("alpha".into(), icon());
    first.icons.insert("beta".into(), icon());
    first.projects[0].icon = Some("alpha".into());
    let mut second = first.clone();
    second.icons.clear();
    second.icons.insert("beta".into(), icon());
    second.icons.insert("alpha".into(), icon());

    // Act
    let hashes = (hash(&first), hash(&second));

    // Assert
    assert_eq!(hashes.0, hashes.1);
}

#[test]
fn unused_definition_is_retained_in_state_and_changes_identity() {
    // Arrange
    let mut base = config();
    base.icons.insert("used".into(), icon());
    base.projects[0].icon = Some("used".into());
    let mut with_unused = base.clone();
    with_unused.icons.insert("spare".into(), icon());

    // Act
    let before = build_state(&base, &Snapshot::default(), "same-time").unwrap();
    let after = build_state(&with_unused, &Snapshot::default(), "same-time").unwrap();
    let serialized = serde_json::to_value(&after).unwrap();

    // Assert
    assert_ne!(before.semantic_hash, after.semantic_hash);
    assert_eq!(after.icons, with_unused.icons);
    assert_eq!(
        serialized["icons"]["spare"],
        serde_json::to_value(icon()).unwrap()
    );
    assert!(
        after
            .nodes
            .iter()
            .all(|node| node.icon.as_deref() != Some("spare"))
    );
}

#[test]
fn editing_unused_definition_changes_identity_without_selecting_it() {
    // Arrange
    let mut base = config();
    base.icons.insert("spare".into(), icon());
    let mut changed = base.clone();
    changed.icons.get_mut("spare").unwrap().radius = 17.0;

    // Act
    let before = build_state(&base, &Snapshot::default(), "same-time").unwrap();
    let after = build_state(&changed, &Snapshot::default(), "same-time").unwrap();

    // Assert
    assert_ne!(before.semantic_hash, after.semantic_hash);
    assert_eq!(after.icons["spare"].radius, 17.0);
    assert!(after.nodes.iter().all(|node| node.icon.is_none()));
}

#[test]
fn definition_geometry_paint_motion_and_reference_change_identity() {
    // Arrange
    let mut base = config();
    base.icons.insert("alpha".into(), icon());
    base.icons.insert("beta".into(), icon());
    base.projects[0].icon = Some("alpha".into());
    let mut radius = base.clone();
    radius.icons.get_mut("alpha").unwrap().radius = 17.0;
    let mut paint = base.clone();
    let mut value = serde_json::to_value(icon()).unwrap();
    value["elements"][0]["stroke"] = "mint".into();
    paint
        .icons
        .insert("alpha".into(), serde_json::from_value(value).unwrap());
    let mut motion = base.clone();
    let mut value = serde_json::to_value(icon()).unwrap();
    value["elements"][0]["motion"] = serde_json::json!({"kind": "signal", "phase": 1});
    motion
        .icons
        .insert("alpha".into(), serde_json::from_value(value).unwrap());
    let mut reference = base.clone();
    reference.projects[0].icon = Some("beta".into());

    // Act
    let baseline = hash(&base);
    let changed = [&radius, &paint, &motion, &reference].map(hash);

    // Assert
    assert!(changed.iter().all(|hash| hash != &baseline));
}

#[test]
fn organizations_namespace_identical_local_icons_and_preserve_builtin_refs() {
    // Arrange
    let base = empty_config();
    let mut alpha = organization("alpha");
    let beta = organization("beta");
    let mut builtin = alpha.projects[0].clone();
    builtin.id = "builtin".into();
    builtin.icon = Some("builtin:sourcefield".into());
    alpha.projects.push(builtin);

    // Act
    let (composed, _) =
        compose_organizations(&base, &[alpha, beta], &LayoutAssignments::new()).unwrap();

    // Assert
    assert_eq!(
        composed
            .icons
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["alpha/signal", "beta/signal"]
    );
    assert!(
        composed
            .projects
            .iter()
            .any(|project| project.icon.as_deref() == Some("alpha/signal"))
    );
    assert!(
        composed
            .projects
            .iter()
            .any(|project| project.icon.as_deref() == Some("beta/signal"))
    );
    assert!(
        composed
            .projects
            .iter()
            .any(|project| project.icon.as_deref() == Some("builtin:sourcefield"))
    );
}

#[test]
fn composition_rejects_existing_qualified_definition_collision() {
    // Arrange
    let mut base = empty_config();
    base.icons.insert("alpha/signal".into(), icon());

    // Act
    let result = compose_organizations(&base, &[organization("alpha")], &LayoutAssignments::new());

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("duplicate identifier: alpha/signal")
    );
}

#[test]
fn manifest_rejects_missing_local_reference_and_namespace_impersonation() {
    // Arrange
    let invalid = ["absent", "beta/signal", "builtin:unknown"];

    // Act
    let results = invalid.map(|reference| {
        let mut organization = organization("alpha");
        organization.projects[0].icon = Some(reference.into());

        compose_organizations(&empty_config(), &[organization], &LayoutAssignments::new())
    });

    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn parser_rejects_nonlocal_or_invalid_icon_definition_keys() {
    // Arrange
    let invalid = ["beta/signal", "Signal", "builtin:sourcefield"];

    // Act
    let results = invalid.map(|key| {
        let mut organization = organization("alpha");
        organization.icons.insert(key.into(), icon());

        parse_organization(&toml::to_string(&organization).unwrap())
    });

    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn composition_checks_aggregate_catalog_budget() {
    // Arrange
    let mut organizations = [organization("alpha"), organization("beta")];
    let mut definition = icon();
    definition.elements = vec![definition.elements[0].clone(); 20];

    for organization in &mut organizations {
        organization.projects[0].icon = None;
        organization.icons = (0..64)
            .map(|index| (format!("icon-{index}"), definition.clone()))
            .collect();
    }

    // Act
    let standalone = organizations
        .iter()
        .map(|organization| sourcefield_core::validate_icon_catalog(&organization.icons))
        .collect::<Vec<_>>();
    let aggregate =
        compose_organizations(&empty_config(), &organizations, &LayoutAssignments::new());

    // Assert
    assert!(standalone.into_iter().all(|result| result.is_ok()));
    assert!(aggregate.is_err());
}

#[test]
fn repeated_icon_references_cannot_bypass_rendered_usage_budget() {
    // Arrange
    let mut config = config();
    let mut definition = icon();
    definition.elements = vec![definition.elements[0].clone(); 64];
    let mut project = config.projects[0].clone();
    project.icon = Some("dense".into());
    project.components.clear();
    config.icons.insert("dense".into(), definition);
    config.projects = (0..129)
        .map(|index| {
            let mut project = project.clone();
            project.id = format!("dense-{index}");

            project
        })
        .collect();
    let mut state = build_state(&self::config(), &Snapshot::default(), "same-time").unwrap();
    state.icons = config.icons.clone();
    let mut node = state
        .nodes
        .iter()
        .find(|node| node.kind == NodeKind::Project)
        .unwrap()
        .clone();
    node.icon = Some("dense".into());
    state.nodes = (0..129)
        .map(|index| {
            let mut node = node.clone();
            node.id = format!("project:dense-{index}");

            node
        })
        .collect();

    // Act
    let results = [validate_config(&config), validate_state(&state)];

    // Assert
    assert!(results.into_iter().all(|result| {
        result
            .unwrap_err()
            .to_string()
            .contains("8192 rendered elements")
    }));
}

/// Exercise public authored parsing without relying on a persistent checkout fixture.
fn load_namespace_config(config: &Config) -> Result<Config, sourcefield_core::ConfigError> {
    static NEXT_FILE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let suffix = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "sourcefield-icon-namespace-{}-{suffix}.toml",
        std::process::id(),
    ));

    std::fs::write(&path, toml::to_string(config).unwrap()).unwrap();
    let result = sourcefield_core::load_config(&path);
    std::fs::remove_file(path).unwrap();

    result
}

/// Defer one personal project's icon until its selected organization is composed.
fn namespace_config(namespace: &str, local: &str) -> Config {
    let mut config = config();
    config.projects[0].icon = Some(format!("{namespace}/{local}"));
    config.imports.push(sourcefield_core::OrganizationImport {
        id: namespace.into(),
        source: sourcefield_core::OrganizationSource::Local {
            path: "organization.toml".into(),
        },
    });

    config
}

/// Preserve the full pre-icon organization-ID contract, including long and punctuation-led IDs.
fn legacy_namespaces() -> Vec<String> {
    ["Example_Labs", "7labs", "under_score", "_labs", "-labs"]
        .into_iter()
        .map(str::to_owned)
        .chain(std::iter::once("Long_Organization_".repeat(8)))
        .collect()
}

#[test]
fn namespace_authored_parser_defers_legacy_organization_ids_without_normalization() {
    // Arrange
    let namespaces = legacy_namespaces();
    let configs = namespaces
        .iter()
        .map(|name| namespace_config(name, "signal"))
        .collect::<Vec<_>>();

    // Act
    let results = configs
        .iter()
        .map(load_namespace_config)
        .collect::<Vec<_>>();

    // Assert
    for (namespace, result) in namespaces.iter().zip(results) {
        let loaded = result.unwrap_or_else(|error| panic!("{namespace}: {error}"));

        assert_eq!(loaded.imports.last().unwrap().id, *namespace);
        assert_eq!(
            loaded.projects[0].icon.as_deref(),
            Some(format!("{namespace}/signal").as_str())
        );
    }
}

#[test]
fn namespace_manifests_and_composition_preserve_legacy_organization_ids() {
    // Arrange
    let namespaces = legacy_namespaces();
    let base = empty_config();
    let inputs = namespaces
        .iter()
        .map(|name| toml::to_string(&organization(name)).unwrap())
        .collect::<Vec<_>>();

    // Act
    let results = inputs
        .iter()
        .map(|input| {
            let manifest = parse_organization(input).map_err(|error| error.to_string())?;

            compose_organizations(&base, &[manifest], &LayoutAssignments::new())
                .map_err(|error| error.to_string())
        })
        .collect::<Vec<_>>();

    // Assert
    for (namespace, result) in namespaces.iter().zip(results) {
        let (composed, _) = result.unwrap_or_else(|error| panic!("{namespace}: {error}"));
        let qualified = format!("{namespace}/signal");

        assert!(composed.icons.contains_key(&qualified));
        assert_eq!(
            composed.projects[0].icon.as_deref(),
            Some(qualified.as_str())
        );
        assert_eq!(composed.domains[0].id, *namespace);
    }
}

#[test]
fn namespace_standalone_state_accepts_legacy_organization_ids() {
    // Arrange
    let baseline = build_state(&config(), &Snapshot::default(), "same-time").unwrap();
    let states = legacy_namespaces()
        .iter()
        .map(|namespace| {
            let mut state = baseline.clone();
            let reference = format!("{namespace}/signal");
            state.icons.insert(reference.clone(), icon());
            state
                .nodes
                .iter_mut()
                .find(|node| node.kind == NodeKind::Project)
                .unwrap()
                .icon = Some(reference);

            serde_json::from_str::<sourcefield_core::ProfileState>(
                &serde_json::to_string(&state).unwrap(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();

    // Act
    let results = states.iter().map(validate_state).collect::<Vec<_>>();

    // Assert
    assert!(results.iter().all(Result::is_ok), "{results:?}");
}

#[test]
fn namespace_invalid_parts_fail_in_authored_parsing_and_standalone_state() {
    // Arrange
    let cases = [
        ("bad.org", "signal", "organization namespace"),
        ("bad org", "signal", "organization namespace"),
        ("bad:org", "signal", "organization namespace"),
        ("", "signal", "organization namespace"),
        ("Example_Labs", "Signal", "local identifier"),
        ("Example_Labs", "signal_name", "local identifier"),
        ("Example_Labs", "7signal", "local identifier"),
        ("Example_Labs", "signal/extra", "local identifier"),
    ];

    let baseline = build_state(&config(), &Snapshot::default(), "same-time").unwrap();
    let configs = cases.map(|(namespace, local, _)| namespace_config(namespace, local));
    let states = cases.map(|(namespace, local, _)| {
        let mut state = baseline.clone();
        let reference = format!("{namespace}/{local}");
        state.icons.insert(reference.clone(), icon());
        state
            .nodes
            .iter_mut()
            .find(|node| node.kind == NodeKind::Project)
            .unwrap()
            .icon = Some(reference);

        state
    });

    // Act
    let authored = configs
        .iter()
        .map(load_namespace_config)
        .collect::<Vec<_>>();
    let standalone = states.iter().map(validate_state).collect::<Vec<_>>();

    // Assert
    for ((authored, standalone), (_, _, expected)) in
        authored.into_iter().zip(standalone).zip(cases)
    {
        let authored = authored.unwrap_err().to_string();
        let standalone = standalone.unwrap_err().to_string();

        assert!(
            authored.contains(expected),
            "expected {expected}: {authored}"
        );
        assert!(
            standalone.contains(expected),
            "expected {expected}: {standalone}"
        );
    }
}
