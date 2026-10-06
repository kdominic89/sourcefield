use crate::*;

fn config() -> Config {
    toml::from_str(include_str!("../../../config/profile.toml")).unwrap()
}

fn snapshot() -> Snapshot {
    serde_json::from_str(include_str!("../../../config/offline-snapshot.json")).unwrap()
}

#[test]
fn invalid_authored_package_id_is_rejected() {
    // Arrange
    let mut value = config();
    value.publications[0].packages[0].id = "bad](https://evil.test)".into();

    // Act
    let result = validate_config(&value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn package_url_cannot_change_markdown_destination_or_package_identity() {
    // Arrange
    let cases = [
        "https://www.nuget.org/packages/Other.Package/",
        "https://www.nuget.org/packages/Example/)[extra](https://evil.test)",
    ];

    // Act
    let results = cases.map(|url| {
        let mut value = config();
        value.publications[0].packages[0].url = url.into();
        validate_config(&value)
    });

    // Assert
    assert!(results.iter().all(Result::is_err));
}

#[test]
fn history_capacity_rejects_limit_plus_one() {
    // Arrange
    let mut value = config();
    value.collection.history_limit = 257;

    // Act
    let result = validate_config(&value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn xml_invalid_text_fails_before_rendering() {
    // Arrange
    let mut value = config();
    value.projects[0].summary = "invalid\u{1}text".into();

    // Act
    let result = validate_config(&value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn metric_changes_leave_technology_coordinates_stable() {
    // Arrange
    let value = config();
    let original = snapshot();
    let mut changed = original.clone();
    changed.user.followers += 1;
    let before = build_state(&value, &original, "same").unwrap();

    // Act
    let after = build_state(&value, &changed, "same").unwrap();

    // Assert
    assert_ne!(before.semantic_hash, after.semantic_hash);
    for node in before
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Technology)
    {
        let other = after
            .nodes
            .iter()
            .find(|other| other.id == node.id)
            .unwrap();
        assert_eq!((node.x, node.y), (other.x, other.y));
    }
}

#[test]
fn empty_organizations_reserve_distinct_regions() {
    // Arrange
    let mut value = config();
    value.domains.clear();
    value.projects.clear();
    value.publications.clear();
    value.technologies.clear();
    value.collection.github_organizations.clear();
    let mut first: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();
    first.projects.clear();
    first.publications.clear();
    first.technologies.clear();
    first.id = "first".into();
    let mut second = first.clone();
    second.id = "second".into();
    second.owner = "second-owner".into();

    // Act
    let result = compose_organizations(&value, &[first, second], &LayoutAssignments::new());

    // Assert
    let (composed, _) = result.unwrap();
    assert!(composed.domains[1].anchor[1] > composed.domains[0].anchor[1] + 150.0);
}

#[test]
fn history_capacity_accepts_exact_limit() {
    // Arrange
    let mut value = config();
    value.collection.history_limit = MAX_HISTORY_ENTRIES;

    // Act
    let result = validate_config(&value);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn package_id_grammar_accepts_valid_segments_and_rejects_separators() {
    // Arrange
    let cases = [
        ("Example.Name_1-Adapter", true),
        ("_", true),
        (".Start", false),
        ("End-", false),
        ("Two..Dots", false),
        ("Two.-Separators", false),
        ("<script>", false),
        ("Name]", false),
        ("", false),
    ];

    // Act
    let results = cases.map(|(id, expected)| (valid_package_id(id), expected));

    // Assert
    assert!(results.iter().all(|(actual, expected)| actual == expected));
}

#[test]
fn imported_invalid_package_is_rejected_during_composition() {
    // Arrange
    let mut value = config();
    value.domains.clear();
    value.projects.clear();
    value.publications.clear();
    value.technologies.clear();
    value.collection.github_organizations.clear();
    let mut manifest: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();
    manifest.publications[0].packages[0].id = "<!-- sourcefield:packages:end -->".into();

    // Act
    let result = compose_organizations(&value, &[manifest], &LayoutAssignments::new());

    // Assert
    assert!(result.is_err());
}

#[test]
fn package_markdown_escapes_delimiters_even_without_validation() {
    // Arrange
    let mut value = config();
    value.publications[0].label = "<!-- sourcefield:packages:end -->".into();
    value.publications[0].packages[0].id = "bad](https://evil.test)".into();
    value.publications[0].packages[0].url = "https://example.test/)[other](x)".into();

    // Act
    let markdown = render_package_readme_prepared(&value);

    // Assert
    assert_eq!(
        markdown
            .matches("<!-- sourcefield:packages:start -->")
            .count(),
        1
    );
    assert_eq!(
        markdown
            .matches("<!-- sourcefield:packages:end -->")
            .count(),
        1
    );
    assert!(!markdown.contains(")[other]("));
    assert!(markdown.contains("%29%5Bother%5D%28x%29"));
    assert!(markdown.contains("&lt;!-- sourcefield:packages:end --&gt;"));
}

#[test]
fn xml_valid_unicode_and_whitespace_remain_unchanged() {
    // Arrange
    let mut value = config();
    let text = "A\tB\nC\rD & <tag> \u{00b7} \u{1f680}";
    value.projects[0].summary = text.into();

    // Act
    let state = build_state(&value, &snapshot(), "test").unwrap();

    // Assert
    assert!(state.nodes.iter().any(|node| node.summary == text));
}

#[test]
fn invalid_xml_metadata_is_rejected_at_state_boundary() {
    // Arrange
    let mut state = build_state(&config(), &snapshot(), "test").unwrap();
    state.nodes[0]
        .details
        .push("invalid\u{ffff}metadata".into());

    // Act
    let result = validate_state(&state);

    // Assert
    assert!(result.is_err());
}

#[test]
fn unrelated_repository_growth_preserves_existing_satellite_positions() {
    // Arrange
    let mut value = config();
    value.collection.visualize_discovered_repositories = true;
    let owner = value.domains[0].owner.clone();
    let mut source = snapshot();
    source.repositories.push(RepositorySnapshot {
        owner: owner.clone(),
        name: "existing".into(),
        full_name: format!("{owner}/existing"),
        url: format!("https://github.com/{owner}/existing"),
        ..RepositorySnapshot::default()
    });
    let before = build_state(&value, &source, "same").unwrap();
    source.repositories.push(RepositorySnapshot {
        owner: owner.clone(),
        name: "aaa-unrelated".into(),
        full_name: format!("{owner}/aaa-unrelated"),
        url: format!("https://github.com/{owner}/aaa-unrelated"),
        ..RepositorySnapshot::default()
    });
    source.user.followers += 5;
    source
        .repositories
        .iter_mut()
        .for_each(|repository| repository.stars += 5);
    value.render.height += 200;

    // Act
    let after = build_state(&value, &source, "same").unwrap();

    // Assert
    let selected = before
        .nodes
        .iter()
        .filter(|node| {
            node.kind == NodeKind::Technology || node.id == format!("repository:{owner}/existing")
        })
        .collect::<Vec<_>>();
    assert!(
        selected
            .iter()
            .any(|node| node.id.starts_with("repository:"))
    );
    for node in selected {
        let other = after
            .nodes
            .iter()
            .find(|other| other.id == node.id)
            .unwrap();
        assert_eq!((node.x, node.y), (other.x, other.y), "{}", node.id);
    }
}

#[test]
fn package_id_length_boundary_is_explicit() {
    // Arrange
    let at_limit = "a".repeat(100);
    let over_limit = "a".repeat(101);

    // Act
    let actual = (valid_package_id(&at_limit), valid_package_id(&over_limit));

    // Assert
    assert_eq!(actual, (true, false));
}

/// Keep each regression focused on one component while retaining valid fixture references.
fn component_relation_config(integrates: &[&str], targets: &[&str]) -> (Config, String) {
    let mut value = config();
    let project = &mut value.projects[0];
    let component = &mut project.components[0];
    component.integrates = integrates.iter().map(|value| (*value).into()).collect();
    component.targets = targets.iter().map(|value| (*value).into()).collect();
    let component_id = format!("component:{}:{}", project.id, component.id);

    (value, component_id)
}

#[test]
fn component_relations_preserve_integrates_only() {
    // Arrange
    let (value, component_id) = component_relation_config(&["rust"], &[]);
    let source = snapshot();

    // Act
    let result = build_state(&value, &source, "test");

    // Assert
    let state = result.unwrap();
    let edges = state
        .edges
        .iter()
        .filter(|edge| edge.to == component_id && edge.from == "technology:rust")
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].kind, EdgeKind::Integrates);
    assert_eq!(edges[0].weight, 0.25);
    assert!(!edges[0].show_in_readme);
}

#[test]
fn component_relations_preserve_targets_only() {
    // Arrange
    let (value, component_id) = component_relation_config(&[], &["rust"]);
    let source = snapshot();

    // Act
    let result = build_state(&value, &source, "test");

    // Assert
    let state = result.unwrap();
    let edges = state
        .edges
        .iter()
        .filter(|edge| edge.to == component_id && edge.from == "technology:rust")
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].kind, EdgeKind::Targets);
    assert_eq!(edges[0].weight, 0.25);
    assert!(!edges[0].show_in_readme);
}

#[test]
fn component_relations_preserve_both_kinds_for_shared_endpoints() {
    // Arrange
    let (value, component_id) = component_relation_config(&["rust"], &["rust"]);
    let source = snapshot();

    // Act
    let result = build_state(&value, &source, "test");

    // Assert
    let state = result.unwrap();
    let edges = state
        .edges
        .iter()
        .filter(|edge| edge.to == component_id && edge.from == "technology:rust")
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    assert_eq!(
        edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Integrates)
            .count(),
        1
    );
    assert_eq!(
        edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Targets)
            .count(),
        1
    );
    assert!(
        edges
            .iter()
            .all(|edge| edge.weight == 0.25 && !edge.show_in_readme)
    );
}

