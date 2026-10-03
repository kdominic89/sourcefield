//! CLI boundary tests use one arrange, act, assert sequence per behavior.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Each test owns its filesystem to avoid accidental coupling through parallel execution.
struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "sourcefield-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir(&path).unwrap();

        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

/// Build synthetic canonical data independently of production account information.
fn state() -> ProfileState {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = load_config(root.join("config/profile.toml")).unwrap();
    let snapshot: Snapshot = read_json(&root.join("config/offline-snapshot.json")).unwrap();

    build_state(&config, &snapshot, "2026-09-05T12:00:00Z").unwrap()
}

#[test]
fn readme_update_preserves_authored_prose() {
    let text =
        "intro\n<!-- sourcefield:projects:start -->old<!-- sourcefield:projects:end -->\nfooter";

    let updated = replace_section(text, "projects", "new").unwrap();

    assert_eq!(updated, "intro\nnew\nfooter");
}

#[test]
fn duplicate_readme_markers_are_rejected() {
    let text = "<!-- sourcefield:projects:start --><!-- sourcefield:projects:start --><!-- sourcefield:projects:end -->";

    let result = replace_section(text, "projects", "new");

    assert!(result.is_err());
}

#[test]
fn reversed_readme_markers_are_rejected() {
    let text = "<!-- sourcefield:projects:end --><!-- sourcefield:projects:start -->";

    let result = replace_section(text, "projects", "new");

    assert!(result.is_err());
}

#[test]
fn strict_live_accepts_complete_observation() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    assert!(result.is_ok());
}

#[test]
fn strict_live_rejects_incomplete_observation() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    assert!(result.is_err());
}

#[test]
fn strict_live_rejects_warnings() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        warnings: vec!["missing source".into()],
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    assert!(result.is_err());
}

#[test]
fn identical_state_preserves_timestamp() {
    let directory = TemporaryDirectory::new();
    let original = state();
    let path = directory.0.join("state.json");
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let mut current = original.clone();
    current.generated_at = "2026-09-06T12:00:00Z".into();

    preserve_generation_time(&mut current, &path).unwrap();

    assert_eq!(current.generated_at, original.generated_at);
}

#[test]
fn legacy_state_requires_explicit_migration() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("state.json");
    fs::write(&path, "{\"schema\":2}").unwrap();
    let mut current = state();

    let result = preserve_generation_time(&mut current, &path);

    assert!(result.is_err());
}

/// Install history through the production candidate and ownership transaction.
fn publish_history(
    root: &Path,
    current: &ProfileState,
    limit: usize,
) -> Result<sourcefield_workspace::CommitReport> {
    let mut transaction = Transaction::begin(root)?;

    stage_history(
        &mut transaction,
        root,
        Path::new("docs"),
        current,
        limit,
        true,
        false,
    )?;
    transaction.validate()?;

    transaction.commit()
}

#[test]
fn repeated_history_preserves_original_archive_and_index_bytes() {
    let directory = TemporaryDirectory::new();
    let mut current = state();
    publish_history(&directory.0, &current, 2).unwrap();
    let index_path = directory.0.join("docs/history/index.json");
    let archive_path = directory.0.join("docs/history").join(format!(
        "{}.json",
        current.semantic_hash.to_ascii_lowercase()
    ));

    let original_index = fs::read(&index_path).unwrap();
    let original_archive = fs::read(&archive_path).unwrap();
    current.generated_at = "2026-09-06T12:00:00Z".into();

    publish_history(&directory.0, &current, 2).unwrap();

    assert_eq!(fs::read(index_path).unwrap(), original_index);
    assert_eq!(fs::read(archive_path).unwrap(), original_archive);
}

#[test]
fn corrupted_index_does_not_change_archives() {
    let directory = TemporaryDirectory::new();
    let history = directory.0.join("docs/history");
    fs::create_dir_all(&history).unwrap();
    fs::write(history.join("index.json"), "{broken").unwrap();
    fs::write(history.join("unrelated.json"), "preserve").unwrap();

    let result = publish_history(&directory.0, &state(), 1);

    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(history.join("index.json")).unwrap(),
        "{broken"
    );
    assert_eq!(
        fs::read_to_string(history.join("unrelated.json")).unwrap(),
        "preserve"
    );
    assert!(!directory.0.join(".sourcefield-owned.json").exists());
}

