use super::*;

fn config() -> Config {
    toml::from_str(include_str!("../../../config/profile.toml")).expect("checked-in configuration")
}

fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("../../../config/offline-snapshot.json"))
        .expect("checked-in historical snapshot")
}

#[test]
fn approved_content_builds_nine_projects_and_nine_packages() {
    let config = config();

    validate_config(&config).expect("approved valid configuration");
    let state = build_state(&config, &snapshot(), "test").unwrap();

    validate_state(&state).expect("closed graph and safe public links");
    assert_eq!(config.projects.len(), 9);
    assert_eq!(state.stats.package_count, 9);
    assert_eq!(
        state.profile.tagline,
        "Sample Developer \u{b7} .NET, developer tools, and open source."
    );
    assert_eq!(
        config
            .projects
            .iter()
            .find(|project| project.id == "agent-tools")
            .unwrap()
            .components
            .len(),
        6
    );
    assert_eq!(state.canvas.width, 1800);
    assert_eq!(state.canvas.height, 1795);
    assert_eq!(state.learning.len(), 2);
    assert_eq!(state.presentation.hardware.len(), 3);
    assert!(state.nodes.iter().any(|node| node.label == "TraceDemo"));
    assert!(
        !serde_json::to_string(&state)
            .unwrap()
            .contains("StackTrace")
    );
}

#[test]
fn unordered_inputs_produce_identical_hash_geometry_and_content() {
    let original = config();
    let mut reordered = original.clone();
    reordered.projects.reverse();
    reordered.domains.reverse();
    reordered.technologies.reverse();
    reordered.publications.reverse();
    for publication in &mut reordered.publications {
        publication.packages.reverse();
        publication.technologies.reverse();
    }

    for project in &mut reordered.projects {
        project.components.reverse();
        project.implemented_with.reverse();
    }

    let first_snapshot = snapshot();
    let mut reordered_snapshot = first_snapshot.clone();
    reordered_snapshot.repositories.reverse();
    reordered_snapshot.organizations.reverse();
    reordered_snapshot.packages.reverse();
    reordered_snapshot.sources.reverse();

    let first = build_state(&original, &first_snapshot, "same-time").unwrap();
    let second = build_state(&reordered, &reordered_snapshot, "same-time").unwrap();

    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
}

#[test]
fn generation_and_fetch_timestamps_do_not_change_semantic_identity() {
    let config = config();
    let original = snapshot();
    let mut later = original.clone();
    later.fetched_at = "a different observation timestamp".into();

    let first = build_state(&config, &original, "first").unwrap();
    let second = build_state(&config, &later, "second").unwrap();

    assert_eq!(first.semantic_hash, second.semantic_hash);
    assert_ne!(first.generated_at, second.generated_at);
}

#[test]
fn semantic_copy_changes_change_the_hash() {
    let original = config();
    let mut revised = original.clone();
    revised.projects[0].summary = "Updated approved description.".into();

    let first = build_state(&original, &snapshot(), "same").unwrap();
    let second = build_state(&revised, &snapshot(), "same").unwrap();

    assert_ne!(first.semantic_hash, second.semantic_hash);
}

#[test]
fn project_rename_and_package_addition_do_not_require_validator_edits() {
    let mut config = config();
    config.projects[0].id = "renamed-agent-tools".into();
    config.publications[0].packages.push(PackageConfig {
        id: "Example.Extra".into(),
        family: "extension".into(),
        url: "https://www.nuget.org/packages/Example.Extra/".into(),
        anchor: None,
        summary: "Another public package.".into(),
    });

    let validation = validate_config(&config);
    let state = build_state(&config, &snapshot(), "test").unwrap();

    assert!(validation.is_ok());
    assert_eq!(state.stats.package_count, 10);
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.id == "project:renamed-agent-tools")
    );
    assert!(validate_state(&state).is_ok());
}

#[test]
fn missing_account_and_partial_download_counts_remain_unknown() {
    let config = config();
    let mut partial = snapshot();
    partial.user = AccountSnapshot::default();
    partial.organizations.clear();
    partial.packages[0].total_downloads = None;

    let state = build_state(&config, &partial, "test").unwrap();

    assert_eq!(state.stats.personal_public_repositories, None);
    assert_eq!(state.stats.organization_public_repositories, None);
    assert_eq!(state.stats.followers, None);
    assert_eq!(state.stats.package_downloads, None);
}

#[test]
fn complete_zero_values_are_distinct_from_missing_values() {
    let config = config();
    let mut complete = snapshot();
    complete.user.public_repositories = 0;
    complete.user.followers = 0;
    for package in &mut complete.packages {
        package.total_downloads = Some(0);
    }

    let state = build_state(&config, &complete, "test").unwrap();

    assert_eq!(state.stats.personal_public_repositories, Some(0));
    assert_eq!(state.stats.followers, Some(0));
    assert_eq!(state.stats.package_downloads, Some(0));
}

#[test]
fn source_failure_never_promotes_stale_account_to_live_count() {
    let config = config();
    let mut partial = snapshot();
    partial
        .sources
        .iter_mut()
        .find(|source| source.source == "github:user")
        .unwrap()
        .status = DataStatus::Missing;

    let state = build_state(&config, &partial, "test").unwrap();

    assert_eq!(state.stats.personal_public_repositories, None);
    assert_eq!(state.stats.followers, None);
}

