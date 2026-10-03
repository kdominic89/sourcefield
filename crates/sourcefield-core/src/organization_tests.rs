use crate::*;

fn base() -> Config {
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    config.domains.clear();
    config.projects.clear();
    config.publications.clear();
    config.technologies.clear();
    config.collection.github_organizations.clear();

    config
}

fn organization(id: &str) -> OrganizationManifest {
    let mut value: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();

    value.id = id.into();
    value.owner = format!("{id}-owner");

    for project in &mut value.projects {
        project.repository = Some(format!("{id}-owner/{}", project.id));
    }

    for publication in &mut value.publications {
        publication.owner = value.owner.clone();
        publication.discovery_prefixes = vec![format!("{id}.Database")];
        publication.label = format!("{id}.Database");
        publication.label_prefix = format!("{id}.Database.");

        for package in &mut publication.packages {
            package.id = package.id.replace("Example", id);
            package.url = format!("https://www.nuget.org/packages/{}/", package.id);
        }
    }

    value
}

#[test]
fn two_organizations_scope_identical_local_ids() {
    // Arrange
    let config = base();
    let organizations = [organization("alpha"), organization("beta")];

    // Act
    let (composed, _) =
        compose_organizations(&config, &organizations, &LayoutAssignments::new()).unwrap();

    // Assert
    assert!(
        composed
            .projects
            .iter()
            .any(|item| item.id == "alpha/database")
    );
    assert!(
        composed
            .projects
            .iter()
            .any(|item| item.id == "beta/database")
    );
    assert_eq!(composed.collection.github_organizations.len(), 2);
}

#[test]
fn growth_preserves_existing_assignments_and_allocates_a_free_slot() {
    // Arrange
    let config = base();
    let mut organization = organization("alpha");
    let (_, previous) =
        compose_organizations(&config, &[organization.clone()], &LayoutAssignments::new()).unwrap();

    let mut new_project = organization.projects[0].clone();
    new_project.id = "aaa-new-project".into();
    organization.projects.push(new_project);

    // Act
    let (_, current) = compose_organizations(&config, &[organization], &previous).unwrap();

    // Assert
    assert_eq!(
        current["project:alpha/database"],
        previous["project:alpha/database"]
    );
    assert_ne!(
        current["project:alpha/database"],
        current["project:alpha/aaa-new-project"]
    );
}

#[test]
fn authored_layout_override_wins_over_previous_assignment() {
    // Arrange
    let mut config = base();
    config
        .layout
        .overrides
        .insert("project:alpha/database".into(), [300.0, 650.0]);
    let previous = [("project:alpha/database".into(), [400.0, 800.0])].into();

    // Act
    let (_, current) = compose_organizations(&config, &[organization("alpha")], &previous).unwrap();

    // Assert
    assert_eq!(current["project:alpha/database"], [300.0, 650.0]);
}

#[test]
fn stale_override_is_rejected() {
    // Arrange
    let mut config = base();
    config
        .layout
        .overrides
        .insert("project:alpha/removed".into(), [300.0, 650.0]);

    // Act
    let result =
        compose_organizations(&config, &[organization("alpha")], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("absent item"))
    );
}

#[test]
fn duplicate_scopes_are_rejected() {
    // Arrange
    let config = base();
    let organizations = [organization("alpha"), organization("alpha")];

    // Act
    let result = compose_organizations(&config, &organizations, &LayoutAssignments::new());

    // Assert
    assert!(matches!(result, Err(ValidationError::DuplicateId(_))));
}

#[test]
fn colliding_authored_projects_are_rejected() {
    // Arrange
    let mut config = base();
    let mut organization = organization("alpha");
    let mut second = organization.projects[0].clone();
    second.id = "second".into();
    organization.projects.push(second);
    config
        .layout
        .overrides
        .insert("project:alpha/database".into(), [300.0, 650.0]);
    config
        .layout
        .overrides
        .insert("project:alpha/second".into(), [300.0, 650.0]);

    // Act
    let result = compose_organizations(&config, &[organization], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("collision"))
    );
}

