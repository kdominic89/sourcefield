//! Authored captions survive canonical extraction, admission and authenticated locked replay.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use sourcefield_core::{
    AccountSnapshot, Config, DataStatus, OrganizationImport, OrganizationManifest,
    OrganizationSource, ProfileState, ProfileVariant, RepositoryCaptionConfig,
    RepositoryCaptionSource, Snapshot, SnapshotMode, SourceStatus, Visibility,
};
use sourcefield_workspace::OwnershipManifest;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Each probe uses actual CLI admission with isolated authored inputs and no inherited credentials.
struct Fixture(PathBuf);

impl Fixture {
    /// Write a minimal consumer and its explicit observation seed below a physical temporary root.
    fn new(config: &Config) -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "sourcefield-repository-captions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/profile.toml"),
            toml::to_string_pretty(config).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("config/offline-snapshot.json"),
            serde_json::to_vec(&Snapshot::default()).unwrap(),
        )
        .unwrap();

        Self(root)
    }

    /// Both variants import identical canonical organization bytes instead of duplicating settings.
    fn imported(variant: ProfileVariant) -> Self {
        let mut config = inline_config();

        config.domains.retain(|domain| domain.kind == "account");
        config.projects.clear();
        config.collection.github_organizations = vec!["example-labs".into()];
        config.imports.push(OrganizationImport {
            id: "example-labs".into(),
            source: OrganizationSource::Local {
                path: "organization.toml".into(),
            },
        });
        config.profile.variant = variant;
        if variant == ProfileVariant::Organization {
            config.domains.clear();
            config.profile.username = "example-labs".into();
            config.profile.organization = "example-labs".into();
            config.collection.github_user.clear();
        }

        let fixture = Self::new(&config);

        fixture.write_manifest(&organization());

        fixture
    }

    /// Isolate all inherited credentials before each invocation, including authorized test cases.
    fn cli(&self, root: &Path, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sourcefield"));

        command
            .arg("--root")
            .arg(root)
            .args(arguments)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("PROFILE_TOKEN")
            .env_remove("SOURCEFIELD_PRIVATE_COUNTS");

        command
    }

    /// Invoke the compiled CLI without allowing ambient authorization to alter count policy.
    fn command(&self, root: &Path, arguments: &[&str]) -> Output {
        self.cli(root, arguments).output().unwrap()
    }

    /// Model explicit offline authorization without collecting or storing actual credentials.
    fn private_command(&self, arguments: &[&str]) -> Output {
        self.cli(&self.0, arguments)
            .env("PROFILE_TOKEN", "synthetic-offline-authorization")
            .output()
            .unwrap()
    }

    /// Replay only authenticated publication inputs without current credentials or opt-in flags.
    fn replay(&self) -> Output {
        self.command(
            &self.0,
            &["generate", "--offline", "--locked", "--no-history"],
        )
    }

    /// Persist the synthetic observation seed independently of published effective captures.
    fn write_snapshot(&self, snapshot: &Snapshot) {
        fs::write(
            self.0.join("config/offline-snapshot.json"),
            serde_json::to_vec(snapshot).unwrap(),
        )
        .unwrap();
    }

    /// Generate an owned preview without network access or history updates.
    fn generate(&self) -> Output {
        self.command(&self.0, &["generate", "--offline", "--no-history"])
    }

    /// Export an inline organization into its own manifest without changing source inputs.
    fn extract(&self) -> Output {
        self.command(
            &self.0,
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
    }

    /// Persist a canonical content edit while retaining the same consumer and generator build.
    fn write_manifest(&self, manifest: &OrganizationManifest) {
        fs::write(
            self.0.join("config/organization.toml"),
            toml::to_string_pretty(manifest).unwrap(),
        )
        .unwrap();
    }

    /// Read generated facts without deriving expected captions through the production formatter.
    fn state(&self, root: &Path) -> ProfileState {
        serde_json::from_slice(&fs::read(root.join("assets/profile-state.json")).unwrap()).unwrap()
    }

    /// Read the exact compiled identity recorded by real generation before and after content edits.
    fn generator_identity(&self) -> [String; 3] {
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(self.0.join("assets/generation-record.json")).unwrap(),
        )
        .unwrap();

        [
            "generator_version",
            "source_revision",
            "generator_fingerprint",
        ]
        .map(|field| record[field].as_str().unwrap().to_string())
    }

    /// Capture actual owned bytes so failed validation cannot silently alter any artifact.
    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        let manifest: OwnershipManifest =
            serde_json::from_slice(&fs::read(self.0.join(".sourcefield-owned.json")).unwrap())
                .unwrap();

        manifest
            .files
            .keys()
            .chain(std::iter::once(&".sourcefield-owned.json".into()))
            .map(|name| (name.clone(), fs::read(self.0.join(name)).unwrap()))
            .collect()
    }

    /// Rebind only digests so captured-manifest validation must reject invalid authored facts.
    fn tamper_captured_manifest(&self, manifest: &str) {
        let assets = self.0.join("assets");
        let path = assets.join("import-capture.json");
        let mut capture: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

        capture["imports"][0]["manifest"] = manifest.into();
        capture["imports"][0]["digest"] = sourcefield_io::sha256_bytes(manifest.as_bytes()).into();
        let bytes = serde_json::to_vec_pretty(&capture).unwrap();

        fs::write(path, &bytes).unwrap();
        let record_path = assets.join("generation-record.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();

        record["inputs"]["import-capture.json"] = sourcefield_io::sha256_bytes(&bytes).into();
        fs::write(record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup must preserve the original failure if an assertion already unwinds.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Keep synthetic caption wording distinct from defaults to expose lost ownership during export.
fn selected_caption() -> RepositoryCaptionConfig {
    RepositoryCaptionConfig {
        source: RepositoryCaptionSource::SelectedProjects,
        public_label: "open".into(),
        private_label: "restricted".into(),
        suffix: "projects".into(),
        unavailable: "counts unknown".into(),
    }
}

/// Owner counts use independent configured observation totals rather than selected project circles.
fn owner_caption() -> RepositoryCaptionConfig {
    RepositoryCaptionConfig {
        source: RepositoryCaptionSource::OwnerRepositories,
        public_label: "public".into(),
        private_label: "private".into(),
        suffix: "repos".into(),
        unavailable: "repos unavailable".into(),
    }
}

/// Reuse approved synthetic geometry while removing unrelated technology and publication inputs.
fn inline_config() -> Config {
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml")).unwrap();

    config.technologies.clear();
    config.publications.clear();
    config.interests.clear();
    config.learning.clear();
    config
        .projects
        .retain(|project| project.domain == "sample-labs");
    config.collection.nuget_owner = None;
    config.collection.discover_public_repositories = false;
    config.collection.collect_contributions = false;
    for project in &mut config.projects {
        project.implemented_with.clear();
        project.integrates.clear();
        project.targets.clear();
        project.components.clear();
    }

    for domain in &mut config.domains {
        domain.repository_caption = Some(if domain.kind == "organization" {
            selected_caption()
        } else {
            owner_caption()
        });
    }

    config
}

/// The same manifest owns three selected public projects and one unlinked private abstraction.
fn organization() -> OrganizationManifest {
    let mut manifest: OrganizationManifest =
        toml::from_str(include_str!("../../../examples/organization.toml")).unwrap();
    let mut project = manifest.projects.remove(0);

    project.implemented_with.clear();
    manifest.technologies.clear();
    manifest.publications.clear();
    manifest.repository_caption = Some(selected_caption());
    for id in ["alpha", "beta", "gamma", "private-lab"] {
        let mut selected = project.clone();

        selected.id = id.into();
        selected.label = id.into();
        selected.surface_label = id.into();
        selected.repository = Some(format!("example-labs/{id}"));
        if id == "private-lab" {
            selected.visibility = Visibility::PrivateAbstract;
            selected.repository = None;
        }

        manifest.projects.push(selected);
    }

    manifest
}

/// A synthetic private aggregate is an observation; publication still requires explicit consent.
fn owner_observations() -> Snapshot {
    Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T12:00:00Z".into(),
        user: AccountSnapshot {
            login: "sample-user".into(),
            public_repositories: 12,
            ..AccountSnapshot::default()
        },
        private_repository_count: Some(9),
        sources: vec![
            SourceStatus {
                source: "github:user".into(),
                status: DataStatus::Live,
            },
            SourceStatus {
                source: "github:private-count".into(),
                status: DataStatus::Live,
            },
        ],
        ..Snapshot::default()
    }
}

/// Report process diagnostics only when a synthetic command violates its expected success boundary.
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "synthetic caption command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Read one already materialized domain caption for behavioral assertions.
fn caption<'a>(state: &'a ProfileState, id: &str) -> Option<&'a str> {
    state
        .nodes
        .iter()
        .find(|node| node.id == format!("domain:{id}"))
        .unwrap()
        .repository_caption
        .as_deref()
}