#[test]
fn retention_removes_only_expired_owned_archives() {
    let directory = TemporaryDirectory::new();
    let mut oldest = state();
    oldest.semantic_hash = "1111111111111111".into();
    oldest.generated_at = "2026-09-01T12:00:00Z".into();
    publish_history(&directory.0, &oldest, 2).unwrap();
    let mut retained = oldest.clone();
    retained.semantic_hash = "2222222222222222".into();
    retained.generated_at = "2026-09-02T12:00:00Z".into();
    publish_history(&directory.0, &retained, 2).unwrap();
    let history = directory.0.join("docs/history");
    fs::write(history.join("unindexed.json"), "authored recovery evidence").unwrap();
    let mut newest = retained.clone();
    newest.semantic_hash = "3333333333333333".into();
    newest.generated_at = "2026-09-03T12:00:00Z".into();

    let report = publish_history(&directory.0, &newest, 2).unwrap();

    let index = load_history(&history).unwrap();
    assert_eq!(report.removed, 1);
    assert_eq!(
        index
            .states
            .iter()
            .map(|entry| entry.hash.as_str())
            .collect::<Vec<_>>(),
        vec!["3333333333333333", "2222222222222222"]
    );
    assert!(!history.join("1111111111111111.json").exists());
    assert!(history.join("2222222222222222.json").exists());
    assert!(history.join("3333333333333333.json").exists());
    assert_eq!(
        fs::read_to_string(history.join("unindexed.json")).unwrap(),
        "authored recovery evidence"
    );
}

#[test]
fn corrupt_retained_archive_prevents_new_history_publication() {
    let directory = TemporaryDirectory::new();
    let mut current = state();
    publish_history(&directory.0, &current, 2).unwrap();
    let history = directory.0.join("docs/history");
    let index_before = fs::read(history.join("index.json")).unwrap();
    let manifest_before = fs::read(directory.0.join(".sourcefield-owned.json")).unwrap();
    let archive = history.join(format!(
        "{}.json",
        current.semantic_hash.to_ascii_lowercase()
    ));

    fs::write(&archive, b"{invalid archive").unwrap();
    current.semantic_hash = "4444444444444444".into();

    let result = publish_history(&directory.0, &current, 2);

    assert!(result.is_err());
    assert_eq!(fs::read(archive).unwrap(), b"{invalid archive");
    assert_eq!(fs::read(history.join("index.json")).unwrap(), index_before);
    assert_eq!(
        fs::read(directory.0.join(".sourcefield-owned.json")).unwrap(),
        manifest_before
    );
    assert!(!history.join("4444444444444444.json").exists());
}

#[test]
fn history_traversal_is_rejected() {
    let name = "../0123456789abcdef.json";

    let result = validate_history_name("0123456789ABCDEF", name);

    assert!(result.is_err());
}

#[test]
fn canonical_svg_is_accepted() {
    let current = state();
    let svg = render_svg(&current, Theme::Dark, true);

    let result = validate_svg(&svg, &current, Theme::Dark, true);

    assert!(result.is_ok());
}

#[test]
fn injected_svg_is_rejected() {
    let current = state();
    let svg =
        render_svg(&current, Theme::Dark, true).replacen("<svg", "<svg onload=\"alert(1)\"", 1);

    let result = validate_svg(&svg, &current, Theme::Dark, true);

    assert!(result.is_err());
}

#[test]
fn missing_private_authorization_clears_cached_count() {
    let mut snapshot = Snapshot {
        private_repository_count: Some(27),
        ..Snapshot::default()
    };

    apply_private_count_policy(&mut snapshot, false, true);

    assert_eq!(snapshot.private_repository_count, None);
}

#[test]
fn explicit_private_authorization_retains_count() {
    let mut snapshot = Snapshot {
        private_repository_count: Some(27),
        ..Snapshot::default()
    };

    apply_private_count_policy(&mut snapshot, true, true);

    assert_eq!(snapshot.private_repository_count, Some(27));
}

#[test]
fn output_outside_root_is_rejected() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("../README.md");

    let result = relative_output(&directory.0, &path);

    assert!(result.is_err());
}

/// Execute preview/replay using isolated outputs and the synthetic checked-in configuration.
async fn preview(directory: &Path, locked: bool, readmes: &[PathBuf]) -> Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = directory.join("profile.toml");

    if !config.exists() {
        fs::copy(fixtures.join("config/profile.toml"), &config)?;
    }

    generate(GenerateOptions {
        root: directory,
        config_path: &config,
        fallback_snapshot_path: &fixtures.join("config/offline-snapshot.json"),
        assets_dir: &directory.join("assets"),
        docs_dir: &directory.join("docs"),
        readme_paths: readmes,
        offline: true,
        locked,
        runtime: None,
        adopt_existing: false,
        strict_live: false,
        private_counts: false,
        no_history: false,
    })
    .await
}

#[tokio::test]
async fn locked_offline_replay_preserves_all_owned_bytes() {
    let directory = TemporaryDirectory::new();
    preview(&directory.0, false, &[]).await.unwrap();
    let before = fs::read(directory.0.join(".sourcefield-owned.json")).unwrap();

    preview(&directory.0, true, &[]).await.unwrap();

    assert_eq!(
        fs::read(directory.0.join(".sourcefield-owned.json")).unwrap(),
        before
    );
    assert!(directory.0.join("docs/app.js").is_file());
    assert!(directory.0.join("docs/simulation-fallback.js").is_file());
}

