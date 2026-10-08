//! Exercise replay ordering against journaled on-disk publication and competing writers.

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use sourcefield_workspace::{OwnershipManifest, Transaction, file_sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);
const OWNERSHIP: &str = ".sourcefield-owned.json";
const RECORD: &str = "assets/generation-record.json";

/// A complete generated consumer isolates each replay from shared checkout state.
struct Fixture(PathBuf);

impl Fixture {
    /// Publish the compiled sample inputs through the actual CLI before arranging a replay.
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "sourcefield-replay-{}-{}",
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
            include_bytes!("../../../config/offline-snapshot.json"),
        )
        .unwrap();
        let fixture = Self(root);
        let output = fixture.generate(false);

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        fixture
    }

    /// Invoke the real binary with fixed offline policy and no environment credentials.
    fn generate(&self, locked: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sourcefield"));

        command
            .arg("--root")
            .arg(&self.0)
            .args(["generate", "--offline", "--no-history"])
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("PROFILE_TOKEN")
            .env_remove("SOURCEFIELD_PRIVATE_COUNTS");

        if locked {
            command.arg("--locked");
        }

        command.output().unwrap()
    }

    /// Capture every owned output so a replay can be checked for exact byte preservation.
    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        let manifest: OwnershipManifest =
            serde_json::from_slice(&fs::read(self.0.join(OWNERSHIP)).unwrap()).unwrap();

        manifest
            .files
            .keys()
            .chain(std::iter::once(&OWNERSHIP.to_owned()))
            .map(|name| (name.clone(), fs::read(self.0.join(name)).unwrap()))
            .collect()
    }

    /// Publish an invalid provenance record with an internally consistent ownership manifest.
    fn invalidate_record(&self) -> Vec<u8> {
        let original = fs::read(self.0.join(RECORD)).unwrap();
        let mut invalid: serde_json::Value = serde_json::from_slice(&original).unwrap();

        invalid["generator_version"] = "invalid-recovered-generator".into();
        let invalid = serde_json::to_vec_pretty(&invalid).unwrap();
        let mut transaction = Transaction::begin(&self.0).unwrap();

        for (name, bytes) in self.published() {
            if name != OWNERSHIP {
                let bytes = if name == RECORD { &invalid } else { &bytes };

                transaction.stage(name, bytes).unwrap();
            }
        }

        transaction.commit().unwrap();

        original
    }

    /// Materialize the durable journal/backup boundary and one already promoted candidate file.
    /// This models process interruption without adding a production fault-injection interface.
    fn interrupt_publication(&self, changed_path: &str, candidate_bytes: &[u8]) {
        let published = self.published();
        let mut transaction = Transaction::begin(&self.0).unwrap();
        let candidate = transaction.candidate_dir().to_path_buf();
        let control = candidate.parent().unwrap();
        let backup = control.join("backup");
        let mut files = BTreeMap::new();
        let mut entries = BTreeMap::new();

        for (name, bytes) in &published {
            let target = backup.join(name);

            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
            entries.insert(name.clone(), Some(file_sha256(self.0.join(name)).unwrap()));

            if name != OWNERSHIP {
                let bytes = if name == changed_path {
                    candidate_bytes
                } else {
                    bytes
                };

                transaction.stage(name, bytes).unwrap();
                files.insert(name.clone(), file_sha256(candidate.join(name)).unwrap());
            }
        }

        let manifest = OwnershipManifest {
            schema_version: 1,
            files,
            authored_files: BTreeMap::new(),
        };

        fs::write(
            candidate.join(OWNERSHIP),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let journal = serde_json::json!({
            "schema_version": 1,
            "candidate_manifest_sha256": file_sha256(candidate.join(OWNERSHIP)).unwrap(),
            "entries": entries,
        });

        fs::write(
            control.join("journal.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        fs::copy(candidate.join(changed_path), self.0.join(changed_path)).unwrap();
        drop(transaction);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup must not replace an assertion failure with a second panic during unwinding.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fixture_cleanup_removes_directory() {
    // Arrange
    let fixture = Fixture::new();
    let path = fixture.0.clone();

    // Act
    drop(fixture);

    // Assert
    assert!(!path.exists());
}

#[test]
fn fixture_cleanup_does_not_panic_when_directory_is_missing() {
    // Arrange
    let fixture = Fixture::new();
    fs::remove_dir_all(&fixture.0).unwrap();

    // Act
    let result = std::panic::catch_unwind(|| drop(fixture));

    // Assert
    assert!(result.is_ok(), "fixture cleanup must not introduce a panic");
}

#[test]
fn normal_locked_replay_preserves_every_published_byte() {
    let fixture = Fixture::new();
    let before = fixture.published();

    let output = fixture.generate(true);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.published(), before);
    assert!(!fixture.0.join(".sourcefield-lock").exists());
}

#[test]
fn interrupted_publication_restores_inputs_before_locked_replay() {
    let fixture = Fixture::new();
    let before = fixture.published();
    fixture.interrupt_publication("assets/source-snapshot.json", b"interrupted candidate");

    let output = fixture.generate(true);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fixture.published(), before);
    assert!(!fixture.0.join(".sourcefield-transaction").exists());
    assert!(!fixture.0.join(".sourcefield-lock").exists());
}

#[test]
fn invalid_recovered_provenance_is_rejected_before_replay() {
    let fixture = Fixture::new();
    let valid_record = fixture.invalidate_record();
    let before = fixture.published();
    fixture.interrupt_publication(RECORD, &valid_record);

    let output = fixture.generate(true);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "generation provenance generator identity mismatch: use the recorded generator build"
    ));
    assert_eq!(fixture.published(), before);
    assert!(!fixture.0.join(".sourcefield-transaction").exists());
    assert!(!fixture.0.join(".sourcefield-lock").exists());
}

#[test]
fn competing_writer_is_rejected_before_replay_input_verification() {
    let fixture = Fixture::new();
    let _writer = Transaction::begin(&fixture.0).unwrap();
    let lock_token = fs::read(fixture.0.join(".sourcefield-lock")).unwrap();
    fs::write(
        fixture.0.join("assets/source-snapshot.json"),
        b"writer in progress",
    )
    .unwrap();
    let before = fixture.published();

    let output = fixture.generate(true);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("workspace is locked"));
    assert_eq!(
        fs::read(fixture.0.join(".sourcefield-lock")).unwrap(),
        lock_token
    );
    assert_eq!(fixture.published(), before);
}

#[test]
fn changed_replay_input_is_rejected_without_publication() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("assets/source-snapshot.json"),
        b"unapproved edit",
    )
    .unwrap();
    let before = fixture.published();

    let output = fixture.generate(true);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("generation provenance input digest mismatch")
    );
    assert_eq!(fixture.published(), before);
    assert!(!fixture.0.join(".sourcefield-transaction").exists());
    assert!(!fixture.0.join(".sourcefield-lock").exists());
}
