//! Preserve history policy through production publication and ownership cleanup.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Isolate the history transaction and clean up without obscuring failed assertions.
struct Fixture(PathBuf);

impl Fixture {
    /// Create a distinct physical root accepted by the workspace transaction.
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "sourcefield-history-policy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir_all(&root).unwrap();

        Self(root)
    }

    /// Stage through the production helper so ownership removal is part of the assertion.
    fn publish(&self, state: &ProfileState, limit: usize, append: bool) -> Result<()> {
        let mut transaction = Transaction::begin(&self.0)?;

        stage_history(
            &mut transaction,
            &self.0,
            Path::new("docs"),
            state,
            limit,
            append,
            false,
        )?;
        transaction.commit()?;

        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A cleanup error must not replace the history invariant failure.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Synthetic live state exercises history policy without opening an HTTP transport.
fn live_state(hash: &str, day: u8) -> ProfileState {
    let config = toml::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/profile.toml"
    )))
    .unwrap();
    let mut state = build_state(&config, &Snapshot::default(), "1970-01-01T00:00:00Z").unwrap();

    state.mode = SnapshotMode::Live;
    state.semantic_hash = hash.into();
    state.generated_at = format!("2026-09-{day:02}T12:00:00Z");

    state
}

#[test]
fn disabled_history_keeps_the_index_bytes_order_and_all_owned_archives() {
    // Arrange
    let fixture = Fixture::new();
    let oldest = live_state("1111111111111111", 1);
    let newest = live_state("2222222222222222", 2);

    fixture.publish(&oldest, 24, true).unwrap();
    fixture.publish(&newest, 24, true).unwrap();
    let history = fixture.0.join("docs/history");
    let index_before = fs::read(history.join("index.json")).unwrap();
    let oldest_before = fs::read(history.join("1111111111111111.json")).unwrap();
    let newest_before = fs::read(history.join("2222222222222222.json")).unwrap();

    // Act
    let result = fixture.publish(&newest, 1, false);

    // Assert
    assert!(result.is_ok(), "disabled history staging must succeed");
    assert!(
        fs::read(history.join("index.json")).unwrap() == index_before,
        "disabled history must preserve index bytes"
    );
    assert!(
        fs::read(history.join("1111111111111111.json")).unwrap() == oldest_before,
        "old archive bytes must be preserved"
    );
    assert!(
        fs::read(history.join("2222222222222222.json")).unwrap() == newest_before,
        "new archive bytes must be preserved"
    );
}

#[tokio::test]
async fn live_no_history_generation_does_not_apply_retention_or_append_an_archive() {
    // Arrange
    let fixture = Fixture::new();
    let oldest = live_state("1111111111111111", 1);
    let newest = live_state("2222222222222222", 2);

    fixture.publish(&oldest, 24, true).unwrap();
    fixture.publish(&newest, 24, true).unwrap();
    let history = fixture.0.join("docs/history");
    let index_before = fs::read(history.join("index.json")).unwrap();
    let mut config: sourcefield_core::Config = toml::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/profile.toml"
    )))
    .unwrap();

    config.collection.history_limit = 1;
    let config_path = fixture.0.join("profile.toml");
    let snapshot_path = fixture.0.join("offline-snapshot.json");
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T00:00:00Z".into(),
        ..Snapshot::default()
    };

    fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(&snapshot_path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let options = GenerateOptions {
        root: &fixture.0,
        config_path: &config_path,
        fallback_snapshot_path: &snapshot_path,
        assets_dir: &fixture.0.join("assets"),
        docs_dir: &fixture.0.join("docs"),
        readme_paths: &[],
        offline: false,
        locked: false,
        runtime: None,
        adopt_existing: false,
        strict_live: false,
        private_counts: false,
        no_history: true,
    };

    // Act
    let result =
        generate_with_collection(options, CollectionAccess::default(), async |_, _, _, _| {
            Ok(snapshot)
        })
        .await;

    // Assert
    assert!(
        result.is_ok(),
        "live generation with disabled history must succeed"
    );
    assert!(
        fs::read(history.join("index.json")).unwrap() == index_before,
        "disabled live history must preserve the original index"
    );
    assert_eq!(fs::read_dir(&history).unwrap().count(), 3);
    assert!(history.join("1111111111111111.json").exists());
    assert!(history.join("2222222222222222.json").exists());
}

#[test]
fn allowed_live_history_applies_reduced_retention_to_an_already_known_hash() {
    // Arrange
    let fixture = Fixture::new();
    let oldest = live_state("1111111111111111", 1);
    let newest = live_state("2222222222222222", 2);

    fixture.publish(&oldest, 24, true).unwrap();
    fixture.publish(&newest, 24, true).unwrap();
    let history = fixture.0.join("docs/history");
    let newest_before = fs::read(history.join("2222222222222222.json")).unwrap();

    // Act
    let result = fixture.publish(&newest, 1, true);

    // Assert
    assert!(result.is_ok(), "allowed live retention must succeed");
    let index = load_history(&history).unwrap();

    assert_eq!(index.states.len(), 1);
    assert_eq!(index.states[0].hash, newest.semantic_hash);
    assert!(!history.join("1111111111111111.json").exists());
    assert!(
        fs::read(history.join("2222222222222222.json")).unwrap() == newest_before,
        "new archive bytes must be preserved"
    );
}