#[test]
fn extraction_preserves_canonical_caption_and_unselected_personal_setting() {
    // Arrange
    let config = inline_config();
    let fixture = Fixture::new(&config);
    let authored = fs::read(fixture.0.join("config/profile.toml")).unwrap();

    // Act
    let output = fixture.extract();

    // Assert
    success(&output);
    let manifest: OrganizationManifest =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/organization.toml")).unwrap())
            .unwrap();
    let consumer: Config =
        toml::from_str(&fs::read_to_string(fixture.0.join("extracted/profile.toml")).unwrap())
            .unwrap();

    assert_eq!(manifest.repository_caption, Some(selected_caption()));
    assert_eq!(consumer.domains.len(), 1);
    assert_eq!(
        consumer.domains[0].repository_caption,
        Some(owner_caption())
    );
    assert_eq!(consumer.imports[0].id, "sample-labs");
    assert!(fs::read(fixture.0.join("config/profile.toml")).unwrap() == authored);
}

#[test]
fn extracted_caption_survives_manifest_serialization_and_generation() {
    // Arrange
    let fixture = Fixture::new(&inline_config());

    success(&fixture.extract());
    let root = fixture.0.join("extracted");

    // Act
    let output = fixture.command(
        &root,
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

    // Assert
    success(&output);
    let state = fixture.state(&root);

    assert_eq!(
        caption(&state, "sample-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert_eq!(caption(&state, "personal"), Some("repos unavailable"));
}

#[test]
fn personal_profile_uses_organization_owned_caption_from_the_canonical_import() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    // Act
    let output = fixture.generate();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(state.profile.variant, ProfileVariant::Personal);
    assert_eq!(
        caption(&state, "example-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert!(state.nodes.iter().any(|node| {
        node.id == "project:example-labs/private-lab"
            && node.visibility == Some(Visibility::PrivateAbstract)
            && node.url.is_none()
    }));
}

#[test]
fn organization_profile_uses_the_same_canonical_caption_without_a_consumer_copy() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Organization);

    // Act
    let output = fixture.generate();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(state.profile.variant, ProfileVariant::Organization);
    assert_eq!(
        caption(&state, "example-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert!(state.nodes.iter().all(|node| node.id != "domain:personal"));
}

#[test]
fn personal_owner_source_uses_observed_total_instead_of_selected_project_circles() {
    // Arrange
    let fixture = Fixture::new(&inline_config());
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T12:00:00Z".into(),
        user: AccountSnapshot {
            login: "sample-user".into(),
            public_repositories: 12,
            ..AccountSnapshot::default()
        },
        sources: vec![SourceStatus {
            source: "github:user".into(),
            status: DataStatus::Live,
        }],
        ..Snapshot::default()
    };

    fs::write(
        fixture.0.join("config/offline-snapshot.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();

    // Act
    let output = fixture.generate();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(caption(&state, "personal"), Some("12 public repos"));
    assert_eq!(
        caption(&state, "sample-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert_eq!(state.stats.private_repository_count, None);
}

#[test]
fn later_canonical_wording_changes_use_the_same_compiled_generator() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    success(&fixture.generate());
    let before = fixture.state(&fixture.0);
    let previous = fixture.generator_identity();
    let mut manifest = organization();

    manifest.repository_caption.as_mut().unwrap().public_label = "published".into();
    fixture.write_manifest(&manifest);

    // Act
    let output = fixture.generate();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(
        caption(&state, "example-labs"),
        Some("3 published / 1 restricted projects")
    );
    assert_ne!(state.semantic_hash, before.semantic_hash);
    assert_eq!(state.nodes.len(), before.nodes.len());
    assert_eq!(
        fixture.generator_identity(),
        previous,
        "text edit must retain the build"
    );
}

#[test]
fn later_canonical_count_source_changes_use_the_same_compiled_generator() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T12:00:00Z".into(),
        organizations: vec![AccountSnapshot {
            login: "example-labs".into(),
            public_repositories: 99,
            ..AccountSnapshot::default()
        }],
        sources: vec![SourceStatus {
            source: "github:org:example-labs".into(),
            status: DataStatus::Live,
        }],
        ..Snapshot::default()
    };

    fs::write(
        fixture.0.join("config/offline-snapshot.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    success(&fixture.generate());
    let before = fixture.state(&fixture.0);
    let previous = fixture.generator_identity();
    let mut manifest = organization();

    manifest.repository_caption.as_mut().unwrap().source =
        RepositoryCaptionSource::OwnerRepositories;
    fixture.write_manifest(&manifest);

    // Act
    let output = fixture.generate();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(
        caption(&before, "example-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert_eq!(caption(&state, "example-labs"), Some("99 open projects"));
    assert_ne!(state.semantic_hash, before.semantic_hash);
    assert_eq!(state.nodes.len(), before.nodes.len());
    assert_eq!(
        fixture.generator_identity(),
        previous,
        "source edit must retain the build"
    );
    assert!(state.nodes.iter().any(|node| {
        node.id == "project:example-labs/private-lab"
            && node.visibility == Some(Visibility::PrivateAbstract)
            && node.url.is_none()
    }));
}

#[test]
fn validation_rejects_unknown_inline_caption_source_before_loading_artifacts() {
    // Arrange
    let fixture = Fixture::new(&inline_config());
    let path = fixture.0.join("config/profile.toml");
    let text = fs::read_to_string(&path).unwrap();

    fs::write(
        path,
        text.replace("selected-projects", "unregistered-source"),
    )
    .unwrap();

    // Act
    let output = fixture.command(&fixture.0, &["validate"]);

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        message.contains("unknown variant `unregistered-source`"),
        "unexpected synthetic validation diagnostic: {message}"
    );
    assert!(!fixture.0.join("assets").exists());
    assert!(!fixture.0.join("docs").exists());
}

#[test]
fn validation_rejects_multiline_inline_caption_label_before_loading_artifacts() {
    // Arrange
    let mut config = inline_config();

    config.domains[0]
        .repository_caption
        .as_mut()
        .unwrap()
        .public_label = "public\nprivate".into();
    let fixture = Fixture::new(&config);

    // Act
    let output = fixture.command(&fixture.0, &["validate"]);

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        message.contains("repository caption public_label"),
        "unexpected synthetic validation diagnostic: {message}"
    );
    assert!(!fixture.0.join("assets").exists());
    assert!(!fixture.0.join("docs").exists());
}

#[test]
fn validation_rejects_unknown_captured_caption_source_even_with_rebound_digests() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    success(&fixture.generate());
    let manifest = toml::to_string_pretty(&organization()).unwrap();

    fixture.tamper_captured_manifest(&manifest.replace("selected-projects", "unregistered-source"));
    let before = fixture.published();

    // Act
    let output = fixture.command(&fixture.0, &["validate"]);

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    // Untrusted canonical parse errors deliberately expose one redacted public diagnostic.
    assert!(
        message.contains("invalid canonical organization manifest"),
        "unexpected synthetic validation diagnostic: {message}"
    );
    assert!(!message.contains("digest mismatch"));
    assert!(
        fixture.published() == before,
        "rejected captured facts must not publish"
    );
}

#[test]
fn validation_rejects_multiline_canonical_caption_label_even_with_rebound_digests() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    success(&fixture.generate());
    let mut manifest = organization();

    manifest.repository_caption.as_mut().unwrap().public_label = "public\nprivate".into();
    fixture.tamper_captured_manifest(&toml::to_string_pretty(&manifest).unwrap());
    let before = fixture.published();

    // Act
    let output = fixture.command(&fixture.0, &["validate"]);

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    // Canonical text admission follows digest verification without disclosing untrusted labels.
    assert!(
        message.contains("invalid canonical organization manifest"),
        "unexpected synthetic validation diagnostic: {message}"
    );
    assert!(!message.contains("digest mismatch"));
    assert!(
        fixture.published() == before,
        "rejected captured facts must not publish"
    );
}

#[test]
fn personal_locked_replay_keeps_captured_canonical_caption_after_local_manifest_changes() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    success(&fixture.generate());
    let before = fixture.published();
    let mut manifest = organization();
    let configured = manifest.repository_caption.as_mut().unwrap();

    configured.public_label = "fresh".into();
    configured.source = RepositoryCaptionSource::OwnerRepositories;
    fixture.write_manifest(&manifest);

    // Act
    let output = fixture.replay();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(state.profile.variant, ProfileVariant::Personal);
    assert_eq!(
        caption(&state, "example-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert!(
        fixture.published() == before,
        "locked replay must retain all previously owned publication bytes"
    );
}

#[test]
fn organization_locked_replay_keeps_captured_canonical_caption_after_local_manifest_changes() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Organization);

    success(&fixture.generate());
    let before = fixture.published();
    let mut manifest = organization();
    let configured = manifest.repository_caption.as_mut().unwrap();

    configured.public_label = "fresh".into();
    configured.source = RepositoryCaptionSource::OwnerRepositories;
    fixture.write_manifest(&manifest);

    // Act
    let output = fixture.replay();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(state.profile.variant, ProfileVariant::Organization);
    assert_eq!(
        caption(&state, "example-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert!(state.nodes.iter().all(|node| node.id != "domain:personal"));
    assert!(
        fixture.published() == before,
        "locked replay must retain all previously owned publication bytes"
    );
}

#[test]
fn personal_locked_replay_keeps_authorized_private_caption_without_credentials_or_opt_in() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    fixture.write_snapshot(&owner_observations());
    success(&fixture.private_command(&[
        "generate",
        "--offline",
        "--private-counts",
        "--no-history",
    ]));
    let before = fixture.published();

    // Act
    let output = fixture.replay();

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(
        caption(&state, "personal"),
        Some("12 public / 9 private repos")
    );
    assert_eq!(state.stats.private_repository_count, Some(9));
    assert_eq!(
        caption(&state, "example-labs"),
        Some("3 open / 1 restricted projects")
    );
    assert!(
        fixture.published() == before,
        "replay must retain the approved aggregate without new authorization"
    );
}

