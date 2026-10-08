//! Durable observations and effective rendering inputs remain distinct through the public CLI.

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use sourcefield_core::{DataStatus, Snapshot, SnapshotMode};
use sourcefield_workspace::{OwnershipManifest, Transaction};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// An isolated approved capture has deliberately noncanonical JSON bytes and a private aggregate.
struct Fixture {
    root: PathBuf,
    source: Vec<u8>,
}

impl Fixture {
    /// Admit authored facts and the existing capture through the real ownership boundary.
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "sourcefield-capture-lifecycle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/profile.toml"),
            include_bytes!("../../../config/profile.toml"),
        )
        .unwrap();
        fs::write(
            root.join("config/offline-snapshot.json"),
            serde_json::to_vec(&Snapshot::default()).unwrap(),
        )
        .unwrap();
        let mut snapshot: Snapshot =
            serde_json::from_slice(include_bytes!("../../../config/offline-snapshot.json"))
                .unwrap();

        snapshot.mode = SnapshotMode::Live;
        snapshot.fetched_at = "2026-10-01T12:00:00Z".into();
        snapshot.private_repository_count = Some(27);
        snapshot.warnings.clear();
        for source in &mut snapshot.sources {
            source.status = DataStatus::Live;
        }

        let mut source = serde_json::to_vec_pretty(&snapshot).unwrap();

        // Legal trailing whitespace catches accidental reserialization of the durable capture.
        source.extend_from_slice(b"\n \n");
        let mut transaction = Transaction::begin(&root).unwrap();

        transaction
            .stage("assets/source-snapshot.json", &source)
            .unwrap();
        transaction.commit().unwrap();

        Self { root, source }
    }

    /// Corrupt only envelope admission, preserving actual generated roles and their digests.
    fn publish_record_identity(&self, schema_version: u32, generator_version: Option<&str>) {
        let path = self.root.join("assets/generation-record.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

        record["schema_version"] = schema_version.into();
        if let Some(version) = generator_version {
            record["generator_version"] = version.into();
        }

        fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }

    /// Run the compiled CLI with no inherited authorization or alternate offline seed.
    fn command(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sourcefield"))
            .arg("--root")
            .arg(&self.root)
            .args(arguments)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("PROFILE_TOKEN")
            .env_remove("SOURCEFIELD_PRIVATE_COUNTS")
            .output()
            .unwrap()
    }

    /// Read all owned bytes so a deterministic replay assertion includes generated provenance.
    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        let manifest: OwnershipManifest =
            serde_json::from_slice(&fs::read(self.root.join(".sourcefield-owned.json")).unwrap())
                .unwrap();

        manifest
            .files
            .keys()
            .chain(std::iter::once(&".sourcefield-owned.json".into()))
            .map(|name| (name.clone(), fs::read(self.root.join(name)).unwrap()))
            .collect()
    }

    /// Rebind durable bytes and ownership to probe schema admission beyond digest comparison.
    fn publish_durable_schema(&self, schema_version: u32) {
        let mut files = self.published();
        let mut source: Snapshot =
            serde_json::from_slice(&files["assets/source-snapshot.json"]).unwrap();

        source.schema_version = schema_version;
        let bytes = serde_json::to_vec_pretty(&source).unwrap();
        let digest = sourcefield_io::sha256_bytes(&bytes);

        files.insert("assets/source-snapshot.json".into(), bytes);
        if let Some(bytes) = files.get_mut("assets/generation-record.json") {
            let mut record: serde_json::Value = serde_json::from_slice(bytes).unwrap();

            record["inputs"]["source-snapshot.json"] = digest.into();
            *bytes = serde_json::to_vec_pretty(&record).unwrap();
        }

        let mut transaction = Transaction::begin(&self.root).unwrap();

        for (name, bytes) in files {
            if name != ".sourcefield-owned.json" {
                transaction.stage(name, &bytes).unwrap();
            }
        }

        transaction.commit().unwrap();
    }

    /// Arrange a preview through the production pipeline before probing validation or replay.
    fn preview(&self) {
        let output = self.command(&[
            "generate",
            "--offline",
            "--fallback-snapshot",
            "assets/source-snapshot.json",
            "--no-history",
        ]);

        assert!(
            output.status.success(),
            "preview must publish: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup must not replace the assertion that identified the failed input boundary.
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn offline_preview_retains_dated_live_capture_bytes_but_seals_effective_private_data() {
    // Arrange
    let fixture = Fixture::new();

    // Act
    let output = fixture.command(&[
        "generate",
        "--offline",
        "--fallback-snapshot",
        "assets/source-snapshot.json",
        "--no-history",
    ]);

    // Assert
    assert!(
        output.status.success(),
        "preview must publish: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let source = fs::read(fixture.root.join("assets/source-snapshot.json")).unwrap();
    let effective: Snapshot = serde_json::from_slice(
        &fs::read(fixture.root.join("assets/render-snapshot.json")).unwrap(),
    )
    .unwrap();

    assert!(
        source == fixture.source,
        "approved source bytes must remain exact"
    );
    assert_eq!(effective.mode, SnapshotMode::Preview);
    assert!(effective.fetched_at.is_empty());
    assert_eq!(effective.private_repository_count, None);
    assert!(
        effective
            .sources
            .iter()
            .all(|source| source.status == DataStatus::Preview)
    );
}

#[test]
fn replay_reuses_effective_preview_without_reauthorizing_the_approved_source() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["generate", "--offline", "--locked"]);

    // Assert
    assert!(
        output.status.success(),
        "locked replay must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture.published() == before,
        "replay must preserve every owned byte"
    );
    assert!(fs::read(fixture.root.join("assets/source-snapshot.json")).unwrap() == fixture.source);
}

#[test]
fn standalone_validation_reconstructs_the_sealed_render_snapshot() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        output.status.success(),
        "effective inputs must validate: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture.published() == before,
        "validation must never publish"
    );
}

