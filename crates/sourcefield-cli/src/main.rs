//! Collect, validate, and publish reproducible profile artifacts.
#![forbid(unsafe_code)]

mod diagnostics;
mod extract;
mod imports;
mod modes;
mod observations;
mod runtime;

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Parser, Subcommand};
use diagnostics::require_live;
use modes::ExecutionMode;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use sourcefield_collector::Collector;
use sourcefield_core::{
    ProfileState, Snapshot, SnapshotMode, build_prepared_state, build_state, expanded_config,
    load_config, prepare_profile, render_package_readme_prepared, render_project_readme,
    restore_package_cache, validate_config, validate_state,
};
use sourcefield_render::{PreparedPresentation, Theme, render_svg};
use sourcefield_workspace::{Transaction, file_sha256};

#[derive(Debug, Parser)]
#[command(name = "sourcefield")]
#[command(about = "Generate a deterministic GitHub profile topology")]
#[command(version)]
struct Cli {
    /// Consumer root; all managed output paths must stay beneath it.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Collect public GitHub and NuGet signals, analyze the topology and render all assets.
    Generate {
        #[arg(long, default_value = "config/profile.toml")]
        config: PathBuf,
        #[arg(long, default_value = "config/offline-snapshot.json")]
        fallback_snapshot: PathBuf,
        #[arg(long, default_value = "assets")]
        assets: PathBuf,
        #[arg(long, default_value = "docs")]
        docs: PathBuf,
        /// Explicit README destination; omitted for isolated generation.
        #[arg(long)]
        readme: Vec<PathBuf>,
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        strict_live: bool,
        #[arg(long)]
        private_counts: bool,
        #[arg(long)]
        no_history: bool,
        /// Replay captured configuration and observations without remote access.
        #[arg(long)]
        locked: bool,
        /// Verified runtime bundle including matching WASM and browser files.
        #[arg(long)]
        runtime: Option<PathBuf>,
        /// Explicitly adopt existing generated files during the one-time transition.
        #[arg(long)]
        adopt_existing: bool,
    },
    /// Separate canonical organization facts while retaining approved layout overrides.
    ExtractOrganization {
        #[arg(long)]
        config: PathBuf,
        /// Root-relative captured inputs and layout directory, retained beneath the export.
        #[arg(long, default_value = "assets")]
        assets: PathBuf,
        #[arg(long)]
        organization: String,
        #[arg(long)]
        destination: PathBuf,
    },
    /// Convert legacy consumer inputs into a separate directory without changing originals.
    Migrate {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, value_parser = ["personal", "organization"])]
        variant: String,
    },
    /// Restore an interrupted candidate from its journal after explicit lock release.
    Recover {
        /// Exact abandoned lock token; use only after confirming its writer has stopped.
        #[arg(long)]
        abandoned_lock_token: Option<String>,
    },
    /// Validate configuration, semantic state and generated SVG assets.
    Validate {
        #[arg(long, default_value = "config/profile.toml")]
        config: PathBuf,
        #[arg(long, default_value = "assets/profile-state.json")]
        state: PathBuf,
        #[arg(long, default_value = "assets")]
        assets: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = sourcefield_workspace::checked_workspace_root(&cli.root)
        .context("resolve consumer root")?;

    match cli.command {
        Command::Generate {
            config,
            fallback_snapshot,
            assets,
            docs,
            offline,
            strict_live,
            private_counts,
            no_history,
            readme,
            locked,
            runtime,
            adopt_existing,
        } => {
            let config = root.join(config);
            let fallback_snapshot = root.join(fallback_snapshot);
            let assets = root.join(assets);
            let docs = root.join(docs);
            let readme = readme
                .iter()
                .map(|path| root.join(path))
                .collect::<Vec<_>>();

            generate(GenerateOptions {
                root: &root,
                locked,
                runtime: runtime.as_deref(),
                adopt_existing,
                config_path: &config,
                fallback_snapshot_path: &fallback_snapshot,
                assets_dir: &assets,
                docs_dir: &docs,
                readme_paths: &readme,
                offline,
                strict_live,
                private_counts,
                no_history,
            })
            .await
        }

        Command::ExtractOrganization {
            config,
            assets,
            organization,
            destination,
        } => {
            extract::export_from_path(
                &root,
                &root.join(config),
                &root.join(assets),
                &organization,
                &root.join(destination),
            )
            .await
        }

        Command::Migrate {
            source,
            destination,
            variant,
        } => migrate(&source, &destination, &variant),

        Command::Recover {
            abandoned_lock_token,
        } => {
            if let Some(token) = abandoned_lock_token {
                sourcefield_workspace::release_abandoned_lock(&root, &token)?;
            }

            sourcefield_workspace::recover(&root)
        }

        Command::Validate {
            config,
            state,
            assets,
        } => validate(&root.join(config), &root.join(state), &root.join(assets)).await,
    }
}