#[test]
fn organization_local_technology_references_cannot_escape_scope() {
    // Arrange
    let config = base();
    let mut organization = organization("alpha");
    organization.projects[0]
        .implemented_with
        .push("beta/rust".into());

    // Act
    let result = compose_organizations(&config, &[organization], &LayoutAssignments::new());

    // Assert
    assert!(matches!(
        result,
        Err(ValidationError::UnknownTechnology { .. })
    ));
}

#[test]
fn manifest_rejects_unknown_fields() {
    // Arrange
    let text = format!(
        "misspelled = true\n{}",
        include_str!("../../../examples/organization.toml")
    );

    // Act
    let result = parse_organization(&text);

    // Assert
    assert!(matches!(result, Err(ConfigError::Parse(_))));
}

#[test]
fn manifest_rejects_unsupported_schema() {
    // Arrange
    let text = include_str!("../../../examples/organization.toml").replacen(
        "schema_version = 1",
        "schema_version = 99",
        1,
    );

    // Act
    let result = parse_organization(&text);

    // Assert
    assert!(matches!(
        result,
        Err(ConfigError::Validation(ValidationError::UnsupportedVersion))
    ));
}

#[test]
fn old_config_version_key_requires_explicit_migration() {
    // Arrange
    let text = include_str!("../../../config/profile.toml").replacen(
        "schema_version = 1",
        "version = 1",
        1,
    );

    // Act
    let result = toml::from_str::<Config>(&text);

    // Assert
    assert!(result.is_err());
}

#[test]
fn organization_snapshot_excludes_personal_and_unselected_data() {
    // Arrange
    let mut config = base();
    config.profile.variant = ProfileVariant::Organization;
    config.collection.github_organizations = vec!["selected-owner".into()];
    let snapshot = Snapshot {
        user: AccountSnapshot {
            login: "private-user".into(),
            ..AccountSnapshot::default()
        },
        contributions: Some(ContributionSnapshot {
            restricted: 7,
            ..ContributionSnapshot::default()
        }),
        private_repository_count: Some(10),
        organizations: vec![AccountSnapshot {
            login: "unselected-owner".into(),
            ..AccountSnapshot::default()
        }],
        repositories: vec![RepositorySnapshot {
            owner: "unselected-owner".into(),
            name: "hidden".into(),
            full_name: "unselected-owner/hidden".into(),
            ..RepositorySnapshot::default()
        }],
        warnings: vec!["unselected-owner diagnostic".into()],
        ..Snapshot::default()
    };

    // Act
    let scoped = scope_snapshot(&config, &snapshot);

    // Assert
    assert!(scoped.user.login.is_empty());
    assert!(scoped.contributions.is_none());
    assert!(scoped.private_repository_count.is_none());
    assert!(scoped.organizations.is_empty());
    assert!(scoped.repositories.is_empty());
    assert!(scoped.warnings.is_empty());
}

