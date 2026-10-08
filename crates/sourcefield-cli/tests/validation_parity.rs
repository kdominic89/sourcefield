//! Bind validation to authored/captured inputs and preserve disabled history through the public CLI.

use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use sha2::Digest;
use sourcefield_core::{
    Config, OrganizationImport, OrganizationSource, ProfileState, Snapshot, SnapshotMode,
    build_state,
};
use sourcefield_render::{Theme, render_svg};
use sourcefield_workspace::{OwnershipManifest, Transaction, file_sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Every case owns an isolated consumer and exercises the compiled public command.
struct Fixture(PathBuf);

impl Fixture {
    /// Write direct-authored synthetic facts without generation or shared checkout mutations.
    fn authored() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "sourcefield-validation-parity-{}-{}",
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

        Self(root)
    }

    /// Publish supported direct-authored artifacts without generation provenance or imports.
    fn direct_authored() -> Self {
        let fixture = Self::authored();
        let assets = fixture.0.join("assets");
        let snapshot = Snapshot::default();
        let state = build_state(&fixture.config(), &snapshot, "1970-01-01T00:00:00Z").unwrap();

        fs::create_dir_all(&assets).unwrap();
        fs::write(
            assets.join("profile-state.json"),
            serde_json::to_vec_pretty(&state).unwrap(),
        )
        .unwrap();
        fs::write(
            assets.join("source-snapshot.json"),
            serde_json::to_vec_pretty(&snapshot).unwrap(),
        )
        .unwrap();

        for (name, theme, motion) in [
            ("sourcefield.dark.svg", Theme::Dark, true),
            ("sourcefield.light.svg", Theme::Light, true),
            ("sourcefield.static.svg", Theme::Dark, false),
        ] {
            fs::write(assets.join(name), render_svg(&state, theme, motion)).unwrap();
        }

        fixture
    }

    /// Generate a valid complete record through the actual staged publication path.
    fn generated() -> Self {
        let fixture = Self::authored();
        let output = fixture.command(&["generate", "--offline", "--no-history"]);

        assert!(
            output.status.success(),
            "initial staged generation must succeed"
        );

        fixture
    }

    /// Resolve a real captured canonical manifest, rather than exercising only an empty import set.
    fn imported() -> Self {
        let fixture = Self::authored();
        let mut config = fixture.config();

        config.imports.push(OrganizationImport {
            id: "example-labs".into(),
            source: OrganizationSource::Local {
                path: "organization.toml".into(),
            },
        });
        fixture.write_config(&config);
        fs::write(
            fixture.0.join("config/organization.toml"),
            include_bytes!("../../../examples/organization.toml"),
        )
        .unwrap();
        let output = fixture.command(&["generate", "--offline", "--no-history"]);

        assert!(
            output.status.success(),
            "imported staged generation must succeed"
        );

        fixture
    }

    /// Suppress credential inheritance so every fixture has an explicit offline privacy boundary.
    fn command(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sourcefield"))
            .arg("--root")
            .arg(&self.0)
            .args(arguments)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("PROFILE_TOKEN")
            .env_remove("SOURCEFIELD_PRIVATE_COUNTS")
            .output()
            .unwrap()
    }

    /// Load only the synthetic authored fixture used to arrange a semantic edit.
    fn config(&self) -> Config {
        toml::from_str(&fs::read_to_string(self.0.join("config/profile.toml")).unwrap()).unwrap()
    }

    /// Persist an authored change without updating the earlier generation record.
    fn write_config(&self, config: &Config) {
        fs::write(
            self.0.join("config/profile.toml"),
            toml::to_string(config).unwrap(),
        )
        .unwrap();
    }

    /// Read actual owned bytes rather than treating the manifest's claims as proof.
    fn published(&self) -> BTreeMap<String, Vec<u8>> {
        let manifest: OwnershipManifest =
            serde_json::from_slice(&fs::read(self.0.join(".sourcefield-owned.json")).unwrap())
                .unwrap();

        manifest
            .files
            .keys()
            .chain(std::iter::once(&".sourcefield-owned.json".to_owned()))
            .map(|name| (name.clone(), fs::read(self.0.join(name)).unwrap()))
            .collect()
    }

    /// Update a recorded digest to probe semantic reconstruction beyond simple byte admission.
    fn update_record_digest(&self, name: &str) {
        let path = self.0.join("assets/generation-record.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

        record["inputs"][name] = file_sha256(self.0.join("assets").join(name))
            .unwrap()
            .into();
        fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }

    /// Populate twenty-four valid owned archives in deliberately unsorted, byte-sensitive order.
    fn seed_history(&self) -> BTreeMap<String, Vec<u8>> {
        let mut transaction = Transaction::begin(&self.0).unwrap();
        let mut preserved = BTreeMap::new();
        let mut states = Vec::new();
        let original: ProfileState =
            serde_json::from_slice(&fs::read(self.0.join("assets/profile-state.json")).unwrap())
                .unwrap();

        for (name, bytes) in self.published() {
            if name != ".sourcefield-owned.json" && name != "docs/history/index.json" {
                transaction.stage(name, &bytes).unwrap();
            }
        }

        for day in 1..=24 {
            let mut state = original.clone();

            state.semantic_hash = format!("{day:016X}");
            state.generated_at = format!("2026-09-{day:02}T12:00:00Z");
            let file = format!("{day:016x}.json");
            let name = format!("docs/history/{file}");
            let bytes = serde_json::to_vec_pretty(&state).unwrap();

            transaction.stage(&name, &bytes).unwrap();
            preserved.insert(name, bytes);
            states.push(serde_json::json!({
                "hash": state.semantic_hash,
                "generated_at": state.generated_at,
                "file": file,
                "node_count": state.nodes.len(),
                "edge_count": state.edges.len(),
            }));
        }

        // Trailing whitespace is legal JSON and exposes unnecessary index rewrites as well as pruning.
        let mut index = serde_json::to_vec(&serde_json::json!({
            "schema_version": sourcefield_core::STATE_SCHEMA_VERSION,
            "states": states,
        }))
        .unwrap();

        index.extend_from_slice(b"\n\n");
        transaction
            .stage("docs/history/index.json", &index)
            .unwrap();
        transaction.commit().unwrap();
        preserved.insert("docs/history/index.json".into(), index);
        let mut config = self.config();

        config.collection.history_limit = 1;
        self.write_config(&config);

        preserved
    }

    /// Model an already approved live capture and bind every changed replay input under ownership.
    fn publish_live_capture(&self) {
        let refresh = self.command(&["generate", "--offline"]);

        assert!(
            refresh.status.success(),
            "fixture refresh must update authored provenance without pruning history"
        );
        let mut files = self.published();
        let config: Config = serde_json::from_slice(&files["assets/resolved-config.json"]).unwrap();
        let mut snapshot: Snapshot =
            serde_json::from_slice(&files["assets/source-snapshot.json"]).unwrap();

        snapshot.mode = SnapshotMode::Live;
        snapshot.fetched_at = "2026-10-01T00:00:00Z".into();
        snapshot.warnings.clear();
        let state = build_state(&config, &snapshot, "2026-10-01T12:00:00Z").unwrap();

        for name in ["source-snapshot.json", "render-snapshot.json"] {
            files.insert(
                format!("assets/{name}"),
                serde_json::to_vec_pretty(&snapshot).unwrap(),
            );
        }
        files.insert(
            "assets/profile-state.json".into(),
            serde_json::to_vec_pretty(&state).unwrap(),
        );
        let mut record: serde_json::Value =
            serde_json::from_slice(&files["assets/generation-record.json"]).unwrap();

        for name in [
            "source-snapshot.json",
            "render-snapshot.json",
            "profile-state.json",
        ] {
            record["inputs"][name] =
                hex::encode(sha2::Sha256::digest(&files[&format!("assets/{name}")])).into();
        }

        files.insert(
            "assets/generation-record.json".into(),
            serde_json::to_vec_pretty(&record).unwrap(),
        );
        let mut transaction = Transaction::begin(&self.0).unwrap();

        for (name, bytes) in files {
            if name != ".sourcefield-owned.json" {
                transaction.stage(name, &bytes).unwrap();
            }
        }

        transaction.commit().unwrap();
    }

    /// Observe the complete directory, including archive files that ownership cleanup might remove.
    fn history(&self) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(self.0.join("docs/history"))
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = format!(
                    "docs/history/{}",
                    path.file_name().unwrap().to_str().unwrap()
                );

                (name, fs::read(path).unwrap())
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup must not hide the original failed assertion during unwinding.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn generated_candidate_and_standalone_validation_share_the_provenance_contract() {
    // Arrange
    let fixture = Fixture::generated();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        output.status.success(),
        "current generated inputs must validate"
    );
    assert!(
        fixture.published() == before,
        "validation must not alter published bytes"
    );
}

#[test]
fn validation_rejects_authored_semantic_change_without_mutating_outputs() {
    // Arrange
    let fixture = Fixture::generated();
    let mut config = fixture.config();

    config.profile.headline = "A changed approved headline.".into();
    fixture.write_config(&config);
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "stale output must not validate against changed authored facts"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("authored inputs changed"));
    assert!(
        fixture.published() == before,
        "validation must not alter published bytes"
    );
}