#[test]
fn personal_locked_replay_cannot_add_private_counts_with_new_credentials_and_opt_in() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    fixture.write_snapshot(&owner_observations());
    success(&fixture.generate());
    let before = fixture.published();

    // Act
    let output = fixture.private_command(&[
        "generate",
        "--offline",
        "--locked",
        "--private-counts",
        "--no-history",
    ]);

    // Assert
    success(&output);
    let state = fixture.state(&fixture.0);

    assert_eq!(caption(&state, "personal"), Some("12 public repos"));
    assert_eq!(state.stats.private_repository_count, None);
    assert!(
        fixture.published() == before,
        "new consent must not escalate the recorded public-only replay inputs"
    );
}

#[test]
fn locked_replay_rejects_changed_authored_caption_without_publishing() {
    // Arrange
    let mut config = inline_config();
    let fixture = Fixture::new(&config);

    success(&fixture.generate());
    config.domains[0]
        .repository_caption
        .as_mut()
        .unwrap()
        .public_label = "published".into();
    let path = fixture.0.join("config/profile.toml");
    let authored = toml::to_string_pretty(&config).unwrap();

    fs::write(&path, &authored).unwrap();
    let before = fixture.published();

    // Act
    let output = fixture.replay();

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        message.contains("generation provenance authored inputs changed"),
        "unexpected synthetic replay diagnostic: {message}"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), authored);
    assert!(
        fixture.published() == before,
        "rejected authored caption changes must not alter any owned byte"
    );
}

