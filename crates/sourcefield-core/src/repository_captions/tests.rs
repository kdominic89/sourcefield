use super::*;
use crate::{
    AccountSnapshot, DataStatus, NodeKind, ProfileVariant, RepositorySnapshot, SourceStatus,
    build_state, compose_organizations, parse_organization, validate_state,
};

/// Use the checked-in inventory so legacy identity and publication boundaries stay realistic.
fn config() -> Config {
    toml::from_str(include_str!("../../../../config/profile.toml")).unwrap()
}

fn policy(source: RepositoryCaptionSource) -> RepositoryCaptionConfig {
    RepositoryCaptionConfig {
        source,
        ..RepositoryCaptionConfig::default()
    }
}

fn configured(domain: &str, source: RepositoryCaptionSource) -> Config {
    let mut config = config();
    config
        .domains
        .iter_mut()
        .find(|item| item.id == domain)
        .unwrap()
        .repository_caption = Some(policy(source));

    config
}

fn observed(public: u32, private: Option<u32>) -> Snapshot {
    Snapshot {
        user: AccountSnapshot {
            login: "sample-user".into(),
            public_repositories: public,
            ..AccountSnapshot::default()
        },
        private_repository_count: private,
        ..Snapshot::default()
    }
}

fn source(snapshot: &mut Snapshot, key: &str, status: DataStatus) {
    snapshot.sources.push(SourceStatus {
        source: key.into(),
        status,
    });
}

fn caption(config: &Config, snapshot: &Snapshot, domain: &str) -> String {
    build_state(config, snapshot, "test")
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == format!("domain:{domain}"))
        .unwrap()
        .repository_caption
        .unwrap()
}

#[test]
fn omitted_policy_preserves_legacy_identity_and_serialized_shape() {
    // Arrange
    let config = config();
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../../config/offline-snapshot.json")).unwrap();

    // Act
    let state = build_state(&config, &snapshot, "1970-01-01T00:00:00Z").unwrap();
    let config_json = serde_json::to_value(&config).unwrap();
    let state_json = serde_json::to_value(&state).unwrap();

    // Assert
    assert_eq!(state.semantic_hash, "5B663867C0693D1D");
    assert!(
        config_json["domains"]
            .as_array()
            .unwrap()
            .iter()
            .all(|domain| domain.get("repository_caption").is_none())
    );
    assert!(
        state_json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node.get("repository_caption").is_none())
    );
}

#[test]
fn omitted_canonical_policy_is_not_serialized() {
    // Arrange
    let manifest =
        parse_organization(include_str!("../../../../examples/organization.toml")).unwrap();

    // Act
    let serialized = serde_json::to_value(&manifest).unwrap();

    // Assert
    assert!(serialized.get("repository_caption").is_none());
}

#[test]
fn omitted_policy_fields_use_documented_defaults() {
    // Arrange
    let input = "{}";

    // Act
    let parsed = serde_json::from_str::<RepositoryCaptionConfig>(input).unwrap();

    // Assert
    assert_eq!(parsed, RepositoryCaptionConfig::default());
    assert_eq!(parsed.source, RepositoryCaptionSource::SelectedProjects);
    assert_eq!(parsed.public_label, "public");
    assert_eq!(parsed.private_label, "private");
    assert_eq!(parsed.suffix, "repos");
    assert_eq!(parsed.unavailable, "repos unavailable");
}

#[test]
fn unknown_source_is_rejected_by_the_typed_parser() {
    // Arrange
    let input = r#"{"source":"all-projects"}"#;

    // Act
    let result = serde_json::from_str::<RepositoryCaptionConfig>(input);

    // Assert
    let diagnostic = result.unwrap_err().to_string();
    assert!(diagnostic.contains("unknown variant `all-projects`"));
    assert!(diagnostic.contains("selected-projects"));
    assert!(diagnostic.contains("owner-repositories"));
}

#[test]
fn unknown_policy_field_is_rejected() {
    // Arrange
    let input = r#"{"template":"{public}"}"#;

    // Act
    let result = serde_json::from_str::<RepositoryCaptionConfig>(input);

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("unknown field `template`")
    );
}