#[test]
fn component_relations_reject_repeated_entries_within_each_kind() {
    // Arrange
    let cases = [
        (
            component_relation_config(&["rust", "csharp", "rust"], &[]).0,
            "integrates",
        ),
        (
            component_relation_config(&[], &["rust", "csharp", "rust"]).0,
            "targets",
        ),
    ];
    let source = snapshot();

    // Act
    let results = cases.map(|(value, relation)| (build_state(&value, &source, "test"), relation));

    // Assert
    for (result, relation) in results {
        let error = result.expect_err("one relation cannot repeat its technology endpoint");
        assert!(matches!(
            error,
            GraphError::Validation(ValidationError::InvalidValue(_))
        ));
        let message = error.to_string();
        assert!(message.contains(relation), "{message}");
        assert!(message.contains("rust"), "{message}");
    }
}

#[test]
fn project_relations_preserve_all_kinds_for_shared_endpoints() {
    // Arrange
    let mut value = config();
    let project = &mut value.projects[0];
    project.implemented_with = vec!["rust".into()];
    project.integrates = vec!["rust".into()];
    project.targets = vec!["rust".into()];
    let project_id = format!("project:{}", project.id);
    let source = snapshot();

    // Act
    let result = build_state(&value, &source, "test");

    // Assert
    let state = result.unwrap();
    let kinds = state
        .edges
        .iter()
        .filter(|edge| edge.to == project_id && edge.from == "technology:rust")
        .map(|edge| edge.kind)
        .collect::<Vec<_>>();
    assert_eq!(kinds.len(), 3);
    assert!(kinds.contains(&EdgeKind::ImplementedWith));
    assert!(kinds.contains(&EdgeKind::Integrates));
    assert!(kinds.contains(&EdgeKind::Targets));
}