#[test]
fn selected_public_repository_survives_scoping() {
    // Arrange
    let mut config = base();
    config
        .collection
        .github_organizations
        .push("selected-owner".into());
    let snapshot = Snapshot {
        repositories: vec![RepositorySnapshot {
            owner: "selected-owner".into(),
            name: "public".into(),
            full_name: "selected-owner/public".into(),
            ..RepositorySnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let scoped = scope_snapshot(&config, &snapshot);

    // Assert
    assert_eq!(scoped.repositories.len(), 1);
}

#[test]
fn managed_project_readme_escapes_untrusted_prose() {
    // Arrange
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    config.projects[0].label = "<script>[injected]".into();
    config.projects[0].show_in_readme = true;

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert!(readme.contains("&lt;script&gt;\\[injected\\]"));
    assert!(!readme.contains("<script>"));
}

#[test]
fn canonical_maintainer_reaches_organization_profile_and_identity() {
    // Arrange
    let mut config = base();
    config.profile.variant = ProfileVariant::Organization;
    config.profile.username = "alpha-owner".into();
    config.profile.organization = "alpha-owner".into();
    let mut organization = organization("alpha");
    organization.maintainer = Some(MaintainerConfig {
        username: "sample-admin".into(),
        role: "Core maintainer".into(),
        url: "https://github.com/sample-admin".into(),
    });
    let (composed, _) =
        compose_organizations(&config, &[organization], &LayoutAssignments::new()).unwrap();

    // Act
    let state = build_state(&composed, &Snapshot::default(), "test").unwrap();

    // Assert
    assert_eq!(
        state.profile.maintainer.as_ref().unwrap().username,
        "sample-admin"
    );
    assert_eq!(
        state.organizations[0].maintainer.as_ref().unwrap().username,
        "sample-admin"
    );
}

#[test]
fn excessive_graph_nodes_are_rejected_before_runtime() {
    // Arrange
    let config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    let mut state = build_state(&config, &Snapshot::default(), "test").unwrap();
    state.nodes.resize(513, state.nodes[0].clone());

    // Act
    let result = validate_state(&state);

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("512 nodes"))
    );
}

#[test]
fn unrelated_owner_failure_does_not_restore_another_owners_cache() {
    // Arrange
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    config.publications[0].owner = Some("selected-owner".into());
    let cache = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T00:00:00Z".into(),
        packages: vec![PackageSnapshot {
            id: config.publications[0].packages[0].id.clone(),
            owner: Some("selected-owner".into()),
            version: Some("1.0.0".into()),
            ..PackageSnapshot::default()
        }],
        ..Snapshot::default()
    };

    let mut snapshot = Snapshot {
        sources: vec![SourceStatus {
            source: "nuget:owner:unrelated-owner".into(),
            status: DataStatus::Missing,
        }],
        ..Snapshot::default()
    };

    // Act
    restore_package_cache(&mut snapshot, &cache, &config);

    // Assert
    assert!(snapshot.packages.is_empty());
}