#[tokio::test]
async fn invalid_readme_leaves_previous_generation_intact() {
    let directory = TemporaryDirectory::new();
    preview(&directory.0, false, &[]).await.unwrap();
    let before = fs::read(directory.0.join(".sourcefield-owned.json")).unwrap();
    let readme = directory.0.join("README.md");
    fs::write(&readme, "authored prose with no markers").unwrap();

    let result = preview(&directory.0, false, std::slice::from_ref(&readme)).await;

    assert!(result.is_err());
    assert_eq!(
        fs::read(directory.0.join(".sourcefield-owned.json")).unwrap(),
        before
    );
    assert_eq!(
        fs::read_to_string(readme).unwrap(),
        "authored prose with no markers"
    );
}

#[tokio::test]
async fn locked_replay_rejects_authored_configuration_drift() {
    let directory = TemporaryDirectory::new();
    preview(&directory.0, false, &[]).await.unwrap();
    let config = directory.0.join("profile.toml");
    let before = fs::read(directory.0.join(".sourcefield-owned.json")).unwrap();
    let mut text = fs::read_to_string(&config).unwrap();
    text.push_str("\n# changed authored input\n");
    fs::write(&config, text).unwrap();

    let result = preview(&directory.0, true, &[]).await;

    assert!(result.is_err());
    assert_eq!(
        fs::read(directory.0.join(".sourcefield-owned.json")).unwrap(),
        before
    );
}

#[tokio::test]
async fn omitted_readme_retains_authored_and_managed_text() {
    let directory = TemporaryDirectory::new();
    let readme = directory.0.join("README.md");
    fs::write(&readme, "Intro\n<!-- sourcefield:projects:start --><!-- sourcefield:projects:end -->\n<!-- sourcefield:packages:start --><!-- sourcefield:packages:end -->\nFooter").unwrap();
    preview(&directory.0, false, std::slice::from_ref(&readme))
        .await
        .unwrap();
    let original = fs::read(&readme).unwrap();

    preview(&directory.0, false, &[]).await.unwrap();

    assert_eq!(fs::read(readme).unwrap(), original);
}