/// Explicit generation policy replaces positional Boolean arguments.
struct GenerateOptions<'a> {
    root: &'a Path,
    locked: bool,
    runtime: Option<&'a Path>,
    adopt_existing: bool,
    config_path: &'a Path,
    fallback_snapshot_path: &'a Path,
    assets_dir: &'a Path,
    docs_dir: &'a Path,
    readme_paths: &'a [PathBuf],
    offline: bool,
    strict_live: bool,
    private_counts: bool,
    no_history: bool,
}

/// Build, validate and promote one complete consumer candidate under an exclusive lock.
async fn generate(options: GenerateOptions<'_>) -> Result<()> {
    generate_with_collection(options, CollectionAccess::from_environment(), collect_live).await
}

/// Explicit collection access separates environment lookup from orchestration and test fixtures.
#[derive(Default)]
struct CollectionAccess {
    /// Credential used only at fixed public GitHub origins.
    public_token: Option<String>,
    /// Credential required for newly authorized private aggregate observations.
    profile_token: Option<String>,
    /// Explicit process-level request, equivalent to the CLI/configuration opt-in.
    private_from_environment: bool,
}

impl CollectionAccess {
    /// Read credentials once; never serialize or log this resource bundle.
    fn from_environment() -> Self {
        Self {
            public_token: first_non_empty_env(&["GH_TOKEN", "GITHUB_TOKEN"]),
            profile_token: first_non_empty_env(&["PROFILE_TOKEN"]),
            private_from_environment: env_flag("SOURCEFIELD_PRIVATE_COUNTS"),
        }
    }
}

/// Construct the live transport only when the validated execution mode invokes its factory.
async fn collect_live(
    config: &sourcefield_core::Config,
    include_private: bool,
    token: Option<String>,
    profile_token: Option<String>,
) -> Result<Snapshot> {
    let collector = Collector::new(token)
        .context("create public signal collector")?
        .with_private_token(profile_token);

    collector
        .collect(config, include_private)
        .await
        .context("collect configured sources")
}