#[test]
fn selected_policy_counts_visible_authored_public_and_private_projects() {
    // Arrange
    let config = configured("sample-labs", RepositoryCaptionSource::SelectedProjects);
    let snapshot = observed(50, Some(80));

    // Act
    let result = caption(&config, &snapshot, "sample-labs");

    // Assert
    assert_eq!(result, "3 public / 1 private repos");
}

#[test]
fn selected_policy_excludes_hidden_projects_and_discovery_satellites() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::SelectedProjects);
    config.collection.visualize_discovered_repositories = true;
    config
        .projects
        .iter_mut()
        .find(|project| project.id == "mysql")
        .unwrap()
        .show_in_readme = false;
    config
        .projects
        .iter_mut()
        .find(|project| project.id == "relational-lab")
        .unwrap()
        .show_in_readme = false;
    let snapshot = Snapshot {
        repositories: vec![RepositorySnapshot {
            owner: "sample-labs".into(),
            name: "unselected".into(),
            full_name: "sample-labs/unselected".into(),
            url: "https://github.com/sample-labs/unselected".into(),
            ..RepositorySnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let state = build_state(&config, &snapshot, "test").unwrap();

    // Assert
    let domain = state
        .nodes
        .iter()
        .find(|node| node.id == "domain:sample-labs")
        .unwrap();
    assert_eq!(
        domain.repository_caption.as_deref(),
        Some("2 public / 0 private repos")
    );
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.id == "repository:sample-labs/unselected")
    );
    assert!(
        state
            .nodes
            .iter()
            .filter(|node| node.kind != NodeKind::Domain)
            .all(|node| node.repository_caption.is_none())
    );
}

#[test]
fn selected_policy_keeps_known_zero_counts() {
    // Arrange
    let mut config = configured("personal", RepositoryCaptionSource::SelectedProjects);
    config
        .projects
        .retain(|project| project.domain != "personal");

    // Act
    let result = caption(&config, &Snapshot::default(), "personal");

    // Assert
    assert_eq!(result, "0 public / 0 private repos");
}

#[test]
fn materialization_preserves_legacy_domain_summary() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::SelectedProjects);
    let policy = config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "sample-labs")
        .unwrap()
        .repository_caption
        .as_mut()
        .unwrap();
    policy.public_label = "open".into();
    policy.private_label = "restricted".into();
    policy.suffix = "projects".into();

    // Act
    let state = build_state(&config, &Snapshot::default(), "test").unwrap();

    // Assert
    let domain = state
        .nodes
        .iter()
        .find(|node| node.id == "domain:sample-labs")
        .unwrap();
    assert_eq!(domain.summary, "3 public / 1 private");
    assert_eq!(
        domain.repository_caption.as_deref(),
        Some("3 open / 1 restricted projects")
    );
}

#[test]
fn owner_policy_uses_authorized_private_observation_without_rechecking_config_opt_in() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshot = observed(12, Some(9));

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert!(!config.collection.collect_private_repository_count);
    assert_eq!(result, "12 public / 9 private repos");
}

#[test]
fn owner_policy_keeps_observed_zero_counts() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshot = observed(0, Some(0));

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "0 public / 0 private repos");
}

#[test]
fn missing_account_does_not_become_an_observed_zero() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);

    // Act
    let result = caption(&config, &Snapshot::default(), "personal");

    // Assert
    assert_eq!(result, "repos unavailable");
}

#[test]
fn unexpected_account_owner_cannot_reuse_primary_observations() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let mut snapshot = observed(999, Some(42));
    snapshot.user.login = "unselected-user".into();

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "repos unavailable");
}

#[test]
fn other_personal_domain_cannot_reuse_primary_observations() {
    // Arrange
    let mut config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "personal")
        .unwrap()
        .owner = "other-user".into();
    let snapshot = observed(12, Some(9));

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "repos unavailable");
}

#[test]
fn partial_public_source_does_not_expose_stale_public_count() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let mut snapshot = observed(12, None);
    source(&mut snapshot, "github:user", DataStatus::Partial);

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "repos unavailable");
}

#[test]
fn private_observation_survives_unavailable_public_source() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let mut snapshot = observed(12, Some(9));
    source(&mut snapshot, "github:user", DataStatus::Missing);
    source(&mut snapshot, "github:private-count", DataStatus::Live);

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "9 private repos");
}

#[test]
fn private_zero_survives_missing_public_account() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshot = Snapshot {
        private_repository_count: Some(0),
        ..Snapshot::default()
    };

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "0 private repos");
}