#[tokio::test]
async fn replay_preserves_previously_approved_private_aggregate_without_credentials() {
    let directory = TemporaryDirectory::new();
    preview(&directory.0, false, &[]).await.unwrap();
    let assets = directory.0.join("assets");
    let config: sourcefield_core::Config = read_json(&assets.join("resolved-config.json")).unwrap();
    let mut snapshot: Snapshot = read_json(&assets.join("source-snapshot.json")).unwrap();
    snapshot.private_repository_count = Some(27);
    let state = build_state(&config, &snapshot, "1970-01-01T00:00:00Z").unwrap();
    fs::write(
        assets.join("source-snapshot.json"),
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();
    fs::write(
        assets.join("profile-state.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
    // Model an earlier approved publication including its matching capture and ownership digests.
    let mut record: GenerationRecord = read_json(&assets.join("generation-record.json")).unwrap();
    for name in REPLAY_FILES {
        record
            .inputs
            .insert((*name).into(), file_sha256(assets.join(name)).unwrap());
    }
    fs::write(
        assets.join("generation-record.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    let manifest_path = directory.0.join(".sourcefield-owned.json");
    let mut ownership: serde_json::Value = read_json(&manifest_path).unwrap();
    for name in [
        "source-snapshot.json",
        "profile-state.json",
        "generation-record.json",
    ] {
        ownership["files"][format!("assets/{name}")] =
            serde_json::json!(file_sha256(assets.join(name)).unwrap());
    }
    fs::write(
        manifest_path,
        serde_json::to_vec_pretty(&ownership).unwrap(),
    )
    .unwrap();

    preview(&directory.0, true, &[]).await.unwrap();

    let replayed: Snapshot = read_json(&assets.join("source-snapshot.json")).unwrap();
    assert_eq!(replayed.private_repository_count, Some(27));
}

/// Read actual generated bytes instead of trusting stored manifest digests as evidence.
fn actual_owned_digests(root: &Path) -> BTreeMap<String, String> {
    let manifest_path = root.join(".sourcefield-owned.json");
    let manifest: sourcefield_workspace::OwnershipManifest = read_json(&manifest_path).unwrap();
    let mut digests = BTreeMap::new();

    for path in manifest.files.keys().chain(manifest.authored_files.keys()) {
        digests.insert(path.clone(), file_sha256(root.join(path)).unwrap());
    }

    digests.insert(
        ".sourcefield-owned.json".into(),
        file_sha256(manifest_path).unwrap(),
    );

    digests
}

#[tokio::test]
async fn locked_replay_rejects_changed_generator_fingerprint_without_writes() {
    let directory = TemporaryDirectory::new();
    preview(&directory.0, false, &[]).await.unwrap();
    let record_path = directory.0.join("assets/generation-record.json");
    let mut record: GenerationRecord = read_json(&record_path).unwrap();
    record.generator_fingerprint = "different-generator-source-fingerprint".into();
    fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let before = actual_owned_digests(&directory.0);

    let result = preview(&directory.0, true, &[]).await;

    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("recorded generator revision"),
        "unexpected rejection: {message}"
    );
    assert_eq!(actual_owned_digests(&directory.0), before);
    assert!(!directory.0.join(".sourcefield-transaction").exists());
    assert!(!directory.0.join(".sourcefield-lock").exists());
}

#[test]
fn unchanged_live_observation_retains_original_collection_time() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("snapshot.json");
    let original = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T00:00:00Z".into(),
        ..Snapshot::default()
    };
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let mut refreshed = original.clone();
    refreshed.fetched_at = "2026-10-02T00:00:00Z".into();

    let retained = preserve_observation_time(&refreshed, &path).unwrap();

    assert_eq!(
        serde_json::to_value(&retained).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
}

#[test]
fn changed_observation_keeps_new_collection_time() {
    let directory = TemporaryDirectory::new();
    let path = directory.0.join("snapshot.json");
    let original = Snapshot {
        mode: SnapshotMode::Live,
        fetched_at: "2026-10-01T00:00:00Z".into(),
        ..Snapshot::default()
    };
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    let mut refreshed = original;
    refreshed.fetched_at = "2026-10-02T00:00:00Z".into();
    refreshed.warnings.push("source changed".into());

    let retained = preserve_observation_time(&refreshed, &path).unwrap();

    assert_eq!(retained.fetched_at, "2026-10-02T00:00:00Z");
}

#[test]
fn permissive_online_mode_accepts_dated_fallback() {
    let mode = ExecutionMode::from_flags(false, false, false).unwrap();
    let snapshot = Snapshot {
        mode: SnapshotMode::Fallback,
        warnings: vec!["dated fallback".into()],
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, mode.is_strict());

    assert!(result.is_ok());
}

#[test]
fn strict_online_mode_rejects_dated_fallback() {
    let mode = ExecutionMode::from_flags(false, false, true).unwrap();
    let snapshot = Snapshot {
        mode: SnapshotMode::Fallback,
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, mode.is_strict());

    assert!(result.is_err());
}

#[test]
fn generated_live_history_round_trips_producer_metadata() {
    let directory = TemporaryDirectory::new();
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = load_config(fixture_root.join("config/profile.toml")).unwrap();
    let mut snapshot: Snapshot =
        read_json(&fixture_root.join("config/offline-snapshot.json")).unwrap();
    snapshot.mode = SnapshotMode::Live;
    snapshot.warnings.clear();
    let current = build_state(&config, &snapshot, "2026-10-03T12:00:00Z").unwrap();

    publish_history(&directory.0, &current, 256).unwrap();
    let index = load_history(&directory.0.join("docs/history")).unwrap();

    // An explicit test-only export lets the browser consume the exact production writer output.
    if let Some(output) = env::var_os("SOURCEFIELD_TEST_HISTORY_OUTPUT") {
        let output = PathBuf::from(output);
        fs::create_dir_all(&output).unwrap();
        fs::copy(
            directory.0.join("docs/history/index.json"),
            output.join("index.json"),
        )
        .unwrap();
        fs::copy(
            directory.0.join("docs/history").join(&index.states[0].file),
            output.join(&index.states[0].file),
        )
        .unwrap();
    }
    assert_eq!(index.states.len(), 1);
    assert_eq!(index.states[0].hash, current.semantic_hash);
    assert!(
        current
            .semantic_hash
            .chars()
            .any(|value| matches!(value, 'A'..='F'))
    );
    assert_eq!(
        index.states[0].file,
        format!("{}.json", current.semantic_hash.to_ascii_lowercase())
    );
}

/// A real generation transaction with only the remote collection boundary replaced.
async fn refresh_fixture(directory: &Path, strict: bool, observed: Result<Snapshot>) -> Result<()> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = directory.join("profile.toml");
    if !config_path.exists() {
        let mut config = load_config(fixtures.join("config/profile.toml"))?;
        config.collection.collect_private_repository_count = false;
        fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    }

    generate_with_collection(
        GenerateOptions {
            root: directory,
            config_path: &config_path,
            fallback_snapshot_path: &fixtures.join("config/offline-snapshot.json"),
            assets_dir: &directory.join("assets"),
            docs_dir: &directory.join("docs"),
            readme_paths: &[],
            offline: false,
            locked: false,
            runtime: None,
            adopt_existing: false,
            strict_live: strict,
            private_counts: false,
            no_history: false,
        },
        CollectionAccess::default(),
        async move |_, _, _, _| observed,
    )
    .await
}

/// Complete synthetic live observation, independent of network or personal account data.
fn live_observation() -> Snapshot {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut snapshot: Snapshot = read_json(&fixtures.join("config/offline-snapshot.json")).unwrap();
    snapshot.mode = SnapshotMode::Live;
    snapshot.fetched_at = "2026-10-01T00:00:00Z".into();
    snapshot.warnings.clear();
    for source in &mut snapshot.sources {
        source.status = sourcefield_core::DataStatus::Live;
    }

    snapshot
}

#[tokio::test]
async fn repeated_live_generation_preserves_every_owned_byte_and_provenance() {
    let directory = TemporaryDirectory::new();
    let initial = live_observation();
    refresh_fixture(&directory.0, true, Ok(initial.clone()))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);
    let mut refreshed = initial;
    refreshed.fetched_at = "2026-10-02T00:00:00Z".into();

    refresh_fixture(&directory.0, true, Ok(refreshed))
        .await
        .unwrap();

    assert_eq!(actual_owned_digests(&directory.0), before);
}

#[tokio::test]
async fn changed_live_generation_updates_capture_and_provenance() {
    let directory = TemporaryDirectory::new();
    let initial = live_observation();
    refresh_fixture(&directory.0, true, Ok(initial.clone()))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);
    let mut refreshed = initial;
    refreshed.fetched_at = "2026-10-02T00:00:00Z".into();
    refreshed.user.followers += 1;

    refresh_fixture(&directory.0, true, Ok(refreshed))
        .await
        .unwrap();

    let after = actual_owned_digests(&directory.0);
    assert_ne!(
        after["assets/source-snapshot.json"],
        before["assets/source-snapshot.json"]
    );
    assert_ne!(
        after["assets/generation-record.json"],
        before["assets/generation-record.json"]
    );
    let capture: Snapshot = read_json(&directory.0.join("assets/source-snapshot.json")).unwrap();
    assert_eq!(capture.fetched_at, "2026-10-02T00:00:00Z");
}