/// Keep one orchestration path while allowing deterministic transport-boundary tests.
async fn generate_with_collection<F>(
    options: GenerateOptions<'_>,
    access: CollectionAccess,
    collect: F,
) -> Result<()>
where
    F: for<'a> AsyncFnOnce(
        &'a sourcefield_core::Config,
        bool,
        Option<String>,
        Option<String>,
    ) -> Result<Snapshot>,
{
    let GenerateOptions {
        root,
        locked,
        runtime,
        adopt_existing,
        config_path,
        fallback_snapshot_path,
        assets_dir,
        docs_dir,
        readme_paths,
        offline,
        strict_live,
        private_counts,
        no_history,
    } = options;

    let mode = ExecutionMode::from_flags(offline, locked, strict_live)?;

    let assets_relative = relative_output(root, assets_dir)?;
    let docs_relative = relative_output(root, docs_dir)?;
    let mut transaction = Transaction::begin(root)?;

    // Recovery can restore replay inputs; verify and consume them under this same writer lock.
    let replay_snapshot = if mode.is_replay() {
        Some(verify_replay_inputs(config_path, assets_dir)?)
    } else {
        None
    };

    let authored = load_config(config_path).context("load profile configuration")?;
    let previous =
        optional_json::<sourcefield_core::LayoutAssignments>(&assets_dir.join("layout.json"))?
            .unwrap_or_default();

    let captured =
        optional_json::<imports::ImportCapture>(&assets_dir.join("import-capture.json"))?;

    let token = access.public_token;
    let resolved = imports::resolve(
        &authored,
        imports::ResolveOptions {
            root: config_path
                .parent()
                .context("configuration directory missing")?,
            previous: &previous,
            captured: captured.as_ref(),
            mode,
            token: token.as_deref(),
        },
    )
    .await?;

    let config = if mode.is_replay() {
        let recorded: sourcefield_core::Config =
            read_json(&assets_dir.join("resolved-config.json"))?;

        if serde_json::to_value(&recorded)? != serde_json::to_value(&resolved.config)? {
            bail!("locked replay inputs resolve differently from the recorded configuration");
        }

        recorded
    } else {
        resolved.config
    };

    validate_config(&config).context("validate profile configuration")?;
    let source_path = assets_dir.join("source-snapshot.json");
    let published_capture = read_observation_capture(&source_path, mode.is_offline())
        .context("read last published observation capture")?;

    let captured = if let Some(name) = replay_snapshot {
        std::borrow::Cow::Owned(
            read_json::<Snapshot>(&assets_dir.join(name))
                .context("load recorded effective observation snapshot")?,
        )
    } else if let Some(capture) = published_capture
        .as_ref()
        .filter(|_| !mode.is_offline() || fallback_snapshot_path == source_path)
    {
        std::borrow::Cow::Borrowed(&capture.snapshot)
    } else {
        let fallback = read_observation_capture(fallback_snapshot_path, false)
            .context("load captured observation snapshot")?
            .map(|capture| capture.snapshot);

        match fallback {
            Some(snapshot) => std::borrow::Cow::Owned(snapshot),
            None if !mode.is_offline() => std::borrow::Cow::Owned(Snapshot::default()),
            None => bail!(
                "load captured observation snapshot: selected offline capture is missing: {}",
                fallback_snapshot_path.display()
            ),
        }
    };

    let private_counts_requested = private_counts
        || access.private_from_environment
        || config.collection.collect_private_repository_count;

    let profile_token = access.profile_token;
    let include_private_counts = private_counts_requested && profile_token.is_some();
    let private_count_requested_without_token = private_counts_requested && profile_token.is_none();

    let mut snapshot = observations::acquire(mode, &captured, || async {
        let mut snapshot = collect(
            &config,
            include_private_counts,
            token,
            profile_token.clone(),
        )
        .await?;

        restore_package_cache(&mut snapshot, &captured, &config);

        Ok(snapshot)
    })
    .await?;

    // Replay consumes an already published, approved snapshot. Credentials only authorize
    // new collection; requiring them again would make offline reproduction nondeterministic.
    if !mode.is_replay() {
        apply_private_count_policy(
            &mut snapshot,
            private_counts_requested,
            profile_token.is_some(),
        );
    }

    if private_count_requested_without_token && !mode.is_replay() {
        observations::add_warning(&mut snapshot, observations::PRIVATE_COUNT_WARNING);
    }

    require_live(&config, &snapshot, mode.is_strict())?;

    let generation_time = if mode.is_replay() {
        read_json::<ProfileState>(&assets_dir.join("profile-state.json"))?.generated_at
    } else if mode.is_offline() {
        "1970-01-01T00:00:00Z".to_string()
    } else {
        Utc::now().to_rfc3339()
    };

    let resolved_config_bytes = serde_json::to_vec_pretty(&config)?;
    // Graph normalization sorts facts for stable hashes. README order belongs to the
    // authored composition.
    let project_readme = render_project_readme(&config);
    let prepared = prepare_profile(&config, &snapshot)?;
    let config = prepared.config();
    let snapshot = if mode.is_replay() {
        std::borrow::Cow::Borrowed(prepared.snapshot())
    } else {
        preserve_observation_time(
            prepared.snapshot(),
            &assets_dir.join("source-snapshot.json"),
        )?
    };

    let mut state =
        build_prepared_state(&prepared, generation_time).context("build semantic profile state")?;

    validate_state(&state).context("validate semantic profile state")?;

    preserve_generation_time(&mut state, &assets_dir.join("profile-state.json"))?;
    // Fail before changing outputs when an existing archive cannot be trusted.
    load_history(&docs_dir.join("history"))?;

    let package_readme = render_package_readme_prepared(config);
    let prepared_readmes = readme_paths
        .iter()
        .map(|path| {
            let bytes = fs::read(path).context("read managed README")?;
            let expected = hex::encode(sha2::Sha256::digest(&bytes));
            let text = std::str::from_utf8(&bytes).context("README must be UTF-8")?;
            let packages = replace_section(text, "packages", &package_readme)?;
            let updated = replace_section(&packages, "projects", &project_readme)?;

            Ok((relative_output(root, path)?, updated, expected))
        })
        .collect::<Result<Vec<_>>>()?;

    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("profile-state.json"),
        &serde_json::to_vec_pretty(&state)?,
        adopt_existing,
    )?;
    stage_generated(
        &mut transaction,
        root,
        &docs_relative.join("profile-state.json"),
        &serde_json::to_vec_pretty(&state)?,
        adopt_existing,
    )?;
    let render_snapshot_bytes = serde_json::to_vec_pretty(&snapshot)?;
    let source_snapshot_bytes = if mode.is_offline() {
        published_capture
            .as_ref()
            .map(|capture| capture.bytes.as_slice())
            .unwrap_or(&render_snapshot_bytes)
    } else {
        &render_snapshot_bytes
    };

    // A preview is an applied rendering input, not a replacement for the durable live capture.
    // Both roles must be staged because ownership cleanup removes every unstaged generated file.
    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("source-snapshot.json"),
        source_snapshot_bytes,
        adopt_existing,
    )?;
    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("render-snapshot.json"),
        &render_snapshot_bytes,
        adopt_existing,
    )?;
    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("resolved-config.json"),
        &resolved_config_bytes,
        adopt_existing,
    )?;
    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("import-capture.json"),
        &serde_json::to_vec_pretty(&resolved.capture)?,
        adopt_existing,
    )?;
    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("layout.json"),
        &serde_json::to_vec_pretty(&resolved.layout)?,
        adopt_existing,
    )?;

    let presentation = PreparedPresentation::new(&state);

    for (name, theme, motion) in [
        ("sourcefield.dark.svg", Theme::Dark, true),
        ("sourcefield.light.svg", Theme::Light, true),
        ("sourcefield.static.svg", Theme::Dark, false),
    ] {
        let svg = presentation.render(theme, motion);

        if svg.len() > 900_000 {
            bail!("generated SVG exceeds the 900 KB README asset budget");
        }

        stage_generated(
            &mut transaction,
            root,
            &assets_relative.join(name),
            svg.as_bytes(),
            adopt_existing,
        )?;
        stage_generated(
            &mut transaction,
            root,
            &docs_relative.join(name),
            svg.as_bytes(),
            adopt_existing,
        )?;
    }

    runtime::stage(
        &mut transaction,
        root,
        &docs_relative,
        runtime,
        adopt_existing,
        &state.profile,
    )?;
    stage_generated(
        &mut transaction,
        root,
        &docs_relative.join("build-meta.json"),
        &serde_json::to_vec_pretty(&BuildMeta::from_state(&state))?,
        adopt_existing,
    )?;
    stage_history(
        &mut transaction,
        root,
        &docs_relative,
        &state,
        config.collection.history_limit,
        !no_history && !mode.is_offline() && state.mode == SnapshotMode::Live,
        adopt_existing,
    )?;

    for (path, content, expected) in prepared_readmes {
        transaction.stage_authored(path, content.as_bytes(), Some(&expected))?;
    }

    let candidate = transaction.candidate_dir().to_path_buf();
    let inputs = REPLAY_FILES
        .iter()
        .map(|name| {
            Ok((
                (*name).to_string(),
                file_sha256(candidate.join(&assets_relative).join(name))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    let record = GenerationRecord {
        schema_version: 2,
        generator_version: env!("CARGO_PKG_VERSION").into(),
        source_revision: env!("SOURCEFIELD_SOURCE_COMMIT").into(),
        generator_fingerprint: env!("SOURCEFIELD_GENERATOR_FINGERPRINT").into(),
        authored_config_sha256: file_sha256(config_path)?,
        inputs,
    };

    stage_generated(
        &mut transaction,
        root,
        &assets_relative.join("generation-record.json"),
        &serde_json::to_vec_pretty(&record)?,
        adopt_existing,
    )?;
    validate(
        config_path,
        &candidate.join(&assets_relative).join("profile-state.json"),
        &candidate.join(&assets_relative),
    )
    .await?;
    load_history(&candidate.join(&docs_relative).join("history"))?;
    transaction.validate()?;
    let report = transaction.commit()?;

    println!(
        "Published {} files; removed {} previously owned files.",
        report.written, report.removed
    );
    println!("SOURCEFIELD {}", state.semantic_hash);
    println!("  mode:     {:?}", state.mode);
    println!("  nodes:    {}", state.nodes.len());
    println!("  edges:    {}", state.edges.len());
    println!("  packages: {}", state.stats.package_count);

    Ok(())
}

/// Reconstruct captured composition, semantic state and SVG bytes from current authored inputs.
/// Validation is offline and never treats an unrelated resolved configuration as authored truth.
async fn validate(config_path: &Path, state_path: &Path, assets_dir: &Path) -> Result<()> {
    let authored = load_config(config_path).context("load profile configuration")?;
    let resolved_path = assets_dir.join("resolved-config.json");
    let mut snapshot_file = "source-snapshot.json";
    let config = if resolved_path.exists() {
        snapshot_file = verify_replay_inputs(config_path, assets_dir)
            .context("validate generation provenance")?;
        let previous = read_json(&assets_dir.join("layout.json"))?;
        let capture = read_json(&assets_dir.join("import-capture.json"))?;
        let resolved = imports::resolve(
            &authored,
            imports::ResolveOptions {
                root: config_path
                    .parent()
                    .context("configuration directory missing")?,
                previous: &previous,
                captured: Some(&capture),
                mode: ExecutionMode::LockedReplay,
                token: None,
            },
        )
        .await?;

        let recorded: sourcefield_core::Config = read_json(&resolved_path)?;

        if serde_json::to_value(&recorded)? != serde_json::to_value(&resolved.config)? {
            bail!("captured inputs resolve differently from the recorded configuration");
        }

        recorded
    } else {
        // Pre-capture direct-authored assets remain readable. Deleting one capture must not
        // downgrade validation.
        if !authored.imports.is_empty()
            || [
                "generation-record.json",
                "import-capture.json",
                "layout.json",
                "render-snapshot.json",
            ]
            .iter()
            .any(|name| assets_dir.join(name).exists())
        {
            bail!("captured generation requires resolved configuration and its generation record");
        }

        authored
    };

    validate_config(&config).context("validate profile configuration")?;
    let state = read_json::<ProfileState>(state_path).context("load semantic state")?;
    validate_state(&state).context("validate semantic state")?;
    if snapshot_file != "source-snapshot.json" {
        // The retained source is independently admitted even when the effective snapshot differs.
        read_observation_capture(&assets_dir.join("source-snapshot.json"), false)
            .context("load durable observation capture")?
            .context("durable observation capture is missing: source-snapshot.json")?;
    }

    let snapshot: Snapshot = read_json(&assets_dir.join(snapshot_file))
        .context("load recorded effective source inputs")?;

    let reconstructed = build_state(&config, &snapshot, state.generated_at.clone())?;
    if serde_json::to_value(&reconstructed)? != serde_json::to_value(&state)? {
        bail!("semantic state does not match configuration and recorded source inputs");
    }

    for name in [
        "sourcefield.dark.svg",
        "sourcefield.light.svg",
        "sourcefield.static.svg",
    ] {
        let path = assets_dir.join(name);
        let svg = fs::read_to_string(&path)
            .with_context(|| format!("read generated SVG {}", path.display()))?;

        let theme = if name == "sourcefield.light.svg" {
            Theme::Light
        } else {
            Theme::Dark
        };

        let motion = name != "sourcefield.static.svg";
        validate_svg(&svg, &state, theme, motion)?;
        if svg.len() > 900_000 {
            bail!("{} exceeds the 900 KB README asset budget", path.display());
        }
    }

    let expanded = expanded_config(&config, &snapshot);
    let configured_packages = expanded
        .publications
        .iter()
        .flat_map(|publication| publication.packages.iter())
        .map(|package| package.id.as_str())
        .collect::<BTreeSet<_>>();

    if configured_packages.len() != state.stats.package_count as usize {
        bail!("package count in state does not match configuration");
    }

    println!(
        "Validated {} nodes, {} edges and all generated SVG assets.",
        state.nodes.len(),
        state.edges.len()
    );

    Ok(())
}

/// Validate generated provenance, not arbitrary SVG via an incomplete payload denylist.
fn validate_svg(svg: &str, state: &ProfileState, theme: Theme, motion: bool) -> Result<()> {
    if svg != render_svg(state, theme, motion) {
        bail!("SVG differs from the canonical renderer for the validated state");
    }

    Ok(())
}

/// Clear even dated fallback counts unless both explicit intent and credentials are present.
fn apply_private_count_policy(snapshot: &mut Snapshot, requested: bool, token_present: bool) {
    if !requested || !token_present {
        snapshot.private_repository_count = None;
    }
}

/// Keep the original successful observation when a refresh changes only its timestamp.
///
/// This retains truthful capture provenance instead of claiming the old artifact was collected
/// again today. A changed value, source status, warning or mode always creates a new observation.
fn preserve_observation_time<'a>(
    snapshot: &'a Snapshot,
    path: &Path,
) -> Result<std::borrow::Cow<'a, Snapshot>> {
    if snapshot.mode != SnapshotMode::Live || !path.exists() {
        return Ok(std::borrow::Cow::Borrowed(snapshot));
    }

    let previous: Snapshot = read_json(path)?;
    if previous.mode != SnapshotMode::Live {
        return Ok(std::borrow::Cow::Borrowed(snapshot));
    }

    let mut comparable = serde_json::to_value(snapshot)?;
    comparable["fetched_at"] = serde_json::Value::String(previous.fetched_at.clone());
    if comparable == serde_json::to_value(&previous)? {
        return Ok(std::borrow::Cow::Owned(previous));
    }

    Ok(std::borrow::Cow::Borrowed(snapshot))
}

/// Preserve content timestamps when semantic inputs and observable provenance are unchanged.
fn preserve_generation_time(state: &mut ProfileState, path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let previous_json: serde_json::Value = read_json(path)?;
    match previous_json
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    {
        Some(version) if version == u64::from(state.schema) => {}
        _ => bail!("existing state has an unsupported or missing schema"),
    }

    let previous: ProfileState = serde_json::from_value(previous_json)
        .context("parse current-schema state before preserving its generation timestamp")?;

    validate_state(&previous).context("validate existing current-schema state")?;
    let mut candidate = state.clone();
    candidate.generated_at.clone_from(&previous.generated_at);
    if serde_json::to_value(&candidate)? == serde_json::to_value(&previous)? {
        state.generated_at = previous.generated_at;
    }

    Ok(())
}

/// Reject traversal, links, duplicate entries and inconsistent immutable archive metadata.
fn load_history(directory: &Path) -> Result<HistoryIndex> {
    if directory.is_symlink() {
        bail!("history directory must not be a symlink");
    }

    let path = directory.join("index.json");
    if path.is_symlink() {
        bail!("history index must not be a symlink");
    }

    if !path.exists() {
        return Ok(HistoryIndex::default());
    }

    let index: HistoryIndex = read_json(&path)?;
    if index.schema_version != sourcefield_core::STATE_SCHEMA_VERSION
        || index.states.len() > sourcefield_core::MAX_HISTORY_ENTRIES
    {
        bail!("unsupported history schema or excessive history entries");
    }

    let mut hashes = BTreeSet::new();
    for entry in &index.states {
        validate_history_name(&entry.hash, &entry.file)?;
        if !hashes.insert(&entry.hash) {
            bail!("history contains duplicate hashes");
        }

        let archive = directory.join(&entry.file);
        if archive.is_symlink() {
            bail!("history archive must not be a symlink");
        }

        let state: ProfileState = read_json(&archive)?;
        validate_state(&state).context("validate archived semantic state")?;
        if state.semantic_hash != entry.hash
            || state.generated_at != entry.generated_at
            || state.nodes.len() != entry.node_count
            || state.edges.len() != entry.edge_count
        {
            bail!("history metadata does not match archived state");
        }

        chrono::DateTime::parse_from_rfc3339(&entry.generated_at)
            .context("invalid archive timestamp")?;
    }

    Ok(index)
}

/// Require the immutable archive filename to match its semantic hash.
fn validate_history_name(hash: &str, file: &str) -> Result<()> {
    if hash.len() != 16
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || file != format!("{}.json", hash.to_ascii_lowercase())
    {
        bail!("invalid history hash or filename");
    }

    Ok(())
}

/// Resolve explicitly ordered credential sources without exposing their contents.
fn first_non_empty_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

/// Read the supported affirmative spellings for an opt-in environment flag.
fn env_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Deserialize only bounded, regular JSON input.
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    if path.is_symlink() {
        bail!("JSON input must not be a symbolic link");
    }

    let file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("JSON input must be a regular file");
    }

    sourcefield_io::read_json_bounded(file, 16 * 1024 * 1024)
        .with_context(|| format!("parse JSON file {}", path.display()))
}

