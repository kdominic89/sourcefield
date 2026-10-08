//! Regression contracts for consumer-owned affinities, maintainer attribution and project tables.

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use sourcefield_core::{
    Config, ConfigError, EdgeKind, LayoutAssignments, MaintainerConfig, OrganizationImport,
    OrganizationManifest, OrganizationSource, ProfileVariant, Snapshot, ValidationError,
    Visibility, build_state, compose_organizations, load_config, parse_organization,
    render_project_readme, validate_config, validate_state,
};

/// Load the shared repository's synthetic profile without importing private consumer content.
fn config() -> Config {
    toml::from_str(include_str!("../../../config/profile.toml")).unwrap()
}

/// Load one synthetic canonical organization with its approved public maintainer.
fn organization() -> OrganizationManifest {
    toml::from_str(include_str!("../../../examples/organization.toml")).unwrap()
}

/// Keep only consumer-owned facts and explicitly select a future canonical domain.
fn imported_config() -> Config {
    let mut config = config();
    config.domains.retain(|domain| domain.id == "personal");
    config.projects.clear();
    config.publications.clear();
    config
        .technologies
        .retain(|technology| technology.id == "rust");
    config.technologies[0].affinities = vec!["personal".into(), "example-labs".into()];
    config.technologies[0].cross_domain = true;
    config.collection.github_organizations.clear();
    config.imports.push(OrganizationImport {
        id: "example-labs".into(),
        source: OrganizationSource::Local {
            path: "organization.toml".into(),
        },
    });

    config
}

/// Supply a stable synthetic identity while varying only the role under test.
fn maintainer(role: &str) -> MaintainerConfig {
    MaintainerConfig {
        username: "example-admin".into(),
        role: role.into(),
        url: "https://github.com/example-admin".into(),
    }
}

/// Own a real authored TOML input so load_config exercises the filesystem admission boundary.
struct InputFile {
    directory: PathBuf,
    path: PathBuf,
}

impl InputFile {
    /// Create a process-unique input without sharing files between concurrently running tests.
    fn new(config: &Config) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "sourcefield-core-parity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir(&directory).unwrap();
        let path = directory.join("profile.toml");
        fs::write(&path, toml::to_string(config).unwrap()).unwrap();

        Self { directory, path }
    }
}