#[tokio::test]
async fn permissive_online_failure_publishes_only_dated_fallback() {
    let directory = TemporaryDirectory::new();
    refresh_fixture(&directory.0, true, Ok(live_observation()))
        .await
        .unwrap();

    refresh_fixture(
        &directory.0,
        false,
        Err(anyhow::anyhow!("upstream unavailable")),
    )
    .await
    .unwrap();

    let capture: Snapshot = read_json(&directory.0.join("assets/source-snapshot.json")).unwrap();
    assert_eq!(capture.mode, SnapshotMode::Fallback);
    assert_eq!(capture.fetched_at, "2026-10-01T00:00:00Z");
}

#[tokio::test]
async fn strict_collection_error_preserves_every_owned_file() {
    let directory = TemporaryDirectory::new();
    refresh_fixture(&directory.0, true, Ok(live_observation()))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);

    let result = refresh_fixture(
        &directory.0,
        true,
        Err(anyhow::anyhow!("upstream unavailable")),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(actual_owned_digests(&directory.0), before);
}

#[tokio::test]
async fn strict_partial_observation_preserves_every_owned_file() {
    let directory = TemporaryDirectory::new();
    let mut partial = live_observation();
    refresh_fixture(&directory.0, true, Ok(partial.clone()))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);
    partial.mode = SnapshotMode::Partial;

    let result = refresh_fixture(&directory.0, true, Ok(partial)).await;

    assert!(result.is_err());
    assert_eq!(actual_owned_digests(&directory.0), before);
}

#[tokio::test]
async fn strict_warning_preserves_every_owned_file() {
    let directory = TemporaryDirectory::new();
    let mut warned = live_observation();
    refresh_fixture(&directory.0, true, Ok(warned.clone()))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);
    warned.warnings.push("source incomplete".into());

    let result = refresh_fixture(&directory.0, true, Ok(warned)).await;

    assert!(result.is_err());
    assert_eq!(actual_owned_digests(&directory.0), before);
}