#[test]
fn validation_rejects_missing_generation_record() {
    // Arrange
    let fixture = Fixture::generated();
    fs::remove_file(fixture.0.join("assets/generation-record.json")).unwrap();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "resolved output requires provenance"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("generation-record.json"));
}

#[test]
fn validation_rejects_malformed_generation_record() {
    // Arrange
    let fixture = Fixture::generated();
    fs::write(fixture.0.join("assets/generation-record.json"), b"{broken").unwrap();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "malformed provenance must fail closed"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("parse JSON file"));
}

#[test]
fn validation_rejects_missing_resolved_config_instead_of_downgrading_to_authored_only() {
    // Arrange
    let fixture = Fixture::generated();
    fs::remove_file(fixture.0.join("assets/resolved-config.json")).unwrap();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "captured generation must not silently downgrade validation"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("resolved configuration"));
}

#[test]
fn validation_rejects_tampered_capture_bytes() {
    // Arrange
    let fixture = Fixture::generated();
    let path = fixture.0.join("assets/import-capture.json");
    let mut bytes = fs::read(&path).unwrap();

    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "capture bytes must match their recorded digest"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("input digest mismatch: import-capture.json")
    );
}

#[test]
fn validation_rechecks_capture_schema_even_when_its_recorded_digest_is_updated() {
    // Arrange
    let fixture = Fixture::generated();
    fs::write(
        fixture.0.join("assets/import-capture.json"),
        br#"{"schema_version":999,"imports":[]}"#,
    )
    .unwrap();
    fixture.update_record_digest("import-capture.json");

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "recorded bytes are not a substitute for capture validation"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported import capture schema"));
}

