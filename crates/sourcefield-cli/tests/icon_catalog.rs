//! Verify icon generation, offline replay, and extraction through the public CLI.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use sourcefield_core::{
    Config, IconDefinition, OrganizationImport, OrganizationManifest, OrganizationSource,
    ProfileState, Snapshot,
};
use sourcefield_workspace::OwnershipManifest;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(config: &Config) -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "sourcefield-icons-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/profile.toml"),
            toml::to_string(config).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("config/offline-snapshot.json"),
            serde_json::to_vec(&Snapshot::default()).unwrap(),
        )
        .unwrap();

        Self(root)
    }

    fn command(&self, root: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sourcefield"))
            .arg("--root")
            .arg(root)
            .args(args)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("PROFILE_TOKEN")
            .env_remove("SOURCEFIELD_PRIVATE_COUNTS")
            .output()
            .unwrap()
    }

    fn generate(&self) -> Output {
        self.command(&self.0, &["generate", "--offline", "--no-history"])
    }

    fn replay(&self) -> Output {
        self.command(
            &self.0,
            &["generate", "--offline", "--no-history", "--locked"],
        )
    }

    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        let manifest: OwnershipManifest =
            serde_json::from_slice(&fs::read(self.0.join(".sourcefield-owned.json")).unwrap())
                .unwrap();

        manifest
            .files
            .keys()
            .map(|path| (path.clone(), fs::read(self.0.join(path)).unwrap()))
            .collect()
    }

    fn state(&self, root: &Path) -> ProfileState {
        serde_json::from_slice(&fs::read(root.join("assets/profile-state.json")).unwrap()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup must not replace an assertion failure with a second panic during unwinding.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn icon() -> IconDefinition {
    serde_json::from_value(serde_json::json!({
        "radius": 16,
        "elements": [{"geometry": {"shape": "circle", "center": [0, 0], "radius": 4}}]
    }))
    .unwrap()
}

fn config() -> Config {
    toml::from_str(include_str!("../../../config/profile.toml")).unwrap()
}

fn icons_config() -> Config {
    let mut config = config();
    config.icons.insert("signal".into(), icon());
    config.projects[0].icon = Some("signal".into());
    config.projects[1].icon = Some("builtin:sourcefield".into());
    config.projects[2].icon = Some("builtin:database-safe".into());

    config
}

fn organization(id: &str) -> OrganizationManifest {
    let mut organization: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();
    organization.id = id.into();
    organization.publications.clear();
    organization.icons.insert("signal".into(), icon());
    organization.projects[0].icon = Some("signal".into());

    organization
}

#[test]
fn fixture_cleanup_removes_directory() {
    // Arrange
    let fixture = Fixture::new(&config());
    let path = fixture.0.clone();

    // Act
    drop(fixture);

    // Assert
    assert!(!path.exists());
}

#[test]
fn fixture_cleanup_does_not_panic_when_directory_is_missing() {
    // Arrange
    let fixture = Fixture::new(&config());
    fs::remove_dir_all(&fixture.0).unwrap();

    // Act
    let result = std::panic::catch_unwind(|| drop(fixture));

    // Assert
    assert!(result.is_ok(), "fixture cleanup must not introduce a panic");
}

#[test]
fn custom_and_builtin_generation_replays_exactly() {
    // Arrange
    let fixture = Fixture::new(&icons_config());
    success(&fixture.generate());
    let before = fixture.published();

    // Act
    let replay = fixture.replay();
    let state = fixture.state(&fixture.0);

    // Assert
    success(&replay);
    assert_eq!(fixture.published(), before);
    assert_eq!(state.icons.len(), 1);
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.icon.as_deref() == Some("signal"))
    );
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.icon.as_deref() == Some("builtin:sourcefield"))
    );
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.icon.as_deref() == Some("builtin:database-safe"))
    );
}

#[test]
fn replay_rejects_tampered_resolved_icon_catalog_without_publication() {
    // Arrange
    let fixture = Fixture::new(&icons_config());
    success(&fixture.generate());
    let path = fixture.0.join("assets/resolved-config.json");
    let mut resolved: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    resolved["icons"]["signal"]["radius"] = 17.into();
    fs::write(path, serde_json::to_vec(&resolved).unwrap()).unwrap();
    let before = fixture.published();

    // Act
    let replay = fixture.replay();

    // Assert
    assert!(!replay.status.success());
    assert!(
        String::from_utf8_lossy(&replay.stderr)
            .contains("generation provenance input digest mismatch: resolved-config.json")
    );
    assert_eq!(fixture.published(), before);
}