#[test]
fn unknown_private_count_is_not_rendered_as_zero() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshot = observed(12, None);

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "12 public repos");
}

#[test]
fn incomplete_private_sources_do_not_expose_stale_private_count() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Missing, DataStatus::Partial].map(|status| {
        let mut snapshot = observed(12, Some(9));
        source(&mut snapshot, "github:private-count", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "personal"));

    // Assert
    assert_eq!(results, ["12 public repos", "12 public repos"]);
}

#[test]
fn complete_public_sources_preserve_the_shared_availability_policy() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Live, DataStatus::Preview, DataStatus::Fallback].map(|status| {
        let mut snapshot = observed(12, None);
        source(&mut snapshot, "github:user", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "personal"));

    // Assert
    assert_eq!(
        results,
        ["12 public repos", "12 public repos", "12 public repos"]
    );
}

#[test]
fn owner_and_source_logins_match_without_case_sensitivity() {
    // Arrange
    let mut config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "personal")
        .unwrap()
        .owner = "SAMPLE-USER".into();
    let mut snapshot = observed(12, Some(9));
    snapshot.user.login = "Sample-User".into();
    source(&mut snapshot, "github:user", DataStatus::Live);

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "12 public / 9 private repos");
}

#[test]
fn organization_owner_uses_its_own_case_insensitive_account() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "sample-labs")
        .unwrap()
        .owner = "SAMPLE-LABS".into();
    let mut snapshot = observed(12, Some(9));
    snapshot.organizations.push(AccountSnapshot {
        login: "Sample-Labs".into(),
        public_repositories: 3,
        ..AccountSnapshot::default()
    });
    source(&mut snapshot, "github:org:sample-labs", DataStatus::Live);

    // Act
    let result = caption(&config, &snapshot, "sample-labs");

    // Assert
    assert_eq!(result, "3 public repos");
}

#[test]
fn multiple_organizations_never_reuse_their_aggregate_repository_total() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    let mut other = config
        .domains
        .iter()
        .find(|domain| domain.id == "sample-labs")
        .unwrap()
        .clone();
    other.id = "other-labs".into();
    other.owner = "other-labs".into();
    other.label = "Other Labs".into();
    other.anchor = [1200.0, 1500.0];
    config.domains.push(other);
    config
        .collection
        .github_organizations
        .push("other-labs".into());
    let snapshot = Snapshot {
        organizations: [("sample-labs", 3), ("other-labs", 7)]
            .map(|(login, public)| AccountSnapshot {
                login: login.into(),
                public_repositories: public,
                ..AccountSnapshot::default()
            })
            .into(),
        ..Snapshot::default()
    };

    // Act
    let state = build_state(&config, &snapshot, "test").unwrap();

    // Assert
    assert_eq!(state.stats.organization_public_repositories, Some(10));
    let primary = state
        .nodes
        .iter()
        .find(|node| node.id == "domain:sample-labs")
        .unwrap();
    let other = state
        .nodes
        .iter()
        .find(|node| node.id == "domain:other-labs")
        .unwrap();
    assert_eq!(
        primary.repository_caption.as_deref(),
        Some("3 public repos")
    );
    assert_eq!(other.repository_caption.as_deref(), Some("7 public repos"));
}