/// Current-schema ordered archive inventory retained with the consumer.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryIndex {
    /// Current unified history index schema.
    schema_version: u32,
    #[serde(default)]
    states: Vec<HistoryEntry>,
}

/// Reference metadata checked against the matching immutable archive.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryEntry {
    hash: String,
    generated_at: String,
    file: String,
    node_count: usize,
    edge_count: usize,
}

/// Browser-visible build identity kept separate from captured observation time.
#[derive(Debug, Serialize)]
struct BuildMeta<'a> {
    generator_version: &'static str,
    source_revision: &'static str,
    generator_fingerprint: &'static str,
    generator: &'static str,
    semantic_hash: &'a str,
    generated_at: &'a str,
    mode: SnapshotMode,
    node_count: usize,
    edge_count: usize,
}

impl<'a> BuildMeta<'a> {
    /// Borrow already prepared state without cloning its strings.
    fn from_state(state: &'a ProfileState) -> Self {
        Self {
            generator_version: env!("CARGO_PKG_VERSION"),
            source_revision: env!("SOURCEFIELD_SOURCE_COMMIT"),
            generator_fingerprint: env!("SOURCEFIELD_GENERATOR_FINGERPRINT"),
            generator: "sourcefield-rust",
            semantic_hash: &state.semantic_hash,
            generated_at: &state.generated_at,
            mode: state.mode,
            node_count: state.nodes.len(),
            edge_count: state.edges.len(),
        }
    }
}