#[test]
fn locked_replay_rejects_tampered_captured_caption_digest_without_publishing() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Personal);

    success(&fixture.generate());
    let path = fixture.0.join("assets/import-capture.json");
    let mut capture: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let manifest = capture["imports"][0]["manifest"].as_str().unwrap();

    capture["imports"][0]["manifest"] = manifest
        .replace("selected-projects", "owner-repositories")
        .into();
    fs::write(path, serde_json::to_vec_pretty(&capture).unwrap()).unwrap();
    let before = fixture.published();

    // Act
    let output = fixture.replay();

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        message.contains("generation provenance input digest mismatch: import-capture.json"),
        "unexpected synthetic replay diagnostic: {message}"
    );
    assert!(
        fixture.published() == before,
        "rejected capture edits must not alter any owned byte"
    );
}

#[test]
fn locked_replay_rejects_invalid_captured_caption_with_rebound_digests_without_publishing() {
    // Arrange
    let fixture = Fixture::imported(ProfileVariant::Organization);

    success(&fixture.generate());
    let mut manifest = organization();

    manifest.repository_caption.as_mut().unwrap().public_label = "public\nprivate".into();
    fixture.tamper_captured_manifest(&toml::to_string_pretty(&manifest).unwrap());
    let before = fixture.published();

    // Act
    let output = fixture.replay();

    // Assert
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        message.contains("invalid canonical organization manifest"),
        "unexpected synthetic replay diagnostic: {message}"
    );
    assert!(!message.contains("digest mismatch"));
    assert!(
        fixture.published() == before,
        "digest verification must not replace semantic caption admission"
    );
}