#[test]
fn validation_reconstructs_resolved_facts_instead_of_trusting_consistently_changed_artifacts() {
    // Arrange
    let fixture = Fixture::generated();
    let assets = fixture.0.join("assets");
    let mut config: Config =
        serde_json::from_slice(&fs::read(assets.join("resolved-config.json")).unwrap()).unwrap();

    config.profile.headline = "This headline was never authored.".into();
    let snapshot: Snapshot =
        serde_json::from_slice(&fs::read(assets.join("source-snapshot.json")).unwrap()).unwrap();
    let state = build_state(&config, &snapshot, "1970-01-01T00:00:00Z").unwrap();

    fs::write(
        assets.join("resolved-config.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    fs::write(
        assets.join("profile-state.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();

    for (name, theme, motion) in [
        ("sourcefield.dark.svg", Theme::Dark, true),
        ("sourcefield.light.svg", Theme::Light, true),
        ("sourcefield.static.svg", Theme::Dark, false),
    ] {
        fs::write(assets.join(name), render_svg(&state, theme, motion)).unwrap();
    }

    fixture.update_record_digest("resolved-config.json");
    fixture.update_record_digest("profile-state.json");

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "coherent generated edits must still agree with authored facts"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("captured inputs resolve differently")
    );
}

#[test]
fn validation_keeps_the_direct_authored_asset_path_without_imports_or_capture_metadata() {
    // Arrange
    let fixture = Fixture::direct_authored();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        output.status.success(),
        "direct-authored artifacts remain verifiable"
    );
}

#[test]
fn direct_authored_validation_identifies_unsupported_observation_snapshot_schema() {
    // Arrange
    let fixture = Fixture::direct_authored();
    let assets = fixture.0.join("assets");
    let unsupported = Snapshot {
        schema_version: 999,
        ..Snapshot::default()
    };

    fs::write(
        assets.join("source-snapshot.json"),
        serde_json::to_vec_pretty(&unsupported).unwrap(),
    )
    .unwrap();
    let before = [
        "source-snapshot.json",
        "profile-state.json",
        "sourcefield.dark.svg",
        "sourcefield.light.svg",
        "sourcefield.static.svg",
    ]
    .map(|name| (name, fs::read(assets.join(name)).unwrap()));

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "unsupported observation must fail closed"
    );
    assert!(message.contains("unsupported observation snapshot schema: 999; expected 1"));
    assert!(!message.contains("configuration version must be 1"));
    for (name, bytes) in before {
        assert!(
            fs::read(assets.join(name)).unwrap() == bytes,
            "validation must not publish"
        );
    }

    assert!(!assets.join("generation-record.json").exists());
    assert!(!fixture.0.join(".sourcefield-owned.json").exists());
}