#[test]
fn organization_without_its_exact_observation_remains_unknown() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    config
        .collection
        .github_organizations
        .push("other-labs".into());
    let snapshot = Snapshot {
        organizations: vec![AccountSnapshot {
            login: "other-labs".into(),
            public_repositories: 7,
            ..AccountSnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let result = caption(&config, &snapshot, "sample-labs");

    // Assert
    assert_eq!(result, "repos unavailable");
}

#[test]
fn incomplete_organization_observation_does_not_expose_stale_count() {
    // Arrange
    let config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Missing, DataStatus::Partial].map(|status| {
        let mut snapshot = Snapshot {
            organizations: vec![AccountSnapshot {
                login: "sample-labs".into(),
                public_repositories: 3,
                ..AccountSnapshot::default()
            }],
            ..Snapshot::default()
        };
        source(&mut snapshot, "github:org:sample-labs", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "sample-labs"));

    // Assert
    assert_eq!(results, ["repos unavailable", "repos unavailable"]);
}

#[test]
fn organization_profile_never_materializes_a_personal_private_aggregate() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    config.profile.variant = ProfileVariant::Organization;
    config.profile.username = "sample-labs".into();
    config.collection.github_user.clear();
    config.domains.retain(|domain| domain.id == "sample-labs");
    config
        .projects
        .retain(|project| project.domain == "sample-labs");
    for technology in &mut config.technologies {
        technology
            .affinities
            .retain(|domain| domain == "sample-labs");
    }

    let mut snapshot = observed(12, Some(9));
    snapshot.organizations.push(AccountSnapshot {
        login: "sample-labs".into(),
        public_repositories: 3,
        ..AccountSnapshot::default()
    });

    // Act
    let state = build_state(&config, &snapshot, "test").unwrap();

    // Assert
    assert_eq!(state.stats.private_repository_count, None);
    let domain = state
        .nodes
        .iter()
        .find(|node| node.id == "domain:sample-labs")
        .unwrap();
    assert_eq!(domain.repository_caption.as_deref(), Some("3 public repos"));
}

#[test]
fn canonical_composition_copies_the_policy_to_the_owned_domain() {
    // Arrange
    let mut base = config();
    base.domains.clear();
    base.projects.clear();
    base.publications.clear();
    base.technologies.clear();
    let mut manifest =
        parse_organization(include_str!("../../../../examples/organization.toml")).unwrap();
    manifest.repository_caption = Some(RepositoryCaptionConfig {
        public_label: "open".into(),
        private_label: "restricted".into(),
        suffix: "projects".into(),
        ..RepositoryCaptionConfig::default()
    });

    // Act
    let (composed, _) =
        compose_organizations(&base, &[manifest.clone()], &Default::default()).unwrap();

    // Assert
    let domain = composed
        .domains
        .iter()
        .find(|domain| domain.id == manifest.id)
        .unwrap();
    assert_eq!(domain.repository_caption, manifest.repository_caption);
}

#[test]
fn canonical_parser_rejects_invalid_caption_text() {
    // Arrange
    let input = format!(
        "{}\n[repository_caption]\npublic_label = \" leading\"\n",
        include_str!("../../../../examples/organization.toml")
    );

    // Act
    let result = parse_organization(&input);

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("repository caption public_label")
    );
}

#[test]
fn direct_canonical_composition_cannot_bypass_caption_admission() {
    // Arrange
    let mut base = config();
    base.domains.clear();
    base.projects.clear();
    base.publications.clear();
    base.technologies.clear();
    let mut manifest =
        parse_organization(include_str!("../../../../examples/organization.toml")).unwrap();
    manifest.repository_caption = Some(RepositoryCaptionConfig {
        suffix: "bad\nline".into(),
        ..RepositoryCaptionConfig::default()
    });

    // Act
    let result = compose_organizations(&base, &[manifest], &Default::default());

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("repository caption suffix")
    );
}

#[test]
fn authored_validation_rejects_invalid_caption_policy() {
    // Arrange
    let input = include_str!("../../../../config/profile.toml").replace(
        "summary = \"Personal projects.\"",
        "summary = \"Personal projects.\"\nrepository_caption = { private_label = \"bad\\nline\" }",
    );

    let config: Config = toml::from_str(&input).unwrap();

    // Act
    let result = crate::validate::validate_authored_config(&config);

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("repository caption private_label")
    );
}

#[test]
fn label_and_suffix_boundaries_accept_24_unicode_scalars() {
    // Arrange
    let config = RepositoryCaptionConfig {
        public_label: "\u{00e9}".repeat(24),
        private_label: "\u{65e5}".repeat(24),
        suffix: "\u{03bb}".repeat(24),
        unavailable: "\u{00e9}".repeat(64),
        ..RepositoryCaptionConfig::default()
    };

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn labels_reject_over_budget_values() {
    // Arrange
    let configurations = ["public_label", "private_label", "suffix", "unavailable"].map(|field| {
        let mut config = RepositoryCaptionConfig::default();
        match field {
            "public_label" => config.public_label = "x".repeat(25),
            "private_label" => config.private_label = "x".repeat(25),
            "suffix" => config.suffix = "x".repeat(25),
            _ => config.unavailable = "x".repeat(65),
        }

        (config, field)
    });

    // Act
    let diagnostics = configurations
        .map(|(config, field)| (validate_config(&config).unwrap_err().to_string(), field));

    // Assert
    for (diagnostic, field) in diagnostics {
        assert!(diagnostic.contains(&format!("repository caption {field}")));
    }
}

#[test]
fn multiline_controls_xml_and_surrounding_whitespace_are_rejected() {
    // Arrange
    let values = [
        "two\nlines",
        "two\rlines",
        "two\twords",
        "a\u{2028}b",
        "a\u{2029}b",
        "a\0b",
        "\u{fffe}",
        "\u{ffff}",
        " leading",
        "trailing ",
        "",
    ];

    // Act
    let diagnostics = values.map(|text| {
        let config = RepositoryCaptionConfig {
            public_label: text.into(),
            ..RepositoryCaptionConfig::default()
        };

        validate_config(&config).unwrap_err().to_string()
    });

    // Assert
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.contains("repository caption public_label"))
    );
}