#[test]
fn publication_growth_shifts_following_rows_without_moving_projects() {
    // Arrange
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    config.publications.truncate(2);
    config.publications[1].anchor[1] += 700.0;
    config.publications[0].discovery_prefixes = vec!["Future.Library".into()];
    config.publications[0].owner = Some("selected-owner".into());
    let snapshot = Snapshot {
        packages: vec![PackageSnapshot {
            id: "Future.Library.Adapter".into(),
            owner: Some("selected-owner".into()),
            version: Some("1.0.0".into()),
            ..PackageSnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let expanded = expanded_config(&config, &snapshot);

    // Assert
    assert_eq!(
        expanded.publications[1].anchor[1],
        config.publications[1].anchor[1] + 90.0
    );
    assert_eq!(expanded.projects[0].anchor, config.projects[0].anchor);
    assert_eq!(expanded.render.height, config.render.height + 90);
}

#[test]
fn growing_first_organization_moves_following_region_as_a_unit() {
    // Arrange
    let config = base();
    let mut first = organization("alpha");
    let second = organization("beta");
    let (_, previous) = compose_organizations(
        &config,
        &[first.clone(), second.clone()],
        &LayoutAssignments::new(),
    )
    .unwrap();

    for index in 0..6 {
        let mut project = first.projects[0].clone();
        project.id = format!("new-{index}");
        first.projects.push(project);
    }

    // Act
    let (_, current) = compose_organizations(&config, &[first, second], &previous).unwrap();

    // Assert
    let domain_shift = current["domain:beta"][1] - previous["domain:beta"][1];
    let project_shift = current["project:beta/database"][1] - previous["project:beta/database"][1];
    assert!(domain_shift > 0.0);
    assert_eq!(domain_shift, project_shift);
    assert_eq!(
        current["project:alpha/database"],
        previous["project:alpha/database"]
    );
}

#[test]
fn authored_radius_and_weight_reach_canonical_graph() {
    // Arrange
    let mut config = base();
    config
        .layout
        .radii
        .insert("project:alpha/database".into(), 48.0);
    config
        .layout
        .weights
        .insert("project:alpha/database".into(), 0.8);
    let (composed, _) =
        compose_organizations(&config, &[organization("alpha")], &LayoutAssignments::new())
            .unwrap();

    // Act
    let state = build_state(&composed, &Snapshot::default(), "test").unwrap();

    // Assert
    let project = state
        .nodes
        .iter()
        .find(|node| node.id == "project:alpha/database")
        .unwrap();

    assert_eq!(project.radius, 48.0);
    assert_eq!(project.weight, 0.8);
}

#[test]
fn stale_radius_override_is_rejected() {
    // Arrange
    let mut config = base();
    config
        .layout
        .radii
        .insert("project:alpha/deleted".into(), 48.0);

    // Act
    let result =
        compose_organizations(&config, &[organization("alpha")], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("absent item"))
    );
}

fn organization_config() -> Config {
    let mut config = base();
    config.profile.variant = ProfileVariant::Organization;
    config.profile.username = "alpha-owner".into();
    config.profile.organization = "alpha-owner".into();
    config.collection.github_user.clear();

    compose_organizations(&config, &[organization("alpha")], &LayoutAssignments::new())
        .unwrap()
        .0
}

#[test]
fn organization_accepts_empty_personal_github_user() {
    // Arrange
    let config = organization_config();

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn organization_rejects_stale_additional_collection_owner() {
    // Arrange
    let mut config = organization_config();
    config
        .collection
        .github_organizations
        .push("unrelated-owner".into());

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("owners differ"))
    );
}

#[test]
fn organization_rejects_foreign_project_repository() {
    // Arrange
    let mut config = organization_config();
    config.projects[0].repository = Some("unrelated-owner/project".into());

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("unrelated owner"))
    );
}

#[test]
fn raw_organization_snapshot_never_admits_stale_additional_owner() {
    // Arrange
    let mut config = organization_config();
    config
        .collection
        .github_organizations
        .push("unrelated-owner".into());
    let snapshot = Snapshot {
        organizations: vec![AccountSnapshot {
            login: "unrelated-owner".into(),
            ..AccountSnapshot::default()
        }],
        repositories: vec![RepositorySnapshot {
            owner: "unrelated-owner".into(),
            full_name: "unrelated-owner/project".into(),
            name: "project".into(),
            ..RepositorySnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let scoped = scope_snapshot(&config, &snapshot);

    // Assert
    assert!(scoped.organizations.is_empty());
    assert!(scoped.repositories.is_empty());
}

#[test]
fn cross_organization_project_overlap_is_rejected() {
    // Arrange
    let mut config = base();
    config
        .layout
        .overrides
        .insert("project:alpha/database".into(), [300.0, 650.0]);
    config
        .layout
        .overrides
        .insert("project:beta/database".into(), [300.0, 650.0]);

    // Act
    let result = compose_organizations(
        &config,
        &[organization("alpha"), organization("beta")],
        &LayoutAssignments::new(),
    );

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("cross-domain collision"))
    );
}

#[test]
fn inline_composition_preserves_approved_canvas_height() {
    // Arrange
    let config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();

    // Act
    let (composed, _) = compose_organizations(&config, &[], &LayoutAssignments::new()).unwrap();

    // Assert
    assert_eq!(composed.render.height, config.render.height);
    assert_eq!(
        composed.publications[0].anchor,
        config.publications[0].anchor
    );
}

#[test]
fn snapshots_require_explicit_schema_version() {
    // Arrange
    let mut value = serde_json::to_value(Snapshot::default()).unwrap();
    value.as_object_mut().unwrap().remove("schema_version");

    // Act
    let result = serde_json::from_value::<Snapshot>(value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn prepared_graph_matches_convenience_projection() {
    // Arrange
    let config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    let snapshot = Snapshot::default();
    let expected = build_state(&config, &snapshot, "same-time").unwrap();
    let prepared = prepare_profile(&config, &snapshot).unwrap();

    // Act
    let actual = build_prepared_state(&prepared, "same-time").unwrap();

    // Assert
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn surface_label_wider_than_canvas_is_rejected() {
    // Arrange
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    config.projects[0].surface_label = "W".repeat(200);

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("surface label cannot fit canvas"))
    );
}

#[test]
fn overlapping_surface_labels_are_rejected_when_circles_do_not_overlap() {
    // Arrange
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();
    config.projects.truncate(2);
    config.publications.clear();
    config.projects[0].anchor = [300.0, 300.0];
    config.projects[1].anchor = [550.0, 300.0];

    for project in &mut config.projects {
        project.surface_label = "W".repeat(16);
        project.radius = Some(45.0);
        project.label_prefix = None;
    }

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("surface label collision"))
    );
}