#[test]
fn offline_generation_retains_all_twenty_four_archives_despite_a_lower_history_limit() {
    // Arrange
    let fixture = Fixture::generated();
    let before = fixture.seed_history();

    // Act
    let output = fixture.command(&["generate", "--offline"]);

    // Assert
    assert!(
        output.status.success(),
        "offline refresh must preserve valid owned history"
    );
    assert!(
        fixture.history() == before,
        "every archive and index byte must be preserved"
    );
}

#[test]
fn no_history_generation_retains_exact_index_and_archive_bytes() {
    // Arrange
    let fixture = Fixture::generated();
    let before = fixture.seed_history();

    // Act
    let output = fixture.command(&["generate", "--offline", "--no-history"]);

    // Assert
    assert!(
        output.status.success(),
        "disabled history must preserve valid owned history"
    );
    assert!(
        fixture.history() == before,
        "every archive and index byte must be preserved"
    );
}

#[test]
fn managed_project_readme_keeps_the_authored_project_order() {
    // Arrange
    let fixture = Fixture::generated();
    let path = fixture.0.join("README.md");
    fs::write(
        &path,
        "Intro\n<!-- sourcefield:projects:start --><!-- sourcefield:projects:end -->\n<!-- sourcefield:packages:start --><!-- sourcefield:packages:end -->\nFooter\n",
    )
    .unwrap();

    // Act
    let output = fixture.command(&[
        "generate",
        "--offline",
        "--no-history",
        "--readme",
        "README.md",
    ]);

    // Assert
    assert!(
        output.status.success(),
        "managed README generation must succeed"
    );
    let text = fs::read_to_string(path).unwrap();

    assert!(text.find("TraceDemo").unwrap() < text.find("ShellConfig").unwrap());
}

#[test]
fn captured_organization_validation_recomposes_without_reading_changed_local_source() {
    // Arrange
    let fixture = Fixture::imported();
    let before = fixture.published();
    fs::write(
        fixture.0.join("config/organization.toml"),
        b"a changed local working file",
    )
    .unwrap();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        output.status.success(),
        "validation consumes recorded canonical bytes without local refresh"
    );
    assert!(
        fixture.published() == before,
        "validation must not alter published bytes"
    );
}

#[test]
fn validation_rejects_a_changed_import_manifest_even_with_matching_recorded_digests() {
    // Arrange
    let fixture = Fixture::imported();
    let path = fixture.0.join("assets/import-capture.json");
    let mut capture: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let manifest = capture["imports"][0]["manifest"].as_str().unwrap().replace(
        "Open source database tools.",
        "Different captured organization facts.",
    );
    let manifest_path = fixture.0.join("config/organization.toml");

    fs::write(&manifest_path, &manifest).unwrap();
    capture["imports"][0]["manifest"] = manifest.into();
    capture["imports"][0]["digest"] = file_sha256(&manifest_path).unwrap().into();
    fs::write(path, serde_json::to_vec_pretty(&capture).unwrap()).unwrap();
    fixture.update_record_digest("import-capture.json");

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "changed canonical facts must agree with recorded resolved output"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("captured inputs resolve differently")
    );
}