#[cfg(test)]
mod tests;

impl Default for HistoryIndex {
    fn default() -> Self {
        Self {
            schema_version: sourcefield_core::STATE_SCHEMA_VERSION,
            states: Vec::new(),
        }
    }
}

/// Require an explicit output beneath the consumer root, with no traversal or symlinks.
fn relative_output(root: &Path, path: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(root)
        .context("output must remain inside the consumer root")?;

    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        bail!("output requires a normalized relative path");
    }

    let mut prefix = root.to_path_buf();

    for part in relative.components() {
        prefix.push(part);

        if prefix.is_symlink() {
            bail!("output path must not contain a symbolic link");
        }
    }

    Ok(relative.to_path_buf())
}

/// Adopt old generated files only when explicitly requested, checking their original bytes.
fn stage_generated(
    transaction: &mut Transaction,
    root: &Path,
    path: &Path,
    bytes: &[u8],
    adopt: bool,
) -> Result<()> {
    if adopt && root.join(path).exists() {
        let digest = file_sha256(root.join(path))?;

        transaction.stage_checked(path, bytes, Some(&digest))?;
    } else {
        transaction.stage(path, bytes)?;
    }

    Ok(())
}

/// Replace a single managed section while retaining every byte of surrounding authored prose.
fn replace_section(readme: &str, kind: &str, section: &str) -> Result<String> {
    let start_marker = format!("<!-- sourcefield:{kind}:start -->");
    let end_marker = format!("<!-- sourcefield:{kind}:end -->");

    if readme.matches(&start_marker).count() != 1 || readme.matches(&end_marker).count() != 1 {
        bail!("README requires exactly one {kind} marker pair");
    }

    let start = readme.find(&start_marker).context("start marker missing")?;
    let end = readme.find(&end_marker).context("end marker missing")?;

    if end < start {
        bail!("README {kind} markers are reversed");
    }

    Ok(format!(
        "{}{}{}",
        &readme[..start],
        section,
        &readme[end + end_marker.len()..]
    ))
}