#[test]
fn maximum_history_rollover_retains_256_valid_owned_archives() {
    let directory = TemporaryDirectory::new();
    let mut transaction = Transaction::begin(&directory.0).unwrap();
    let mut index = HistoryIndex::default();
    let mut current = state();
    for ordinal in 0..sourcefield_core::MAX_HISTORY_ENTRIES {
        current.semantic_hash = format!("{ordinal:016X}");
        current.generated_at = (chrono::DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .unwrap()
            + chrono::Duration::seconds(ordinal as i64))
        .to_rfc3339();
        let file = format!("{}.json", current.semantic_hash.to_ascii_lowercase());
        transaction
            .stage(
                Path::new("docs/history").join(&file),
                &serde_json::to_vec(&current).unwrap(),
            )
            .unwrap();
        index.states.push(HistoryEntry {
            hash: current.semantic_hash.clone(),
            generated_at: current.generated_at.clone(),
            file,
            node_count: current.nodes.len(),
            edge_count: current.edges.len(),
        });
    }

    transaction
        .stage(
            "docs/history/index.json",
            &serde_json::to_vec(&index).unwrap(),
        )
        .unwrap();
    transaction.validate().unwrap();
    transaction.commit().unwrap();
    current.semantic_hash = "FFFFFFFFFFFFFFFF".into();
    current.generated_at = "2026-10-01T00:00:00Z".into();

    let report = publish_history(
        &directory.0,
        &current,
        sourcefield_core::MAX_HISTORY_ENTRIES,
    )
    .unwrap();

    let retained = load_history(&directory.0.join("docs/history")).unwrap();
    assert_eq!(retained.states.len(), 256);
    assert_eq!(report.removed, 1);
    assert!(
        !directory
            .0
            .join("docs/history/0000000000000000.json")
            .exists()
    );
    assert!(
        directory
            .0
            .join("docs/history/ffffffffffffffff.json")
            .exists()
    );
}

/// Exercise opt-in precedence and disclosure through the real generation transaction.
async fn access_fixture(
    directory: &Path,
    config_request: bool,
    cli_request: bool,
    environment_request: bool,
    credential: bool,
) -> (bool, Snapshot) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config_path = directory.join("profile.toml");
    let mut config = load_config(fixtures.join("config/profile.toml")).unwrap();
    config.collection.collect_private_repository_count = config_request;
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    let observed_request = std::cell::Cell::new(false);
    let access = CollectionAccess {
        public_token: None,
        profile_token: credential.then(|| "test-only-token".into()),
        private_from_environment: environment_request,
    };

    generate_with_collection(
        GenerateOptions {
            root: directory,
            config_path: &config_path,
            fallback_snapshot_path: &fixtures.join("config/offline-snapshot.json"),
            assets_dir: &directory.join("assets"),
            docs_dir: &directory.join("docs"),
            readme_paths: &[],
            offline: false,
            locked: false,
            runtime: None,
            adopt_existing: false,
            strict_live: false,
            private_counts: cli_request,
            no_history: false,
        },
        access,
        async |_, requested, _, _| {
            observed_request.set(requested);
            let mut snapshot = live_observation();
            snapshot.private_repository_count = Some(27);

            Ok(snapshot)
        },
    )
    .await
    .unwrap();

    (
        observed_request.get(),
        read_json(&directory.join("assets/source-snapshot.json")).unwrap(),
    )
}

#[tokio::test]
async fn online_cli_private_opt_in_reaches_collection_and_capture() {
    let directory = TemporaryDirectory::new();

    let (requested, snapshot) = access_fixture(&directory.0, false, true, false, true).await;

    assert!(requested);
    assert_eq!(snapshot.private_repository_count, Some(27));
}

#[tokio::test]
async fn online_config_private_opt_in_reaches_collection_and_capture() {
    let directory = TemporaryDirectory::new();

    let (requested, snapshot) = access_fixture(&directory.0, true, false, false, true).await;

    assert!(requested);
    assert_eq!(snapshot.private_repository_count, Some(27));
}

#[tokio::test]
async fn online_environment_private_opt_in_reaches_collection_and_capture() {
    let directory = TemporaryDirectory::new();

    let (requested, snapshot) = access_fixture(&directory.0, false, false, true, true).await;

    assert!(requested);
    assert_eq!(snapshot.private_repository_count, Some(27));
}

#[tokio::test]
async fn online_credentials_without_intent_cannot_publish_private_count() {
    let directory = TemporaryDirectory::new();

    let (requested, snapshot) = access_fixture(&directory.0, false, false, false, true).await;

    assert!(!requested);
    assert_eq!(snapshot.private_repository_count, None);
}

#[tokio::test]
async fn online_intent_without_credentials_cannot_publish_private_count() {
    let directory = TemporaryDirectory::new();

    let (requested, snapshot) = access_fixture(&directory.0, true, true, true, false).await;

    assert!(!requested);
    assert_eq!(snapshot.private_repository_count, None);
    assert!(
        snapshot
            .warnings
            .iter()
            .any(|warning| warning.contains("PROFILE_TOKEN"))
    );
}