#[test]
fn project_relations_reject_repeated_entries_within_each_kind() {
    // Arrange
    let cases = ["implemented_with", "integrates", "targets"].map(|relation| {
        let mut value = config();
        let project = &mut value.projects[0];
        let technologies = match relation {
            "implemented_with" => &mut project.implemented_with,
            "integrates" => &mut project.integrates,
            _ => &mut project.targets,
        };
        *technologies = vec!["rust".into(), "csharp".into(), "rust".into()];

        (value, relation)
    });
    let source = snapshot();

    // Act
    let results = cases.map(|(value, relation)| (build_state(&value, &source, "test"), relation));

    // Assert
    for (result, relation) in results {
        let error = result.expect_err("one relation cannot repeat its technology endpoint");
        assert!(matches!(
            error,
            GraphError::Validation(ValidationError::InvalidValue(_))
        ));
        let message = error.to_string();
        assert!(message.contains(relation), "{message}");
        assert!(message.contains("rust"), "{message}");
    }
}

#[test]
fn component_relations_still_reject_unknown_technologies_in_each_kind() {
    // Arrange
    let cases = [
        component_relation_config(&["missing-technology"], &[]).0,
        component_relation_config(&[], &["missing-technology"]).0,
    ];

    // Act
    let results = cases.map(|value| validate_config(&value));

    // Assert
    for result in results {
        assert!(matches!(
            result,
            Err(ValidationError::UnknownTechnology { technology, .. })
                if technology == "missing-technology"
        ));
    }
}

#[test]
fn interests_remain_in_graph_when_readme_visibility_is_disabled() {
    // Arrange
    let mut value = config();
    value.render.show_interests_in_readme = false;
    let expected_ids = value
        .interests
        .iter()
        .map(|interest| format!("interest:{}", interest.id))
        .collect::<Vec<_>>();
    let source = snapshot();

    // Act
    let result = build_state(&value, &source, "test");

    // Assert
    let state = result.unwrap();
    let interests = state
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Interest)
        .collect::<Vec<_>>();
    assert!(!expected_ids.is_empty());
    assert_eq!(interests.len(), expected_ids.len());
    assert!(
        expected_ids
            .iter()
            .all(|id| interests.iter().any(|node| &node.id == id))
    );
    assert!(interests.iter().all(|node| !node.show_in_readme));
}

#[test]
fn interests_respect_individual_visibility_when_readme_visibility_is_enabled() {
    // Arrange
    let mut value = config();
    value.render.show_interests_in_readme = true;
    value.interests[0].show_in_readme = false;
    value.interests[1].show_in_readme = true;
    let expected = value
        .interests
        .iter()
        .map(|interest| (format!("interest:{}", interest.id), interest.show_in_readme))
        .collect::<Vec<_>>();
    let source = snapshot();

    // Act
    let result = build_state(&value, &source, "test");

    // Assert
    let state = result.unwrap();
    for (id, visible) in expected {
        let node = state.nodes.iter().find(|node| node.id == id).unwrap();
        assert_eq!(node.show_in_readme, visible);
    }
}