#[test]
fn shared_technology_binding_rejects_conflicting_definition() {
    // Arrange
    let mut config = base();
    let organization = organization("alpha");
    let mut shared = organization.technologies[0].clone();
    shared.id = "shared-rust".into();
    shared.label = "Conflicting label".into();
    config.technologies.push(shared);
    config
        .shared_technologies
        .insert("alpha/rust".into(), "shared-rust".into());

    // Act
    let result = compose_organizations(&config, &[organization], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("definition conflicts"))
    );
}

#[test]
fn shared_technology_binding_rejects_missing_canonical_source() {
    // Arrange
    let mut config = base();
    let organization = organization("alpha");
    let mut shared = organization.technologies[0].clone();
    shared.id = "shared-rust".into();
    config.technologies.push(shared);
    config
        .shared_technologies
        .insert("alpha/missing".into(), "shared-rust".into());

    // Act
    let result = compose_organizations(&config, &[organization], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("absent canonical technology"))
    );
}

#[test]
fn shared_technology_binding_rejects_missing_consumer_target() {
    // Arrange
    let mut config = base();
    config
        .shared_technologies
        .insert("alpha/rust".into(), "missing-target".into());

    // Act
    let result =
        compose_organizations(&config, &[organization("alpha")], &LayoutAssignments::new());

    // Assert
    assert!(matches!(
        result,
        Err(ValidationError::UnknownTechnology { .. })
    ));
}

#[test]
fn canonical_technology_rejects_other_consumer_domain_affinities() {
    // Arrange
    let config = base();
    let mut organization = organization("alpha");
    organization.technologies[0].affinities = vec!["private-consumer-domain".into()];

    // Act
    let result = compose_organizations(&config, &[organization], &LayoutAssignments::new());

    // Assert
    assert!(matches!(result, Err(ValidationError::UnknownDomain { .. })));
}

#[test]
fn removing_all_organizations_rejects_remaining_shared_bindings() {
    // Arrange
    let mut config = base();
    let organization = organization("alpha");
    let mut shared = organization.technologies[0].clone();
    shared.id = "shared-rust".into();
    config.technologies.push(shared);
    config
        .shared_technologies
        .insert("alpha/rust".into(), "shared-rust".into());

    // Act
    let result = compose_organizations(&config, &[], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::InvalidValue(message)) if message.contains("require selected canonical organizations"))
    );
}

#[test]
fn third_review_imported_and_implicit_package_anchors_agree() {
    let config = base();
    let organizations = [organization("alpha")];
    let (composed, _) =
        compose_organizations(&config, &organizations, &LayoutAssignments::new()).unwrap();
    let group = &composed.publications[0];
    let mut implicit = group.packages[0].clone();
    implicit.anchor = None;

    let actual = package_anchor(group.anchor, &implicit, 0);

    assert_eq!(Some(actual), group.packages[0].anchor);
}