#[test]
fn strict_diagnostic_names_failed_source() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        sources: vec![sourcefield_core::SourceStatus {
            source: "github:repositories".into(),
            status: sourcefield_core::DataStatus::Missing,
        }],
        warnings: vec!["Repository inventory reached its collection or pagination limit".into()],
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    let message = result.unwrap_err().to_string();
    assert!(message.contains("github:repositories"), "{message}");
    assert!(message.contains("repository_limit"), "{message}");
}

#[tokio::test]
async fn repeated_fallback_preserves_every_owned_byte_and_provenance() {
    let directory = TemporaryDirectory::new();
    refresh_fixture(&directory.0, true, Ok(live_observation()))
        .await
        .unwrap();
    refresh_fixture(&directory.0, false, Err(anyhow::anyhow!("first outage")))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);

    refresh_fixture(
        &directory.0,
        false,
        Err(anyhow::anyhow!("continuing outage")),
    )
    .await
    .unwrap();

    assert_eq!(actual_owned_digests(&directory.0), before);
}

/// Authored synthetic configuration used to authorize diagnostic source identities.
fn fixture_config() -> sourcefield_core::Config {
    load_config(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/profile.toml")).unwrap()
}

#[test]
fn strict_diagnostic_does_not_echo_untrusted_warning_or_source() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        sources: vec![sourcefield_core::SourceStatus {
            source: "github:org:never-log-secret\nforged-log".into(),
            status: sourcefield_core::DataStatus::Missing,
        }],
        warnings: vec!["Authorization: Bearer never-log-secret\nforged-log".into()],
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    let message = result.unwrap_err().to_string();
    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("never-log-secret"));
    assert!(!message.contains("forged-log"));
    assert!(!message.contains('\n'));
}

#[test]
fn strict_diagnostic_names_missing_private_credential_without_value() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        warnings: vec![observations::PRIVATE_COUNT_WARNING.into()],
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    assert!(result.unwrap_err().to_string().contains("PROFILE_TOKEN"));
}

#[test]
fn strict_live_rejects_inconsistent_live_mode_with_missing_source() {
    let snapshot = Snapshot {
        mode: SnapshotMode::Live,
        sources: vec![sourcefield_core::SourceStatus {
            source: "github:repositories".into(),
            status: sourcefield_core::DataStatus::Missing,
        }],
        ..Snapshot::default()
    };

    let result = require_live(&fixture_config(), &snapshot, true);

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("github:repositories")
    );
}

#[test]
fn owned_warning_normalization_preserves_unrelated_diagnostics() {
    let mut snapshot = Snapshot {
        warnings: vec![
            "keep first".into(),
            observations::FALLBACK_WARNING.into(),
            "keep second".into(),
            observations::FALLBACK_WARNING.into(),
        ],
        ..Snapshot::default()
    };

    observations::add_warning(&mut snapshot, observations::FALLBACK_WARNING);

    assert_eq!(
        snapshot.warnings,
        vec!["keep first", observations::FALLBACK_WARNING, "keep second"]
    );
}

#[tokio::test]
async fn new_live_observation_after_fallback_updates_capture() {
    let directory = TemporaryDirectory::new();
    refresh_fixture(&directory.0, true, Ok(live_observation()))
        .await
        .unwrap();
    refresh_fixture(&directory.0, false, Err(anyhow::anyhow!("outage")))
        .await
        .unwrap();
    let before = actual_owned_digests(&directory.0);
    let mut observed = live_observation();
    observed.fetched_at = "2026-10-04T00:00:00Z".into();

    refresh_fixture(&directory.0, true, Ok(observed))
        .await
        .unwrap();

    let capture: Snapshot = read_json(&directory.0.join("assets/source-snapshot.json")).unwrap();
    assert_eq!(capture.mode, SnapshotMode::Live);
    assert_eq!(capture.fetched_at, "2026-10-04T00:00:00Z");
    assert!(capture.warnings.is_empty());
    assert_ne!(actual_owned_digests(&directory.0), before);
}

#[tokio::test]
async fn first_permissive_refresh_never_promotes_preview_package_values() {
    let directory = TemporaryDirectory::new();
    let config = fixture_config();
    let package = &config.publications[0].packages[0].id;
    let mut observed = live_observation();
    observed.mode = SnapshotMode::Partial;
    observed.packages.clear();
    observed.sources.push(sourcefield_core::SourceStatus {
        source: format!("nuget:{}", package.to_ascii_lowercase()),
        status: sourcefield_core::DataStatus::Missing,
    });

    refresh_fixture(&directory.0, false, Ok(observed))
        .await
        .unwrap();

    let capture: Snapshot = read_json(&directory.0.join("assets/source-snapshot.json")).unwrap();
    assert!(capture.packages.is_empty());
    assert_eq!(capture.mode, SnapshotMode::Partial);
    assert!(
        !capture
            .sources
            .iter()
            .any(|source| source.status == sourcefield_core::DataStatus::Fallback)
    );
}