/// Missing optional captures are different from malformed existing captures.
fn optional_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect optional JSON input {}", path.display()));
        }
    }

    Ok(Some(read_json(path)?))
}

/// Preserve exact admitted observation bytes alongside their parsed facts for offline restaging.
struct ObservationCapture {
    /// Original durable input, including legal whitespace and field ordering.
    bytes: Vec<u8>,
    /// Facts used for collection fallback and deliberate preview preparation.
    snapshot: Snapshot,
}

/// Missing capture allows first refresh; malformed, nonregular and inaccessible inputs stay errors.
fn read_observation_capture(path: &Path, retain_bytes: bool) -> Result<Option<ObservationCapture>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect observation capture {}", path.display()));
        }
    };

    if !metadata.is_file() {
        bail!(
            "observation capture must be a regular file without symlinks: {}",
            path.display()
        );
    }

    let (bytes, snapshot): (Vec<u8>, Snapshot) = if retain_bytes {
        let bytes = read_bounded(path)?;
        let snapshot = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse observation capture {}", path.display()))?;

        (bytes, snapshot)
    } else {
        // Refresh replaces this capture, so streaming avoids retaining a redundant raw allocation.
        let snapshot = read_json(path)
            .with_context(|| format!("parse observation capture {}", path.display()))?;

        (Vec::new(), snapshot)
    };

    // A valid effective seed cannot establish compatibility for a different retained capture.
    if snapshot.schema_version != 1 {
        bail!(
            "unsupported observation capture schema: {}",
            snapshot.schema_version
        );
    }

    Ok(Some(ObservationCapture { bytes, snapshot }))
}