impl Drop for InputFile {
    fn drop(&mut self) {
        // Cleanup must not replace the original assertion diagnostic during unwinding.
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn authored_selected_import_affinity_survives_loading_composition_and_graph() {
    // Arrange
    let config = imported_config();
    let input = InputFile::new(&config);
    let manifest = organization();

    // Act
    let loaded = load_config(&input.path).expect("selected imported affinity should load");
    let (composed, _) =
        compose_organizations(&loaded, &[manifest], &LayoutAssignments::new()).unwrap();
    let state = build_state(&composed, &Snapshot::default(), "same-time").unwrap();

    // Assert
    assert_eq!(
        loaded.technologies[0].affinities,
        ["personal", "example-labs"]
    );
    assert!(state.edges.iter().any(|edge| {
        edge.from == "domain:example-labs"
            && edge.to == "technology:rust"
            && edge.kind == EdgeKind::SharedCapability
    }));
    assert_eq!(
        fs::read_to_string(&input.path).unwrap(),
        toml::to_string(&config).unwrap()
    );
}

#[test]
fn authored_unselected_domain_affinity_is_rejected() {
    // Arrange
    let mut config = imported_config();
    config.technologies[0].affinities = vec!["personal".into(), "unselected-labs".into()];
    let input = InputFile::new(&config);

    // Act
    let result = load_config(&input.path);

    // Assert
    assert!(matches!(
        result,
        Err(ConfigError::Validation(ValidationError::UnknownDomain { domain, .. }))
            if domain == "unselected-labs"
    ));
}

#[test]
fn resolved_validation_requires_the_actual_imported_domain() {
    // Arrange
    let config = imported_config();

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(
        matches!(result, Err(ValidationError::UnknownDomain { domain, .. })
        if domain == "example-labs")
    );
}

#[test]
fn composition_without_the_selected_manifest_rejects_its_affinity() {
    // Arrange
    let config = imported_config();

    // Act
    let result = compose_organizations(&config, &[], &LayoutAssignments::new());

    // Assert
    assert!(
        matches!(result, Err(ValidationError::UnknownDomain { domain, .. })
        if domain == "example-labs")
    );
}

#[test]
fn authored_project_cannot_claim_an_imported_domain() {
    // Arrange
    let mut config = imported_config();
    let mut project = self::config().projects.remove(0);
    project.domain = "example-labs".into();
    config.projects.push(project);
    let input = InputFile::new(&config);

    // Act
    let result = load_config(&input.path);

    // Assert
    assert!(matches!(
        result,
        Err(ConfigError::Validation(ValidationError::UnknownDomain { domain, .. }))
            if domain == "example-labs"
    ));
}

#[test]
fn authored_publication_cannot_claim_an_imported_domain() {
    // Arrange
    let mut config = imported_config();
    let mut publication = self::config().publications.remove(0);
    publication.domain = "example-labs".into();
    config.publications.push(publication);
    let input = InputFile::new(&config);

    // Act
    let result = load_config(&input.path);

    // Assert
    assert!(matches!(
        result,
        Err(ConfigError::Validation(ValidationError::UnknownDomain { domain, .. }))
            if domain == "example-labs"
    ));
}

#[test]
fn profile_maintainer_requires_a_nonempty_role() {
    // Arrange
    let mut config = config();
    config.profile.maintainer = Some(maintainer(""));
    let input = InputFile::new(&config);

    // Act
    let result = load_config(&input.path);

    // Assert
    assert!(
        matches!(result, Err(ConfigError::Validation(ValidationError::InvalidValue(value)))
        if value == "maintainer role")
    );
}

#[test]
fn profile_maintainer_rejects_whitespace_only_role() {
    // Arrange
    let mut config = config();
    config.profile.maintainer = Some(maintainer(" \t\r\n "));

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(matches!(result, Err(ValidationError::InvalidValue(value))
        if value == "maintainer role"));
}

#[test]
fn domain_maintainer_requires_a_nonempty_role() {
    // Arrange
    let mut config = config();
    config.domains[0].maintainer = Some(maintainer(""));

    // Act
    let result = validate_config(&config);

    // Assert
    assert!(matches!(result, Err(ValidationError::InvalidValue(value))
        if value == "maintainer role"));
}

#[test]
fn state_profile_maintainer_rejects_whitespace_only_role() {
    // Arrange
    let mut state = build_state(&config(), &Snapshot::default(), "same-time").unwrap();
    state.profile.maintainer = Some(maintainer(" \t "));

    // Act
    let result = validate_state(&state);

    // Assert
    assert!(matches!(result, Err(ValidationError::InvalidValue(value))
        if value == "maintainer role"));
}

#[test]
fn state_organization_maintainer_requires_a_nonempty_role() {
    // Arrange
    let mut state = build_state(&config(), &Snapshot::default(), "same-time").unwrap();
    state.organizations[0].maintainer = Some(maintainer(""));

    // Act
    let result = validate_state(&state);

    // Assert
    assert!(matches!(result, Err(ValidationError::InvalidValue(value))
        if value == "maintainer role"));
}

#[test]
fn canonical_maintainer_requires_a_nonempty_role_during_parse() {
    // Arrange
    let mut manifest = organization();
    manifest.maintainer = Some(maintainer(""));
    let source = toml::to_string(&manifest).unwrap();

    // Act
    let result = parse_organization(&source);

    // Assert
    assert!(
        matches!(result, Err(ConfigError::Validation(ValidationError::InvalidValue(value)))
        if value == "maintainer role")
    );
}

#[test]
fn canonical_maintainer_rejects_whitespace_only_role_during_composition() {
    // Arrange
    let mut config = imported_config();
    config.technologies[0].affinities = vec!["personal".into()];
    let mut manifest = organization();
    manifest.maintainer = Some(maintainer(" \t "));

    // Act
    let result = compose_organizations(&config, &[manifest], &LayoutAssignments::new());

    // Assert
    assert!(matches!(result, Err(ValidationError::InvalidValue(value))
        if value == "maintainer role"));
}

#[test]
fn absent_maintainers_remain_optional() {
    // Arrange
    let mut config = imported_config();
    config.technologies[0].affinities = vec!["personal".into()];
    let mut manifest = organization();
    manifest.maintainer = None;

    // Act
    let (composed, _) =
        compose_organizations(&config, &[manifest], &LayoutAssignments::new()).unwrap();
    let state = build_state(&composed, &Snapshot::default(), "same-time").unwrap();
    let result = validate_state(&state);

    // Assert
    assert!(result.is_ok());
    assert!(state.profile.maintainer.is_none());
    assert!(
        state
            .organizations
            .iter()
            .all(|identity| identity.maintainer.is_none())
    );
}

#[test]
fn valid_maintainer_role_is_preserved_through_composition_and_state() {
    // Arrange
    let mut config = imported_config();
    config.profile.variant = ProfileVariant::Organization;
    config.profile.username = "example-labs".into();
    config.profile.organization = "example-labs".into();
    config.collection.github_user.clear();
    config.domains.clear();
    config.technologies[0].affinities.clear();
    let mut manifest = organization();
    manifest.maintainer = Some(maintainer("Administrator and core maintainer"));

    // Act
    let (composed, _) =
        compose_organizations(&config, &[manifest], &LayoutAssignments::new()).unwrap();
    let state = build_state(&composed, &Snapshot::default(), "same-time").unwrap();
    let result = validate_state(&state);

    // Assert
    assert!(result.is_ok());
    assert_eq!(
        state.organizations[0].maintainer.as_ref().unwrap().role,
        "Administrator and core maintainer"
    );
    assert_eq!(
        state.profile.maintainer.as_ref().unwrap().role,
        "Administrator and core maintainer"
    );
}

#[test]
fn managed_project_table_preserves_public_links_private_labels_and_stacks() {
    // Arrange
    let mut config = config();
    let mut private = config.projects[0].clone();
    private.label = "Private tooling".into();
    private.summary = "Approved abstract description".into();
    private.repository = None;
    private.display_stack = vec!["Rust".into(), "WebAssembly".into()];
    let mut public = private.clone();
    public.label = "Public tooling".into();
    public.repository = Some("sample-user/tooling".into());
    public.visibility = Visibility::Public;
    config.projects = vec![private, public];

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert_eq!(
        readme,
        concat!(
            "<!-- sourcefield:projects:start -->\n### Projects\n\n",
            "| Project | What it does | Stack |\n| --- | --- | --- |\n",
            "| Private tooling | Approved abstract description | Rust / WebAssembly |\n",
            "| [Public tooling](https://github.com/sample-user/tooling) | ",
            "Approved abstract description | Rust / WebAssembly |\n",
            "<!-- sourcefield:projects:end -->"
        )
    );
}

#[test]
fn managed_project_table_uses_authored_domain_and_project_order() {
    // Arrange
    let mut config = config();
    let mut first = config.projects[0].clone();
    first.label = "Zeta personal".into();
    let mut second = first.clone();
    second.label = "Alpha personal".into();
    let mut third = first.clone();
    third.label = "Zeta organization".into();
    third.domain = "sample-labs".into();
    let mut fourth = third.clone();
    fourth.label = "Alpha organization".into();
    config.projects = vec![third, first, fourth, second];

    // Act
    let readme = render_project_readme(&config);

    // Assert
    let labels = readme
        .lines()
        .filter(|line| line.starts_with("| "))
        .skip(2)
        .map(|line| line.split('|').nth(1).unwrap().trim())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "Zeta personal",
            "Alpha personal",
            "Zeta organization",
            "Alpha organization"
        ]
    );
}