#[tokio::test]
async fn permissive_refresh_restores_real_dated_package_observations() {
    let directory = TemporaryDirectory::new();
    let config = fixture_config();
    let package = &config.publications[0].packages[0].id;
    let initial = live_observation();
    let expected = initial
        .packages
        .iter()
        .find(|entry| &entry.id == package)
        .unwrap()
        .clone();
    refresh_fixture(&directory.0, true, Ok(initial.clone()))
        .await
        .unwrap();
    let mut observed = initial;
    observed.mode = SnapshotMode::Partial;
    observed.packages.clear();
    let key = format!("nuget:{}", package.to_ascii_lowercase());
    observed.sources.retain(|source| source.source != key);
    observed.sources.push(sourcefield_core::SourceStatus {
        source: key.clone(),
        status: sourcefield_core::DataStatus::Missing,
    });

    refresh_fixture(&directory.0, false, Ok(observed))
        .await
        .unwrap();

    let capture: Snapshot = read_json(&directory.0.join("assets/source-snapshot.json")).unwrap();
    let actual = capture
        .packages
        .iter()
        .find(|entry| &entry.id == package)
        .unwrap();
    assert_eq!(actual.version, expected.version);
    assert_eq!(actual.total_downloads, expected.total_downloads);
    assert!(
        capture.sources.iter().any(|source| source.source == key
            && source.status == sourcefield_core::DataStatus::Fallback)
    );
}

#[test]
fn third_review_mixed_case_owner_is_authorized() {
    let mut config = fixture_config();
    config.publications[0].owner = Some("Third-Review-Owner".into());
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        sources: vec![sourcefield_core::SourceStatus {
            source: "nuget:owner:third-review-owner".into(),
            status: sourcefield_core::DataStatus::Missing,
        }],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(
        message.contains("nuget:owner:third-review-owner=Missing"),
        "{message}"
    );
    assert!(!message.contains("withheld"), "{message}");
}

#[test]
fn third_review_known_source_warning_is_actionable() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["Source unavailable: github:repositories".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("github:repositories"), "{message}");
    assert!(!message.contains("withheld"), "{message}");
}

#[test]
fn third_review_known_package_match_warning_is_actionable() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec![format!(
            "NuGet returned no exact match for {}",
            config.publications[0].packages[0].id
        )],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("no exact match"), "{message}");
    assert!(!message.contains("withheld"), "{message}");
}

#[test]
fn third_review_known_package_metadata_warning_is_actionable() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec![format!(
            "NuGet metadata unavailable: {}",
            config.publications[0].packages[0].id
        )],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("metadata unavailable"), "{message}");
    assert!(!message.contains("withheld"), "{message}");
}

#[test]
fn third_review_unconfigured_source_warning_stays_redacted() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["Source unavailable: github:org:never-log-secret".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("never-log-secret"));
    assert!(!message.contains('\n'));
}

#[test]
fn third_review_unconfigured_package_warning_stays_redacted() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["NuGet returned no exact match for never-log-secret".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("never-log-secret"));
    assert!(!message.contains('\n'));
}

#[test]
fn third_review_forged_source_warning_stays_redacted() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["Source unavailable: github:repositories\nnever-log-secret".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("never-log-secret"));
    assert!(!message.contains('\n'));
}

#[test]
fn third_review_forged_metadata_warning_stays_redacted() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec![format!(
            "NuGet metadata unavailable: {}\nnever-log-secret",
            config.publications[0].packages[0].id
        )],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("never-log-secret"));
    assert!(!message.contains('\n'));
}

#[test]
fn third_review_unconfigured_owner_remains_redacted() {
    let mut config = fixture_config();
    config.publications[0].owner = Some("Third-Review-Owner".into());
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        sources: vec![sourcefield_core::SourceStatus {
            source: "nuget:owner:third-review-owner-secret".into(),
            status: sourcefield_core::DataStatus::Missing,
        }],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("third-review-owner-secret"));
}

#[test]
fn third_review_service_source_cannot_authorize_package_match() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["NuGet returned no exact match for service-index".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("service-index"));
}

#[test]
fn third_review_service_source_cannot_authorize_package_metadata() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["NuGet metadata unavailable: service-index".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(message.contains("withheld"), "{message}");
    assert!(!message.contains("service-index"));
}

#[test]
fn third_review_service_index_source_warning_is_actionable() {
    let config = fixture_config();
    let snapshot = Snapshot {
        mode: SnapshotMode::Partial,
        warnings: vec!["Source unavailable: nuget:service-index".into()],
        ..Snapshot::default()
    };

    let message = require_live(&config, &snapshot, true)
        .unwrap_err()
        .to_string();

    assert!(
        message.contains("Source unavailable: nuget:service-index"),
        "{message}"
    );
    assert!(!message.contains("withheld"), "{message}");
}