#[test]
fn replay_rejects_changed_authored_icon_reference_without_publication() {
    // Arrange
    let mut config = icons_config();
    let fixture = Fixture::new(&config);
    success(&fixture.generate());
    config.projects[0].icon = Some("builtin:sourcefield".into());
    fs::write(
        fixture.0.join("config/profile.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let before = fixture.published();

    // Act
    let replay = fixture.replay();

    // Assert
    assert!(!replay.status.success());
    assert!(String::from_utf8_lossy(&replay.stderr).contains("authored inputs changed"));
    assert_eq!(fixture.published(), before);
}

#[test]
fn generation_rejects_unknown_icon_before_publication() {
    // Arrange
    let mut config = icons_config();
    config.projects[0].icon = Some("absent".into());
    let fixture = Fixture::new(&config);

    // Act
    let generated = fixture.generate();

    // Assert
    assert!(!generated.status.success());
    assert!(!fixture.0.join("assets/profile-state.json").exists());
    assert!(!fixture.0.join(".sourcefield-owned.json").exists());
}

#[test]
fn local_import_updates_remain_live_after_extracting_another_organization() {
    // Arrange
    let mut config = config();
    config.domains.clear();
    config.projects.clear();
    config.technologies.clear();
    config.publications.clear();
    config.collection.github_organizations.clear();
    config.imports = ["alpha", "beta"]
        .map(|id| OrganizationImport {
            id: id.into(),
            source: OrganizationSource::Local {
                path: format!("{id}.toml"),
            },
        })
        .into();
    let fixture = Fixture::new(&config);

    for id in ["alpha", "beta"] {
        fs::write(
            fixture.0.join(format!("config/{id}.toml")),
            toml::to_string(&organization(id)).unwrap(),
        )
        .unwrap();
    }

    success(&fixture.generate());
    let expected = fixture.state(&fixture.0);

    // Act
    let extracted = fixture.command(
        &fixture.0,
        &[
            "extract-organization",
            "--config",
            "config/profile.toml",
            "--organization",
            "alpha",
            "--destination",
            "extracted",
        ],
    );
    success(&extracted);
    let regenerated = fixture.command(
        &fixture.0.join("extracted"),
        &[
            "generate",
            "--offline",
            "--no-history",
            "--config",
            "profile.toml",
            "--fallback-snapshot",
            "../config/offline-snapshot.json",
        ],
    );
    success(&regenerated);
    let actual = fixture.state(&fixture.0.join("extracted"));
    let mut changed = organization("beta");
    changed.icons.get_mut("signal").unwrap().radius = 18.0;
    changed.projects[0].label = "Updated Beta".into();
    fs::write(
        fixture.0.join("config/beta.toml"),
        toml::to_string(&changed).unwrap(),
    )
    .unwrap();
    let refreshed = fixture.command(
        &fixture.0.join("extracted"),
        &[
            "generate",
            "--offline",
            "--no-history",
            "--config",
            "profile.toml",
            "--fallback-snapshot",
            "../config/offline-snapshot.json",
        ],
    );
    success(&refreshed);
    let updated = fixture.state(&fixture.0.join("extracted"));
    let exported: OrganizationManifest =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/organization.toml")).unwrap())
            .unwrap();
    let consumer: Config =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/profile.toml")).unwrap())
            .unwrap();

    // Assert
    assert_eq!(actual.icons, expected.icons);
    assert_eq!(
        actual
            .nodes
            .iter()
            .filter_map(|node| node.icon.as_ref().map(|icon| (&node.id, icon)))
            .collect::<BTreeMap<_, _>>(),
        expected
            .nodes
            .iter()
            .filter_map(|node| node.icon.as_ref().map(|icon| (&node.id, icon)))
            .collect::<BTreeMap<_, _>>()
    );
    assert_eq!(consumer.imports.len(), 2);
    assert_eq!(consumer.imports[1].id, "beta");
    assert!(matches!(
        &consumer.imports[1].source,
        OrganizationSource::Local { path } if path == "../config/beta.toml"
    ));
    assert_eq!(updated.icons["beta/signal"].radius, 18.0);
    assert!(
        updated
            .nodes
            .iter()
            .any(|node| node.id == "project:beta/database" && node.label == "Updated Beta")
    );
    assert_eq!(consumer.imports[0].id, "alpha");
    assert_eq!(exported.projects[0].id, "database");
    assert_eq!(exported.projects[0].icon.as_deref(), Some("signal"));
    assert_eq!(exported.icons.len(), 1);
    assert!(!consumer.icons.contains_key("beta/signal"));
    assert!(!consumer.icons.contains_key("alpha/signal"));
}

#[test]
fn personal_project_reuses_imported_icon_through_generation_and_extraction() {
    // Arrange
    let mut config = config();
    config.projects[0].icon = Some("alpha/signal".into());
    config.imports.push(OrganizationImport {
        id: "alpha".into(),
        source: OrganizationSource::Local {
            path: "alpha.toml".into(),
        },
    });
    let fixture = Fixture::new(&config);
    fs::write(
        fixture.0.join("config/alpha.toml"),
        toml::to_string(&organization("alpha")).unwrap(),
    )
    .unwrap();
    let personal_id = format!("project:{}", config.projects[0].id);

    // Act
    let generated = fixture.generate();
    success(&generated);
    let before = fixture.state(&fixture.0);
    let replay = fixture.replay();
    let extracted = fixture.command(
        &fixture.0,
        &[
            "extract-organization",
            "--config",
            "config/profile.toml",
            "--organization",
            "alpha",
            "--destination",
            "extracted",
        ],
    );
    success(&extracted);
    let regenerated = fixture.command(
        &fixture.0.join("extracted"),
        &[
            "generate",
            "--offline",
            "--no-history",
            "--config",
            "profile.toml",
            "--fallback-snapshot",
            "../config/offline-snapshot.json",
        ],
    );
    success(&regenerated);
    let after = fixture.state(&fixture.0.join("extracted"));

    // Assert
    success(&replay);
    assert_eq!(
        before
            .nodes
            .iter()
            .find(|node| node.id == personal_id)
            .unwrap()
            .icon
            .as_deref(),
        Some("alpha/signal")
    );
    assert_eq!(
        after
            .nodes
            .iter()
            .find(|node| node.id == personal_id)
            .unwrap()
            .icon
            .as_deref(),
        Some("alpha/signal")
    );
    assert!(!after.icons.contains_key("signal"));
    assert_eq!(
        after.icons.get("alpha/signal"),
        before.icons.get("alpha/signal")
    );
}

#[test]
fn authored_reference_to_unselected_import_is_rejected() {
    // Arrange
    let mut config = config();
    config.projects[0].icon = Some("unselected/signal".into());
    let fixture = Fixture::new(&config);

    // Act
    let generated = fixture.generate();

    // Assert
    assert!(!generated.status.success());
    assert!(!fixture.0.join("assets/profile-state.json").exists());
}

#[test]
fn missing_icon_in_selected_manifest_is_rejected_after_composition() {
    // Arrange
    let mut config = config();
    config.projects[0].icon = Some("alpha/absent".into());
    config.imports.push(OrganizationImport {
        id: "alpha".into(),
        source: OrganizationSource::Local {
            path: "alpha.toml".into(),
        },
    });
    let fixture = Fixture::new(&config);
    fs::write(
        fixture.0.join("config/alpha.toml"),
        toml::to_string(&organization("alpha")).unwrap(),
    )
    .unwrap();

    // Act
    let generated = fixture.generate();

    // Assert
    assert!(!generated.status.success());
    assert!(String::from_utf8_lossy(&generated.stderr).contains("unresolved icon reference"));
    assert!(!fixture.0.join("assets/profile-state.json").exists());
}

#[test]
fn extraction_copies_foreign_icon_with_collision_safe_local_alias() {
    // Arrange
    let mut config = config();
    config.imports.push(OrganizationImport {
        id: "beta".into(),
        source: OrganizationSource::Local {
            path: "beta.toml".into(),
        },
    });
    let mut local = icon();
    local.radius = 17.0;
    config.icons.insert("beta-signal".into(), local.clone());
    config
        .icons
        .insert("sample-labs/imported-icon-1".into(), local.clone());
    let mut selected = config
        .projects
        .iter_mut()
        .filter(|project| project.domain == "sample-labs");
    let borrowed = selected.next().unwrap();
    borrowed.icon = Some("beta/signal".into());
    let borrowed_id = borrowed.id.clone();
    selected.next().unwrap().icon = Some("beta-signal".into());
    let fixture = Fixture::new(&config);
    fs::write(
        fixture.0.join("config/beta.toml"),
        toml::to_string(&organization("beta")).unwrap(),
    )
    .unwrap();

    // Act
    let extracted = fixture.command(
        &fixture.0,
        &[
            "extract-organization",
            "--config",
            "config/profile.toml",
            "--organization",
            "sample-labs",
            "--destination",
            "extracted",
        ],
    );
    success(&extracted);
    let regenerated = fixture.command(
        &fixture.0.join("extracted"),
        &[
            "generate",
            "--offline",
            "--no-history",
            "--config",
            "profile.toml",
            "--fallback-snapshot",
            "../config/offline-snapshot.json",
        ],
    );
    success(&regenerated);
    let manifest: OrganizationManifest =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/organization.toml")).unwrap())
            .unwrap();
    let state = fixture.state(&fixture.0.join("extracted"));

    // Assert
    assert_eq!(manifest.icons.get("beta-signal"), Some(&local));
    assert_eq!(manifest.icons.get("imported-icon-2"), Some(&icon()));
    assert_eq!(
        manifest
            .projects
            .iter()
            .find(|project| project.id == borrowed_id)
            .unwrap()
            .icon
            .as_deref(),
        Some("imported-icon-2")
    );
    assert_eq!(state.icons.get("beta/signal"), Some(&icon()));
    assert_eq!(state.icons.get("sample-labs/imported-icon-1"), Some(&local));
    assert_eq!(
        state.icons.get("sample-labs/imported-icon-2"),
        Some(&icon())
    );
    assert!(
        state
            .nodes
            .iter()
            .any(|node| node.icon.as_deref() == Some("beta/signal"))
    );
}

#[test]
fn extraction_rejects_authored_domain_import_collision_before_writes() {
    // Arrange
    let mut config = config();
    config.imports.push(OrganizationImport {
        id: "sample-labs".into(),
        source: OrganizationSource::Local {
            path: "collision.toml".into(),
        },
    });
    let missing = Fixture::new(&config);
    let present = Fixture::new(&config);
    fs::write(
        present.0.join("config/collision.toml"),
        toml::to_string(&organization("sample-labs")).unwrap(),
    )
    .unwrap();

    // Act
    let results = [&missing, &present].map(|fixture| {
        fixture.command(
            &fixture.0,
            &[
                "extract-organization",
                "--config",
                "config/profile.toml",
                "--organization",
                "sample-labs",
                "--destination",
                "extracted",
            ],
        )
    });

    // Assert
    assert!(results.iter().all(|output| !output.status.success()));
    assert!(!missing.0.join("extracted").exists());
    assert!(!present.0.join("extracted").exists());
}

/// A captured remote input is self-contained; no remote transport is needed to extract it.
fn captured_remote_input() -> (Config, serde_json::Value) {
    let mut config = config();
    config.domains.clear();
    config.projects.clear();
    config.technologies.clear();
    config.publications.clear();
    config.collection.github_organizations.clear();
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let source = OrganizationSource::Remote {
        repository: "example-labs/.github".into(),
        reference: commit.into(),
        path: "config/canonical/organization.toml".into(),
    };

    config.imports.push(OrganizationImport {
        id: "alpha".into(),
        source: source.clone(),
    });
    let manifest = toml::to_string(&organization("alpha")).unwrap();
    let capture = serde_json::json!({
        "schema_version": 1,
        "imports": [{
            "id": "alpha",
            "source": source,
            "commit": commit,
            "repository": "example-labs/.github",
            "digest": sourcefield_io::sha256_bytes(manifest.as_bytes()),
            "manifest": manifest,
        }],
    });

    (config, capture)
}

#[test]
fn captured_remote_icon_extracts_offline_and_regenerates_with_saved_layout() {
    // Arrange
    let (config, capture) = captured_remote_input();
    let fixture = Fixture::new(&config);
    let capture_bytes = serde_json::to_vec(&capture).unwrap();
    fs::create_dir(fixture.0.join("assets")).unwrap();
    fs::write(fixture.0.join("assets/import-capture.json"), &capture_bytes).unwrap();
    fs::write(
        fixture.0.join("assets/layout.json"),
        serde_json::to_vec(&serde_json::json!({
            "domain:alpha": [900, 500],
            "project:alpha/database": [500, 760],
        }))
        .unwrap(),
    )
    .unwrap();

    // Act
    let extracted = fixture.command(
        &fixture.0,
        &[
            "extract-organization",
            "--config",
            "config/profile.toml",
            "--organization",
            "alpha",
            "--destination",
            "extracted",
        ],
    );
    success(&extracted);
    let regenerated = fixture.command(
        &fixture.0.join("extracted"),
        &[
            "generate",
            "--offline",
            "--no-history",
            "--config",
            "profile.toml",
            "--fallback-snapshot",
            "../config/offline-snapshot.json",
        ],
    );
    success(&regenerated);
    let state = fixture.state(&fixture.0.join("extracted"));
    let manifest: OrganizationManifest =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/organization.toml")).unwrap())
            .unwrap();
    let consumer: Config =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/profile.toml")).unwrap())
            .unwrap();

    // Assert
    assert_eq!(manifest.icons.get("signal"), Some(&icon()));
    assert_eq!(state.icons.get("alpha/signal"), Some(&icon()));
    let project = state
        .nodes
        .iter()
        .find(|node| node.id == "project:alpha/database")
        .unwrap();

    assert_eq!(project.icon.as_deref(), Some("alpha/signal"));
    assert_eq!((project.x, project.y), (500.0, 760.0));
    assert_eq!(consumer.imports.len(), 1);
    assert!(matches!(
        &consumer.imports[0].source,
        OrganizationSource::Local { path } if path == "organization.toml"
    ));
    assert_eq!(
        fs::read(fixture.0.join("assets/import-capture.json")).unwrap(),
        capture_bytes
    );
}

#[test]
fn remote_capture_failures_reject_extraction_before_writes() {
    // Arrange
    let (config, capture) = captured_remote_input();
    let mut source_mismatch = capture.clone();
    source_mismatch["imports"][0]["source"]["repository"] = "other-labs/.github".into();
    let mut digest_mismatch = capture.clone();
    digest_mismatch["imports"][0]["digest"] = "0".repeat(64).into();
    let mut repository_mismatch = capture;
    repository_mismatch["imports"][0]["repository"] = "other-labs/.github".into();
    let cases = [
        (None, "offline remote imports require captured inputs"),
        (Some(source_mismatch), "captured import source differs"),
        (Some(digest_mismatch), "captured manifest digest mismatch"),
        (Some(repository_mismatch), "captured repository differs"),
    ]
    .map(|(capture, diagnostic)| {
        let fixture = Fixture::new(&config);
        let before = capture.map(|capture| serde_json::to_vec(&capture).unwrap());

        if let Some(bytes) = &before {
            fs::create_dir(fixture.0.join("assets")).unwrap();
            fs::write(fixture.0.join("assets/import-capture.json"), bytes).unwrap();
        }

        (fixture, diagnostic, before)
    });

    // Act
    let results = cases.each_ref().map(|(fixture, _, _)| {
        fixture.command(
            &fixture.0,
            &[
                "extract-organization",
                "--config",
                "config/profile.toml",
                "--organization",
                "alpha",
                "--destination",
                "extracted",
            ],
        )
    });

    // Assert
    for ((fixture, diagnostic, before), result) in cases.iter().zip(results) {
        let stderr = String::from_utf8_lossy(&result.stderr);

        assert!(!result.status.success(), "{diagnostic}: {stderr}");
        assert!(stderr.contains(diagnostic), "{diagnostic}: {stderr}");
        assert!(!fixture.0.join("extracted").exists());
        assert!(!fixture.0.join(".sourcefield-owned.json").exists());
        assert_eq!(
            fs::read(fixture.0.join("assets/import-capture.json")).ok(),
            *before
        );
    }
}

/// Run the extraction entry point with a selected evidence directory.
fn extract_with_assets(fixture: &Fixture, scope: &str, assets: &str) -> Output {
    fixture.command(
        &fixture.0,
        &[
            "extract-organization",
            "--config",
            "config/profile.toml",
            "--organization",
            scope,
            "--destination",
            "extracted",
            "--assets",
            assets,
        ],
    )
}

/// Generate the exported consumer using the same selected evidence directory.
fn regenerate_extracted(fixture: &Fixture, assets: &str, locked: bool) -> Output {
    let mut arguments = vec![
        "generate",
        "--offline",
        "--no-history",
        "--config",
        "profile.toml",
        "--fallback-snapshot",
        "../config/offline-snapshot.json",
        "--assets",
        assets,
    ];

    if locked {
        arguments.push("--locked");
    }

    fixture.command(&fixture.0.join("extracted"), &arguments)
}

#[test]
fn custom_assets_preserve_remote_source_capture_order_and_shared_identity() {
    // Arrange
    let (mut config, mut capture) = captured_remote_input();
    let remote_source = config.imports[0].source.clone();
    let remote_manifest = capture["imports"][0]["manifest"]
        .as_str()
        .unwrap()
        .replace("id = \"alpha\"", "id = \"beta\"");
    capture["imports"][0]["id"] = "beta".into();
    capture["imports"][0]["manifest"] = remote_manifest.clone().into();
    capture["imports"][0]["digest"] =
        sourcefield_io::sha256_bytes(remote_manifest.as_bytes()).into();
    config.imports[0].id = "beta".into();
    config.imports.push(OrganizationImport {
        id: "alpha".into(),
        source: OrganizationSource::Local {
            path: "alpha.toml".into(),
        },
    });
    config.imports.push(OrganizationImport {
        id: "gamma".into(),
        source: OrganizationSource::Local {
            path: "gamma.toml".into(),
        },
    });
    let mut alpha = organization("alpha");
    alpha.icons.insert("unused-icon".into(), icon());
    alpha.technologies[0].id = "canonical-rust".into();
    alpha.projects[0].implemented_with = vec!["canonical-rust".into()];
    let mut component = self::config().projects[0].components[0].clone();
    component.integrates.clear();
    component.targets = vec!["canonical-rust".into()];
    alpha.projects[0].components.push(component);
    let template: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();
    let mut publication = template.publications[0].clone();
    publication.technologies = vec!["canonical-rust".into()];
    alpha.publications.push(publication);
    let mut technology = alpha.technologies[0].clone();
    technology.id = "personal-rust".into();
    config.technologies.push(technology);
    config
        .shared_technologies
        .insert("alpha/canonical-rust".into(), "personal-rust".into());
    config
        .shared_technologies
        .insert("beta/rust".into(), "personal-rust".into());
    let alpha_bytes = format!(
        "{}\n# Preserve canonical bytes.\n",
        toml::to_string(&alpha).unwrap()
    );
    let fixture = Fixture::new(&config);
    fs::write(fixture.0.join("config/alpha.toml"), &alpha_bytes).unwrap();
    fs::write(
        fixture.0.join("config/gamma.toml"),
        toml::to_string(&organization("gamma")).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(fixture.0.join("evidence/profile")).unwrap();
    fs::write(
        fixture.0.join("evidence/profile/import-capture.json"),
        serde_json::to_vec(&capture).unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.0.join("evidence/profile/layout.json"),
        serde_json::to_vec(&serde_json::json!({
            "domain:beta": [900, 500], "project:beta/database": [500, 760],
            "domain:alpha": [900, 1300], "project:alpha/database": [500, 1560],
            "domain:gamma": [900, 2100], "project:gamma/database": [500, 2360],
        }))
        .unwrap(),
    )
    .unwrap();
    fs::create_dir(fixture.0.join("assets")).unwrap();
    let mut default_capture = capture.clone();
    let mut default_manifest: OrganizationManifest = toml::from_str(&remote_manifest).unwrap();
    default_manifest.icons.get_mut("signal").unwrap().radius = 12.0;
    let default_manifest = toml::to_string(&default_manifest).unwrap();
    default_capture["imports"][0]["manifest"] = default_manifest.clone().into();
    default_capture["imports"][0]["digest"] =
        sourcefield_io::sha256_bytes(default_manifest.as_bytes()).into();
    fs::write(
        fixture.0.join("assets/import-capture.json"),
        serde_json::to_vec(&default_capture).unwrap(),
    )
    .unwrap();

    fs::write(
        fixture.0.join("assets/layout.json"),
        serde_json::to_vec(&serde_json::json!({
            "domain:beta": [900, 450], "project:beta/database": [600, 710],
            "domain:alpha": [900, 1240], "project:alpha/database": [600, 1500],
            "domain:gamma": [900, 2000], "project:gamma/database": [600, 2260],
        }))
        .unwrap(),
    )
    .unwrap();

    // Act
    let extracted = extract_with_assets(&fixture, "alpha", "evidence/profile");
    success(&extracted);
    let consumer: Config =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/profile.toml")).unwrap())
            .unwrap();
    let exported_capture: serde_json::Value = serde_json::from_slice(
        &fs::read(
            fixture
                .0
                .join("extracted/evidence/profile/import-capture.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let ownership: OwnershipManifest = serde_json::from_slice(
        &fs::read(fixture.0.join("extracted/.sourcefield-owned.json")).unwrap(),
    )
    .unwrap();
    let has_copied_record = fixture
        .0
        .join("extracted/evidence/profile/generation-record.json")
        .exists();
    let generated = regenerate_extracted(&fixture, "evidence/profile", false);
    success(&generated);
    let state: ProfileState = serde_json::from_slice(
        &fs::read(
            fixture
                .0
                .join("extracted/evidence/profile/profile-state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let replay = regenerate_extracted(&fixture, "evidence/profile", true);

    // Assert
    success(&replay);
    assert!(!has_copied_record);
    assert_eq!(
        consumer
            .imports
            .iter()
            .map(|import| import.id.as_str())
            .collect::<Vec<_>>(),
        ["beta", "alpha", "gamma"]
    );
    assert_eq!(
        serde_json::to_value(&consumer.imports[0].source).unwrap(),
        serde_json::to_value(remote_source).unwrap()
    );
    assert_eq!(consumer.shared_technologies, config.shared_technologies);
    assert_eq!(
        fs::read_to_string(fixture.0.join("extracted/organization.toml")).unwrap(),
        alpha_bytes
    );
    assert_eq!(exported_capture["imports"][0], capture["imports"][0]);
    assert!(
        ownership
            .files
            .contains_key("evidence/profile/import-capture.json")
    );
    assert!(ownership.files.contains_key("evidence/profile/layout.json"));
    assert!(!ownership.files.contains_key("profile.toml"));
    assert!(!ownership.files.contains_key("organization.toml"));
    assert_eq!(
        state
            .nodes
            .iter()
            .filter(|node| node.id == "technology:personal-rust")
            .count(),
        1
    );
    assert_eq!(state.icons["beta/signal"].radius, 16.0);
    assert!(!fixture.0.join("extracted/assets").exists());

    for (id, expected) in [
        ("beta", (500.0, 760.0)),
        ("alpha", (500.0, 1560.0)),
        ("gamma", (500.0, 2360.0)),
    ] {
        let node = state
            .nodes
            .iter()
            .find(|node| node.id == format!("project:{id}/database"))
            .unwrap();

        assert_eq!((node.x, node.y), expected);
    }

    assert!(
        state
            .edges
            .iter()
            .any(|edge| edge.from == "technology:personal-rust"
                && edge.to.starts_with("component:alpha/database:"))
    );
    assert!(
        state
            .edges
            .iter()
            .any(|edge| edge.from == "technology:personal-rust"
                && edge.to == "publication:alpha/database")
    );
}

#[test]
fn custom_assets_failures_reject_extraction_before_destination_creation() {
    // Arrange
    let (config, capture) = captured_remote_input();
    let cases = [
        "missing-capture",
        "malformed-capture",
        "malformed-layout",
        "outside-root",
    ]
    .map(|case| {
        let fixture = Fixture::new(&config);
        fs::create_dir(fixture.0.join("assets")).unwrap();
        fs::write(
            fixture.0.join("assets/import-capture.json"),
            serde_json::to_vec(&capture).unwrap(),
        )
        .unwrap();
        fs::write(fixture.0.join("assets/layout.json"), b"{}").unwrap();
        fs::create_dir_all(fixture.0.join("evidence/profile")).unwrap();

        if case == "malformed-capture" {
            fs::write(
                fixture.0.join("evidence/profile/import-capture.json"),
                "malformed",
            )
            .unwrap();
        } else if case != "missing-capture" {
            fs::write(
                fixture.0.join("evidence/profile/import-capture.json"),
                serde_json::to_vec(&capture).unwrap(),
            )
            .unwrap();
        }

        if case == "malformed-layout" {
            fs::write(fixture.0.join("evidence/profile/layout.json"), "malformed").unwrap();
        }

        (fixture, case)
    });

    // Act
    let results = cases.each_ref().map(|(fixture, case)| {
        extract_with_assets(
            fixture,
            "alpha",
            if *case == "outside-root" {
                "../evidence"
            } else {
                "evidence/profile"
            },
        )
    });

    // Assert
    for ((fixture, case), result) in cases.iter().zip(results) {
        assert!(!result.status.success(), "{case}");
        assert!(!fixture.0.join("extracted").exists(), "{case}");
    }
}

#[cfg(unix)]
#[test]
fn inline_extraction_keeps_symlinked_import_live_and_binding_only_technology() {
    // Arrange
    let mut config = config();
    let mut technology = config.technologies[0].clone();
    technology.id = "personal-rust".into();
    technology.affinities = vec!["sample-labs".into()];
    config.technologies.push(technology.clone());
    let project = config
        .projects
        .iter_mut()
        .find(|project| project.domain == "sample-labs")
        .unwrap();
    project.implemented_with.push("personal-rust".into());
    project.icon = Some("beta/signal".into());
    let selected_id = project.id.clone();
    let personal = config
        .projects
        .iter_mut()
        .find(|project| project.domain != "sample-labs")
        .unwrap();
    personal.icon = Some("beta/signal".into());
    config.imports.push(OrganizationImport {
        id: "beta".into(),
        source: OrganizationSource::Local {
            path: "../../canonical/organization.toml".into(),
        },
    });
    config
        .shared_technologies
        .insert("beta/canonical-rust".into(), "personal-rust".into());
    let mut beta = organization("beta");
    technology.id = "canonical-rust".into();
    technology.affinities = vec!["beta".into()];
    beta.technologies = vec![technology];
    beta.projects[0].implemented_with = vec!["canonical-rust".into()];
    let mut component = config.projects[0].components[0].clone();
    component.integrates.clear();
    component.targets = vec!["canonical-rust".into()];
    beta.projects[0].components = vec![component];
    let fixture = Fixture::new(&config);
    fs::create_dir_all(fixture.0.join("config/nested")).unwrap();
    fs::write(
        fixture.0.join("config/nested/profile.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(fixture.0.join("sources/one")).unwrap();
    fs::create_dir_all(fixture.0.join("sources/two")).unwrap();
    fs::write(
        fixture.0.join("sources/one/organization.toml"),
        toml::to_string(&beta).unwrap(),
    )
    .unwrap();
    beta.icons.get_mut("signal").unwrap().radius = 20.0;
    beta.projects[0].label = "Live Beta Update".into();
    fs::write(
        fixture.0.join("sources/two/organization.toml"),
        toml::to_string(&beta).unwrap(),
    )
    .unwrap();
    std::os::unix::fs::symlink("sources/one", fixture.0.join("canonical")).unwrap();

    // Act
    let extracted = fixture.command(
        &fixture.0,
        &[
            "extract-organization",
            "--config",
            "config/nested/profile.toml",
            "--organization",
            "sample-labs",
            "--destination",
            "extracted",
        ],
    );
    success(&extracted);
    let first = regenerate_extracted(&fixture, "assets", false);
    success(&first);
    let before = fixture.state(&fixture.0.join("extracted"));
    fs::remove_file(fixture.0.join("canonical")).unwrap();
    std::os::unix::fs::symlink("sources/two", fixture.0.join("canonical")).unwrap();
    let refreshed = regenerate_extracted(&fixture, "assets", false);
    success(&refreshed);
    let after = fixture.state(&fixture.0.join("extracted"));
    let consumer: Config =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/profile.toml")).unwrap())
            .unwrap();

    // Assert
    assert_eq!(
        consumer.shared_technologies.get("beta/canonical-rust"),
        Some(&"personal-rust".to_string())
    );
    let retained = consumer
        .technologies
        .iter()
        .find(|technology| technology.id == "personal-rust")
        .unwrap();

    assert!(retained.affinities.is_empty());
    assert_eq!(before.icons["beta/signal"].radius, 16.0);
    assert_eq!(after.icons["beta/signal"].radius, 20.0);
    assert_eq!(after.icons["sample-labs/beta-signal"].radius, 16.0);
    assert!(
        after
            .nodes
            .iter()
            .any(|node| node.id == "project:beta/database" && node.label == "Live Beta Update")
    );
    assert!(after.nodes.iter().any(
        |node| node.id == format!("project:sample-labs/{selected_id}")
            && node.icon.as_deref() == Some("sample-labs/beta-signal")
    ));
    assert_eq!(
        after
            .nodes
            .iter()
            .filter(|node| node.id == "technology:personal-rust")
            .count(),
        1
    );
    assert!(
        after
            .edges
            .iter()
            .any(|edge| edge.from == "technology:personal-rust"
                && edge.to == "project:beta/database")
    );
    assert!(
        after
            .edges
            .iter()
            .any(|edge| edge.from == "technology:personal-rust"
                && edge.to.starts_with("component:beta/database:"))
    );
    assert!(
        matches!(&consumer.imports[0].source, OrganizationSource::Local { path } if path.contains("config/nested/../../canonical/organization.toml"))
    );
}

#[test]
fn conflicting_or_reserved_assets_fail_before_export_creation() {
    // Arrange
    let fixture = Fixture::new(&config());
    let paths = [
        "profile.toml",
        "PROFILE.TOML",
        "organization.toml/cache",
        "Organization.Toml/cache",
        ".sourcefield-owned.json",
        "nested/.sourcefield-transaction",
        "nested/.git/cache",
        "nested/.GIT/cache",
        "nested/.SOURCEFIELD-TRANSACTION/cache",
        "NUL/cache",
    ];

    // Act
    let results = paths.map(|path| extract_with_assets(&fixture, "sample-labs", path));

    // Assert
    assert!(results.iter().all(|result| !result.status.success()));
    assert!(!fixture.0.join("extracted").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_assets_fail_before_export_creation() {
    // Arrange
    let fixture = Fixture::new(&config());
    fs::create_dir(fixture.0.join("actual-assets")).unwrap();
    std::os::unix::fs::symlink("actual-assets", fixture.0.join("assets-link")).unwrap();

    // Act
    let result = extract_with_assets(&fixture, "sample-labs", "assets-link");

    // Assert
    assert!(!result.status.success());
    assert!(!fixture.0.join("extracted").exists());
}

#[test]
fn inline_extraction_preserves_authored_domain_and_package_overrides() {
    // Arrange
    let mut config = config();
    config
        .layout
        .overrides
        .insert("domain:sample-labs".into(), [1280.0, 519.0]);
    let package_key = format!("package:{}", config.publications[0].packages[0].id);
    config
        .layout
        .overrides
        .insert(package_key.clone(), [105.0, 1067.0]);
    let fixture = Fixture::new(&config);
    success(&fixture.generate());
    let before = fixture.state(&fixture.0);

    // Act
    let extracted = extract_with_assets(&fixture, "sample-labs", "assets");
    success(&extracted);
    let generated = regenerate_extracted(&fixture, "assets", false);
    success(&generated);
    let after = fixture.state(&fixture.0.join("extracted"));
    let positions = |state: &ProfileState| {
        state
            .nodes
            .iter()
            .filter(|node| node.id == "domain:sample-labs" || node.id == package_key)
            .map(|node| (node.id.clone(), (node.x, node.y)))
            .collect::<BTreeMap<_, _>>()
    };

    // Assert
    assert_eq!(positions(&before).len(), 2);
    assert_eq!(positions(&after), positions(&before));
}

#[test]
fn extracted_shared_icon_copies_respect_aggregate_budget_before_writes() {
    // Arrange
    let mut config = config();
    config.icons = (0..sourcefield_core::MAX_ICON_DEFINITIONS)
        .map(|index| (format!("icon-{index}"), icon()))
        .collect();
    config
        .projects
        .iter_mut()
        .find(|project| project.domain == "sample-labs")
        .unwrap()
        .icon = Some("icon-0".into());
    config
        .projects
        .iter_mut()
        .find(|project| project.domain != "sample-labs")
        .unwrap()
        .icon = Some("icon-0".into());
    let fixture = Fixture::new(&config);

    // Act
    let result = extract_with_assets(&fixture, "sample-labs", "assets");

    // Assert
    assert!(!result.status.success());
    assert!(!fixture.0.join("extracted").exists());
}

#[test]
fn inline_extraction_preserves_implicit_package_positions() {
    // Arrange
    let mut config = config();
    let packages = &mut config.publications[0].packages;
    packages.sort_by(|left, right| right.id.cmp(&left.id));

    for package in packages.iter_mut() {
        package.anchor = None;
    }

    let package_keys = packages
        .iter()
        .map(|package| format!("package:{}", package.id))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&config);
    success(&fixture.generate());
    let before = fixture.state(&fixture.0);

    // Act
    let extracted = extract_with_assets(&fixture, "sample-labs", "assets");
    success(&extracted);
    let generated = regenerate_extracted(&fixture, "assets", false);
    success(&generated);
    let after = fixture.state(&fixture.0.join("extracted"));
    let positions = |state: &ProfileState| {
        state
            .nodes
            .iter()
            .filter(|node| package_keys.contains(&node.id))
            .map(|node| (node.id.clone(), (node.x, node.y)))
            .collect::<BTreeMap<_, _>>()
    };

    // Assert
    assert_eq!(positions(&before).len(), package_keys.len());
    assert_eq!(positions(&after), positions(&before));
}