#[test]
fn locked_replay_of_a_live_capture_keeps_history_when_retention_was_previously_disabled() {
    // Arrange
    let fixture = Fixture::generated();
    let before = fixture.seed_history();
    fixture.publish_live_capture();

    // Act
    let output = fixture.command(&["generate", "--offline", "--locked"]);

    // Assert
    assert!(
        output.status.success(),
        "offline replay of an approved live capture must succeed"
    );
    assert!(
        fixture.history() == before,
        "replay must preserve every archived byte without retention"
    );
}

#[test]
fn validation_reports_generator_identity_mismatch_without_authored_drift() {
    // Arrange
    let fixture = Fixture::generated();
    let path = fixture.0.join("assets/generation-record.json");
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

    record["generator_fingerprint"] = "different-compiled-source".into();
    fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    let message = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "generator identity must be admitted"
    );
    assert!(
        message.contains("generator identity mismatch"),
        "unexpected diagnostic: {message}"
    );
    assert!(
        !message.contains("authored inputs"),
        "unchanged authored input is a separate boundary"
    );
    assert!(fixture.published() == before, "validation must not publish");
}

#[test]
fn validation_reports_an_unsupported_envelope_schema_separately() {
    // Arrange
    let fixture = Fixture::generated();
    let path = fixture.0.join("assets/generation-record.json");
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

    record["schema_version"] = 999.into();
    fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "unsupported envelope must fail closed"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported generation record schema: 999")
    );
    assert!(fixture.published() == before, "validation must not publish");
}

#[test]
fn validation_reports_an_incomplete_current_envelope_inventory() {
    // Arrange
    let fixture = Fixture::generated();
    let path = fixture.0.join("assets/generation-record.json");
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();

    record["inputs"]
        .as_object_mut()
        .unwrap()
        .remove("render-snapshot.json");
    fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "current envelope requires all six inputs"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("input inventory mismatch for schema 2")
    );
    assert!(fixture.published() == before, "validation must not publish");
}

#[test]
fn validation_reports_changed_effective_snapshot_bytes() {
    // Arrange
    let fixture = Fixture::generated();
    let path = fixture.0.join("assets/render-snapshot.json");
    let mut bytes = fs::read(&path).unwrap();

    bytes.push(b'\n');
    fs::write(path, bytes).unwrap();
    let before = fixture.published();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "effective inputs must match their captured digest"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("generation provenance input digest mismatch: render-snapshot.json")
    );
    assert!(fixture.published() == before, "validation must not publish");
}

#[test]
fn validation_names_the_missing_required_import_capture_without_publishing() {
    // Arrange
    let fixture = Fixture::imported();
    let before = fixture.published();
    let missing = "assets/import-capture.json";

    fs::remove_file(fixture.0.join(missing)).unwrap();

    // Act
    let output = fixture.command(&["validate"]);

    // Assert
    assert!(
        !output.status.success(),
        "missing captured input must fail closed"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("read required generation input import-capture.json")
    );
    assert!(!fixture.0.join(missing).exists());
    for (name, bytes) in before {
        if name != missing {
            assert!(
                fs::read(fixture.0.join(name)).unwrap() == bytes,
                "validation must preserve the remaining publication"
            );
        }
    }
}

#[test]
fn locked_replay_names_the_missing_effective_snapshot_without_publishing() {
    // Arrange
    let fixture = Fixture::generated();
    let before = fixture.published();
    let missing = "assets/render-snapshot.json";

    fs::remove_file(fixture.0.join(missing)).unwrap();

    // Act
    let output = fixture.command(&["generate", "--offline", "--locked"]);

    // Assert
    assert!(
        !output.status.success(),
        "missing effective replay input must fail closed"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("read required generation input render-snapshot.json")
    );
    assert!(!fixture.0.join(missing).exists());
    assert!(!fixture.0.join(".sourcefield-lock").exists());
    assert!(!fixture.0.join(".sourcefield-transaction").exists());
    for (name, bytes) in before {
        if name != missing {
            assert!(
                fs::read(fixture.0.join(name)).unwrap() == bytes,
                "replay must preserve the remaining publication"
            );
        }
    }
}