#[test]
fn empty_unavailable_caption_is_rejected_with_nonempty_diagnosis() {
    // Arrange
    let config = RepositoryCaptionConfig {
        unavailable: String::new(),
        ..RepositoryCaptionConfig::default()
    };

    // Act
    let diagnostic = validate_config(&config).unwrap_err().to_string();

    // Assert
    assert!(diagnostic.contains("repository caption unavailable must be nonempty, trimmed"));
}

#[test]
fn suffix_can_be_empty_without_trailing_whitespace() {
    // Arrange
    let mut config = configured("sample-labs", RepositoryCaptionSource::SelectedProjects);
    config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "sample-labs")
        .unwrap()
        .repository_caption
        .as_mut()
        .unwrap()
        .suffix
        .clear();

    // Act
    let result = caption(&config, &Snapshot::default(), "sample-labs");

    // Assert
    assert_eq!(result, "3 public / 1 private");
}

#[test]
fn plain_text_xml_delimiters_are_preserved_for_contextual_escaping() {
    // Arrange
    let mut config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let policy = config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "personal")
        .unwrap()
        .repository_caption
        .as_mut()
        .unwrap();
    policy.public_label = "<open> & shared".into();
    policy.suffix.clear();
    let snapshot = observed(12, None);

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result, "12 <open> & shared");
}

#[test]
fn maximum_numeric_counts_and_unicode_labels_fit_the_materialized_budget() {
    // Arrange
    let mut config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let policy = config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "personal")
        .unwrap()
        .repository_caption
        .as_mut()
        .unwrap();
    policy.public_label = "\u{00e9}".repeat(24);
    policy.private_label = "\u{65e5}".repeat(24);
    policy.suffix = "\u{03bb}".repeat(24);
    let snapshot = observed(u32::MAX, Some(u32::MAX));

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(result.chars().count(), 98);
    assert!(result.starts_with("4294967295 "));
    assert!(result.contains(" / 4294967295 "));
}

#[test]
fn standalone_state_rejects_caption_on_a_non_domain_node() {
    // Arrange
    let mut state = build_state(&config(), &Snapshot::default(), "test").unwrap();
    state
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Project)
        .unwrap()
        .repository_caption = Some("1 public repos".into());

    // Act
    let result = validate_state(&state);

    // Assert
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("repository captions are supported only on domain nodes")
    );
}

#[test]
fn standalone_state_rejects_invalid_materialized_caption() {
    // Arrange
    let values = [
        "x".repeat(129),
        "two\nlines".into(),
        " two".into(),
        "".into(),
    ];
    let states = values.map(|value| {
        let mut state = build_state(&config(), &Snapshot::default(), "test").unwrap();
        state
            .nodes
            .iter_mut()
            .find(|node| node.kind == NodeKind::Domain)
            .unwrap()
            .repository_caption = Some(value);

        state
    });

    // Act
    let diagnostics = states.map(|state| validate_state(&state).unwrap_err().to_string());

    // Assert
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.contains("repository caption materialized"))
    );
}

#[test]
fn standalone_state_accepts_materialized_boundary_and_round_trip() {
    // Arrange
    let mut state = build_state(&config(), &Snapshot::default(), "test").unwrap();
    state
        .nodes
        .iter_mut()
        .find(|node| node.kind == NodeKind::Domain)
        .unwrap()
        .repository_caption = Some("\u{00e9}".repeat(128));

    // Act
    let restored: crate::ProfileState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    let result = validate_state(&restored);

    // Assert
    assert!(result.is_ok());
    assert_eq!(
        restored
            .nodes
            .iter()
            .find_map(|node| node.repository_caption.as_ref())
            .unwrap()
            .chars()
            .count(),
        128
    );
}