#[test]
fn managed_project_table_excludes_nonvisible_projects() {
    // Arrange
    let mut config = config();
    config.projects[0].label = "Hidden tooling".into();
    config.projects[0].show_in_readme = false;
    config.projects[1].label = "Visible tooling".into();
    config.projects[1].show_in_readme = true;

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert!(!readme.contains("Hidden tooling"));
    assert!(readme.contains("Visible tooling"));
}

#[test]
fn managed_project_table_without_projects_retains_only_managed_markers() {
    // Arrange
    let mut config = config();
    config.projects.clear();

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert_eq!(
        readme,
        "<!-- sourcefield:projects:start -->\n<!-- sourcefield:projects:end -->"
    );
}

#[test]
fn managed_project_table_with_only_hidden_projects_retains_only_managed_markers() {
    // Arrange
    let mut config = config();
    for project in &mut config.projects {
        project.show_in_readme = false;
    }

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert_eq!(
        readme,
        "<!-- sourcefield:projects:start -->\n<!-- sourcefield:projects:end -->"
    );
}

#[test]
fn managed_project_table_does_not_admit_projects_outside_declared_domains() {
    // Arrange
    let mut config = config();
    for project in &mut config.projects {
        project.domain = "unselected-labs".into();
    }

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert_eq!(
        readme,
        "<!-- sourcefield:projects:start -->\n<!-- sourcefield:projects:end -->"
    );
}