#[test]
fn private_names_are_allowed_but_private_repository_links_are_rejected() {
    let mut config = config();
    let project = config
        .projects
        .iter_mut()
        .find(|project| project.visibility == Visibility::PrivateAbstract)
        .unwrap();

    project.repository = Some("sample-user/private-project".into());

    let result = validate_config(&config);

    assert!(matches!(result, Err(ValidationError::PrivateUrl(_))));
}

#[test]
fn script_credentials_and_ambiguous_link_schemes_are_rejected() {
    for url in [
        "javascript:alert(1)",
        "http://example.com/",
        "https://user@example.com/",
        "https://example.com\\@evil.test/",
        "https://example.com/\nscript",
    ] {
        let mut config = config();
        config.learning[0].url = url.into();

        let result = validate_config(&config);

        assert!(
            matches!(result, Err(ValidationError::InvalidValue(_))),
            "{url}"
        );
    }
}

#[test]
fn broken_affinity_reference_is_rejected() {
    let mut config = config();
    config.technologies[0].affinities = vec!["missing".into()];

    let result = validate_config(&config);

    assert!(matches!(result, Err(ValidationError::UnknownDomain { .. })));
}

#[test]
fn broken_learning_reference_is_rejected() {
    let mut config = super::contract_tests::config();
    config.learning[0].interest = "missing".into();

    assert!(validate_config(&config).is_err());
}

#[test]
fn broken_edge_reference_is_rejected() {
    let mut state = build_state(&super::contract_tests::config(), &snapshot(), "test").unwrap();
    state.edges[0].to = "missing".into();

    assert!(matches!(
        validate_state(&state),
        Err(ValidationError::MissingNode(_))
    ));
}

#[test]
fn nonfinite_and_out_of_bounds_authored_geometry_are_rejected() {
    for coordinate in [f32::NAN, f32::INFINITY, -1.0, 10000.0] {
        let mut config = config();
        config.projects[0].anchor[0] = coordinate;

        let result = validate_config(&config);

        assert!(matches!(result, Err(ValidationError::InvalidValue(_))));
    }
}

#[test]
fn all_render_settings_reach_the_shared_canvas() {
    let mut config = config();
    config.render.show_activity_orbit = false;
    config.render.show_state_hash = false;
    config.render.show_interests_in_readme = false;
    config.render.show_technology_labels = false;
    config.render.detail_level = "detailed".into();

    let state = build_state(&config, &snapshot(), "test").unwrap();

    assert!(!state.canvas.show_activity_orbit);
    assert!(!state.canvas.show_state_hash);
    assert!(!state.canvas.show_interests_in_readme);
    assert!(!state.canvas.show_technology_labels);
    assert!(state.canvas.show_details);
}

#[test]
fn discovery_is_order_independent_and_uses_renamed_ownership_domains() {
    let mut config = config();
    config.collection.visualize_discovered_repositories = true;
    let domain = config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "personal")
        .unwrap();

    domain.id = "my-projects".into();
    for project in &mut config.projects {
        if project.domain == "personal" {
            project.domain = "my-projects".into();
        }
    }

    for technology in &mut config.technologies {
        for affinity in &mut technology.affinities {
            if affinity == "personal" {
                *affinity = "my-projects".into();
            }
        }
    }

    let mut source = snapshot();
    for name in ["alpha", "beta"] {
        source.repositories.push(RepositorySnapshot {
            owner: "sample-user".into(),
            name: name.into(),
            full_name: format!("sample-user/{name}"),
            url: format!("https://github.com/sample-user/{name}"),
            ..RepositorySnapshot::default()
        });
    }

    let first = build_state(&config, &source, "same").unwrap();
    source.repositories.reverse();
    let second = build_state(&config, &source, "same").unwrap();

    assert!(validate_config(&config).is_ok());
    assert!(validate_state(&first).is_ok());
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    assert!(
        first
            .nodes
            .iter()
            .any(|node| node.id == "repository:sample-user/alpha"
                && node.domain.as_deref() == Some("my-projects"))
    );
}

#[test]
fn duplicate_package_case_is_rejected() {
    let mut config = config();
    let mut duplicate = config.publications[0].packages[0].clone();
    duplicate.id = duplicate.id.to_ascii_lowercase();
    config.publications[0].packages.push(duplicate);

    let result = validate_config(&config);

    assert!(matches!(result, Err(ValidationError::DuplicateId(_))));
}

#[test]
fn invalid_state_geometry_is_rejected() {
    let mut state = build_state(&super::contract_tests::config(), &snapshot(), "test").unwrap();
    state.nodes[0].x = f32::NAN;

    assert!(matches!(
        validate_state(&state),
        Err(ValidationError::InvalidValue(_))
    ));
}

#[test]
fn authorized_snapshot_count_survives_cli_opt_in_without_config_mutation() {
    for count in [0, 27] {
        let config = config();
        let mut source = snapshot();
        source.private_repository_count = Some(count);

        let state = build_state(&config, &source, "test").unwrap();

        assert!(!config.collection.collect_private_repository_count);
        assert_eq!(state.stats.private_repository_count, Some(count));
    }
}

#[test]
fn absent_authorized_snapshot_count_remains_absent() {
    let mut config = config();
    config.collection.collect_private_repository_count = true;
    let mut source = snapshot();
    source.private_repository_count = None;

    let state = build_state(&config, &source, "test").unwrap();

    assert_eq!(state.stats.private_repository_count, None);
}