#[test]
fn unavailable_legacy_organization_source_does_not_expose_stale_count() {
    // Arrange
    let config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    let mut snapshot = Snapshot {
        organizations: vec![AccountSnapshot {
            login: "sample-labs".into(),
            public_repositories: 3,
            ..AccountSnapshot::default()
        }],
        ..Snapshot::default()
    };
    source(
        &mut snapshot,
        "github:organization:sample-labs",
        DataStatus::Missing,
    );

    // Act
    let result = caption(&config, &snapshot, "sample-labs");

    // Assert
    assert_eq!(result, "repos unavailable");
}

#[test]
fn mixed_organization_source_aliases_keep_an_explicit_failure_unknown() {
    // Arrange
    let config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Missing, DataStatus::Partial].map(|status| {
        let mut snapshot = Snapshot {
            organizations: vec![AccountSnapshot {
                login: "sample-labs".into(),
                public_repositories: 3,
                ..AccountSnapshot::default()
            }],
            ..Snapshot::default()
        };
        source(&mut snapshot, "github:org:sample-labs", DataStatus::Live);
        source(&mut snapshot, "github:organization:sample-labs", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "sample-labs"));

    // Assert
    assert_eq!(results, ["repos unavailable", "repos unavailable"]);
}

#[test]
fn organization_owner_keeps_an_observed_zero_inventory() {
    // Arrange
    let config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    let snapshot = Snapshot {
        organizations: vec![AccountSnapshot {
            login: "sample-labs".into(),
            public_repositories: 0,
            ..AccountSnapshot::default()
        }],
        ..Snapshot::default()
    };

    // Act
    let result = caption(&config, &snapshot, "sample-labs");

    // Assert
    assert_eq!(result, "0 public repos");
}

#[test]
fn configured_primary_collection_owner_takes_precedence_over_profile_handle() {
    // Arrange
    let mut config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    config.collection.github_user = "other-user".into();
    config
        .domains
        .iter_mut()
        .find(|domain| domain.id == "personal")
        .unwrap()
        .owner = "other-user".into();
    let mut snapshot = observed(2, Some(1));
    snapshot.user.login = "other-user".into();

    // Act
    let result = caption(&config, &snapshot, "personal");

    // Assert
    assert_eq!(config.profile.username, "sample-user");
    assert_eq!(result, "2 public / 1 private repos");
}

#[test]
fn duplicate_organization_source_keys_do_not_hide_a_later_failure() {
    // Arrange
    let config = configured("sample-labs", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Missing, DataStatus::Partial].map(|status| {
        let mut snapshot = Snapshot {
            organizations: vec![AccountSnapshot {
                login: "sample-labs".into(),
                public_repositories: 3,
                ..AccountSnapshot::default()
            }],
            ..Snapshot::default()
        };
        source(&mut snapshot, "github:org:sample-labs", DataStatus::Live);
        source(&mut snapshot, "github:org:sample-labs", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "sample-labs"));

    // Assert
    assert_eq!(results, ["repos unavailable", "repos unavailable"]);
}

#[test]
fn duplicate_public_source_keys_do_not_hide_a_later_failure() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Missing, DataStatus::Partial].map(|status| {
        let mut snapshot = observed(12, None);
        source(&mut snapshot, "github:user", DataStatus::Live);
        source(&mut snapshot, "github:user", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "personal"));

    // Assert
    assert_eq!(results, ["repos unavailable", "repos unavailable"]);
}

#[test]
fn duplicate_private_source_keys_do_not_hide_a_later_failure() {
    // Arrange
    let config = configured("personal", RepositoryCaptionSource::OwnerRepositories);
    let snapshots = [DataStatus::Missing, DataStatus::Partial].map(|status| {
        let mut snapshot = observed(12, Some(9));
        source(&mut snapshot, "github:private-count", DataStatus::Live);
        source(&mut snapshot, "github:private-count", status);

        snapshot
    });

    // Act
    let results = snapshots.map(|snapshot| caption(&config, &snapshot, "personal"));

    // Assert
    assert_eq!(results, ["12 public repos", "12 public repos"]);
}