/// Stage the entire retained history set so ownership cleanup cannot discard live archives.
fn stage_history(
    transaction: &mut Transaction,
    root: &Path,
    docs: &Path,
    state: &ProfileState,
    limit: usize,
    append: bool,
    adopt: bool,
) -> Result<()> {
    let directory = root.join(docs).join("history");
    let mut index = load_history(&directory)?;
    let mut new_archive = None;

    if append
        && !index
            .states
            .iter()
            .any(|entry| entry.hash == state.semantic_hash)
    {
        let file = format!("{}.json", state.semantic_hash.to_ascii_lowercase());

        validate_history_name(&state.semantic_hash, &file)?;

        if directory.join(&file).exists() {
            bail!("unindexed archive exists; preserve it for explicit recovery");
        }

        index.states.push(HistoryEntry {
            hash: state.semantic_hash.clone(),
            generated_at: state.generated_at.clone(),
            file: file.clone(),
            node_count: state.nodes.len(),
            edge_count: state.edges.len(),
        });
        new_archive = Some(file);
    }

    if append {
        index.states.sort_by(|left, right| {
            chrono::DateTime::parse_from_rfc3339(&right.generated_at)
                .ok()
                .cmp(&chrono::DateTime::parse_from_rfc3339(&left.generated_at).ok())
                .then_with(|| left.hash.cmp(&right.hash))
        });
        index.states.truncate(limit.max(1));
    }

    // Ownership cleanup treats unstaged files as deletions, so disabled history must
    // restage every retained byte.
    let index_bytes = if !append && directory.join("index.json").exists() {
        read_bounded(&directory.join("index.json"))?
    } else {
        serde_json::to_vec_pretty(&index)?
    };

    for entry in &index.states {
        let bytes = if new_archive.as_deref() == Some(&entry.file) {
            serde_json::to_vec_pretty(state)?
        } else {
            read_bounded(&directory.join(&entry.file))?
        };

        stage_generated(
            transaction,
            root,
            &docs.join("history").join(&entry.file),
            &bytes,
            adopt,
        )?;
    }

    stage_generated(
        transaction,
        root,
        &docs.join("history/index.json"),
        &index_bytes,
        adopt,
    )?;

    Ok(())
}