#[test]
fn malformed_capture_never_downgrades_to_the_available_seed() {
    // Arrange
    let fixture = Fixture::new();
    let path = fixture.root.join("assets/source-snapshot.json");

    fs::write(&path, b"{not valid JSON").unwrap();

    // Act
    let output = fixture.command(&[
        "generate",
        "--offline",
        "--fallback-snapshot",
        "assets/source-snapshot.json",
    ]);

    // Assert
    assert!(
        !output.status.success(),
        "malformed capture must fail closed"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("parse observation capture"));
    assert!(fs::read(path).unwrap() == b"{not valid JSON");
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn absent_offline_capture_never_downgrades_to_the_available_seed() {
    // Arrange
    let fixture = Fixture::new();

    fs::remove_file(fixture.root.join("assets/source-snapshot.json")).unwrap();

    // Act
    let output = fixture.command(&[
        "generate",
        "--offline",
        "--fallback-snapshot",
        "assets/source-snapshot.json",
    ]);

    // Assert
    assert!(
        !output.status.success(),
        "offline mode requires its selected capture"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("load captured observation snapshot"));
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn first_offline_seed_cannot_create_an_unapproved_private_capture() {
    // Arrange
    let fixture = Fixture::new();
    let seed = Snapshot {
        private_repository_count: Some(27),
        ..Snapshot::default()
    };

    fs::remove_file(fixture.root.join("assets/source-snapshot.json")).unwrap();
    fs::remove_file(fixture.root.join(".sourcefield-owned.json")).unwrap();
    fs::write(
        fixture.root.join("config/offline-snapshot.json"),
        serde_json::to_vec(&seed).unwrap(),
    )
    .unwrap();

    // Act
    let output = fixture.command(&["generate", "--offline", "--no-history"]);

    // Assert
    assert!(
        output.status.success(),
        "first authored preview must publish"
    );
    let source: Snapshot = serde_json::from_slice(
        &fs::read(fixture.root.join("assets/source-snapshot.json")).unwrap(),
    )
    .unwrap();
    let effective: Snapshot = serde_json::from_slice(
        &fs::read(fixture.root.join("assets/render-snapshot.json")).unwrap(),
    )
    .unwrap();

    assert_eq!(source.private_repository_count, None);
    assert_eq!(effective.private_repository_count, None);
    assert_eq!(source.mode, SnapshotMode::Preview);
    assert!(source.fetched_at.is_empty());
}

#[test]
fn standalone_validation_rejects_schema_one_from_the_current_generator() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    fixture.publish_record_identity(1, None);
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(!output.status.success(), "schema one must not be admitted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsupported generation record schema: 1")
    );
    assert!(fixture.published() == before, "validation must not publish");
}

#[test]
fn locked_replay_rejects_schema_one_from_the_current_generator() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    fixture.publish_record_identity(1, None);
    let before = fixture.published();

    // Act
    let output = fixture.command(&["generate", "--offline", "--locked"]);

    // Assert
    assert!(!output.status.success(), "schema one must not be admitted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsupported generation record schema: 1")
    );
    assert!(
        fixture.published() == before,
        "rejected replay must not publish"
    );
    assert!(!fixture.root.join(".sourcefield-lock").exists());
    assert!(!fixture.root.join(".sourcefield-transaction").exists());
}

#[test]
fn standalone_validation_reports_older_generator_before_older_envelope_schema() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    fixture.publish_record_identity(1, Some("0.1.0"));
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "older generator must not be admitted"
    );
    assert!(message.contains(
        "generation provenance generator identity mismatch: use the recorded generator build"
    ));
    assert!(!message.contains("unsupported generation record schema"));
    assert!(fixture.published() == before, "validation must not publish");
}

#[test]
fn locked_replay_reports_older_generator_before_older_envelope_schema() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    fixture.publish_record_identity(1, Some("0.1.0"));
    let before = fixture.published();

    // Act
    let output = fixture.command(&["generate", "--offline", "--locked"]);

    // Assert
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "older generator must not be admitted"
    );
    assert!(message.contains(
        "generation provenance generator identity mismatch: use the recorded generator build"
    ));
    assert!(!message.contains("unsupported generation record schema"));
    assert!(
        fixture.published() == before,
        "rejected replay must not publish"
    );
    assert!(!fixture.root.join(".sourcefield-lock").exists());
    assert!(!fixture.root.join(".sourcefield-transaction").exists());
}

#[test]
fn unsupported_durable_schema_cannot_hide_behind_a_supported_separate_preview_seed() {
    // Arrange
    let fixture = Fixture::new();

    fixture.publish_durable_schema(999);
    let before = fixture.published();

    // Act
    let output = fixture.command(&["generate", "--offline", "--no-history"]);

    // Assert
    assert!(
        !output.status.success(),
        "valid effective seed must not admit unsupported durable input"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported observation capture schema: 999")
    );
    assert!(
        fixture.published() == before,
        "schema rejection must preserve every owned byte"
    );
    assert!(!fixture.root.join("docs").exists());
}

#[test]
fn standalone_validation_rejects_unsupported_durable_schema_even_with_rebound_digest() {
    // Arrange
    let fixture = Fixture::new();

    fixture.preview();
    fixture.publish_durable_schema(999);
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "digest admission must not replace schema admission"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported observation capture schema: 999")
    );
    assert!(fixture.published() == before, "validation must not publish");
}