#[test]
fn managed_project_table_escapes_every_authored_cell() {
    // Arrange
    let mut config = config();
    let project = &mut config.projects[0];
    project.label = "Tool | [link]".into();
    project.summary = "<script>\nA | B & C".into();
    project.display_stack = vec!["Rust | C".into(), "<runtime>\nnext".into()];
    config.projects.truncate(1);

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert!(readme.contains(
        r"| Tool \| \[link\] | &lt;script&gt; A \| B &amp; C | Rust \| C / &lt;runtime&gt; next |"
    ));
    assert!(!readme.contains("<script>"));
    assert_eq!(
        readme.lines().filter(|line| line.starts_with("| ")).count(),
        3
    );
}

#[test]
fn canonical_project_order_is_preserved_without_changing_stable_slot_allocation() {
    // Arrange
    let mut config = imported_config();
    config.technologies[0].affinities = vec!["personal".into()];
    let mut manifest = organization();
    let mut zeta = manifest.projects[0].clone();
    zeta.id = "zeta".into();
    zeta.label = "Zeta".into();
    let mut alpha = zeta.clone();
    alpha.id = "alpha".into();
    alpha.label = "Alpha".into();
    manifest.projects = vec![zeta, alpha];
    manifest.publications.clear();
    let mut reordered = manifest.clone();
    reordered.projects.reverse();

    // Act
    let (composed, assignments) =
        compose_organizations(&config, &[manifest], &LayoutAssignments::new()).unwrap();
    let (_, reordered_assignments) =
        compose_organizations(&config, &[reordered], &LayoutAssignments::new()).unwrap();

    // Assert
    assert_eq!(
        composed
            .projects
            .iter()
            .map(|project| project.label.as_str())
            .collect::<Vec<_>>(),
        ["Zeta", "Alpha"]
    );
    assert_eq!(assignments, reordered_assignments);
}

#[test]
fn managed_project_table_encodes_destination_delimiters_when_validation_is_bypassed() {
    // Arrange
    let mut config = config();
    config.projects[0].repository = Some("sample-user/tool|ing)".into());
    config.projects.truncate(1);

    // Act
    let readme = render_project_readme(&config);

    // Assert
    assert!(readme.contains("](https://github.com/sample-user/tool%7Cing%29)"));
    assert!(!readme.contains("tool|ing)"));
}