/// Bound serialized inputs before parsing to keep malformed files from exhausting memory.
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("input must be a regular file: {}", path.display());
    }

    sourcefield_io::read_file_bounded(file, 16 * 1024 * 1024)
        .with_context(|| format!("read bounded input {}", path.display()))
}

/// Run the explicit migration into a new tree; original inputs remain the recovery source.
fn migrate(source: &Path, destination: &Path, variant: &str) -> Result<()> {
    use sourcefield_workspace::migration::{ProfileKind, migrate_files};

    let kind = match variant {
        "personal" => ProfileKind::Personal,
        "organization" => ProfileKind::Organization,
        _ => bail!("unsupported migration variant"),
    };

    let paths = ["config", "assets", "docs", "README.md", "profile/README.md"]
        .into_iter()
        .filter(|path| source.join(path).exists())
        .map(PathBuf::from)
        .collect::<Vec<_>>();

    let report = migrate_files(source, destination, &paths, kind)?;

    println!("{}", serde_json::to_string_pretty(&report)?);

    Ok(())
}

/// Captured roles required for replay; names never come from untrusted record paths.
const REPLAY_FILES: &[&str] = &[
    "resolved-config.json",
    "source-snapshot.json",
    "render-snapshot.json",
    "import-capture.json",
    "layout.json",
    "profile-state.json",
];

/// Provenance binds the generator and all authored/resolved observations of one generation.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationRecord {
    /// Version of this replay envelope.
    schema_version: u32,
    /// Exact native CLI semantic version.
    generator_version: String,
    /// Release commit or explicit unreleased development provenance.
    source_revision: String,
    /// Exact compiled source identity, including unreleased local changes.
    generator_fingerprint: String,
    /// Authored configuration bytes before import composition.
    authored_config_sha256: String,
    /// SHA-256 digests for the fixed replay input set.
    inputs: BTreeMap<String, String>,
}

/// Reject configuration drift and tampered snapshots after recovery under the held transaction lock.
/// The caller must retain that lock until the verified inputs have been consumed and published.
fn verify_replay_inputs(config: &Path, assets: &Path) -> Result<&'static str> {
    let record: GenerationRecord = read_json(&assets.join("generation-record.json"))?;

    // Older releases use a different envelope; identify their build before admitting its schema.
    if record.generator_version != env!("CARGO_PKG_VERSION")
        || record.source_revision != env!("SOURCEFIELD_SOURCE_COMMIT")
        || record.generator_fingerprint != env!("SOURCEFIELD_GENERATOR_FINGERPRINT")
    {
        bail!(
            "generation provenance generator identity mismatch: use the recorded generator build"
        );
    }

    if record.schema_version != 2 {
        bail!(
            "unsupported generation record schema: {}",
            record.schema_version
        );
    }

    if record.authored_config_sha256 != file_sha256(config)? {
        bail!(
            "generation provenance authored inputs changed: configuration bytes differ from the recorded generation"
        );
    }

    if record.inputs.len() != REPLAY_FILES.len() {
        bail!(
            "generation provenance input inventory mismatch for schema {}",
            record.schema_version
        );
    }

    for name in REPLAY_FILES {
        let digest = file_sha256(assets.join(name))
            .with_context(|| format!("read required generation input {name}"))?;

        if record.inputs.get(*name) != Some(&digest) {
            bail!("generation provenance input digest mismatch: {name}");
        }
    }

    Ok("render-snapshot.json")
}
