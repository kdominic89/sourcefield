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
