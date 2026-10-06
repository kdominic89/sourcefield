//! Separate organization facts from consumer-owned presentation during migration.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use sourcefield_core::{Config, OrganizationImport, OrganizationManifest, OrganizationSource};
use sourcefield_workspace::{Transaction, checked_workspace_root};

/// Resolve selected facts without replacing the consumer's authored import relationships.
pub(crate) async fn export_from_path(
    root: &Path,
    config_path: &Path,
    assets_dir: &Path,
    scope: &str,
    destination: &Path,
) -> Result<()> {
    let assets = crate::relative_output(root, assets_dir)?;
    validate_asset_prefix(&assets)?;
    let destination = export_destination(destination)?;
    let config = sourcefield_core::load_config(config_path)?;
    let previous = crate::optional_json::<sourcefield_core::LayoutAssignments>(
        &assets_dir.join("layout.json"),
    )?
    .unwrap_or_default();
    let captured = crate::optional_json::<crate::imports::ImportCapture>(
        &assets_dir.join("import-capture.json"),
    )?;
    let config_directory = config_path
        .parent()
        .context("configuration directory missing")?;
    let resolved = crate::imports::resolve(
        &config,
        crate::imports::ResolveOptions {
            root: config_directory,
            previous: &previous,
            captured: captured.as_ref(),
            mode: crate::modes::ExecutionMode::OfflinePreview,
            token: None,
        },
    )
    .await?;
    let candidate = prepare_export(&config, &resolved, config_directory, scope, &destination)?;

    publish_export(&destination, &assets, candidate)
}

/// Validated authored inputs plus replaceable evidence for the destination's first generation.
struct ExportCandidate {
    consumer: Config,
    manifest: String,
    capture: crate::imports::ImportCapture,
    layout: sourcefield_core::LayoutAssignments,
}

/// Preserve canonical bytes for imported content; split only facts authored inline by this consumer.
fn prepare_export(
    authored: &Config,
    resolved: &crate::imports::ResolvedImports,
    config_directory: &Path,
    scope: &str,
    destination: &Path,
) -> Result<ExportCandidate> {
    let (manifest, mut consumer) = if authored.imports.iter().any(|import| import.id == scope) {
        let selected = resolved
            .capture
            .imports
            .iter()
            .find(|input| input.id == scope)
            .context("selected organization is missing from resolved inputs")?;

        (selected.manifest.clone(), authored.clone())
    } else {
        let (manifest, consumer) = split_with_lookup(authored, &resolved.config, scope)?;

        (toml::to_string_pretty(&manifest)?, consumer)
    };
    let source = OrganizationSource::Local {
        path: "organization.toml".into(),
    };

    if let Some(import) = consumer
        .imports
        .iter_mut()
        .find(|import| import.id == scope)
    {
        import.source = source;
    } else {
        consumer.imports.push(OrganizationImport {
            id: scope.into(),
            source,
        });
    }

    let mut capture = crate::imports::ImportCapture {
        schema_version: resolved.capture.schema_version,
        imports: Vec::new(),
    };
    let mut manifests = Vec::new();

    for import in &mut consumer.imports {
        let mut input = if import.id == scope {
            crate::imports::CapturedImport {
                id: scope.into(),
                source: import.source.clone(),
                commit: None,
                repository: None,
                digest: sourcefield_io::sha256_bytes(manifest.as_bytes()),
                manifest: manifest.clone(),
            }
        } else {
            resolved
                .capture
                .imports
                .iter()
                .find(|input| input.id == import.id)
                .context("retained organization is missing from resolved inputs")?
                .clone()
        };

        if import.id != scope
            && let OrganizationSource::Local { path } = &mut import.source
        {
            *path = rebase_local_path(config_directory, destination, path)?;
            input.source = import.source.clone();
        }

        manifests.push(sourcefield_core::parse_organization(&input.manifest)?);
        capture.imports.push(input);
    }

    // All retained manifests participate in validation, but their facts never become authored copies.
    let (_, layout) =
        sourcefield_core::compose_organizations(&consumer, &manifests, &resolved.layout)?;

    Ok(ExportCandidate {
        consumer,
        manifest,
        capture,
        layout,
    })
}

/// Rebase the path expression, preserving symlinks and parent traversals in the authored suffix.
fn rebase_local_path(config_directory: &Path, destination: &Path, path: &str) -> Result<String> {
    if Path::new(path).is_absolute() {
        return Ok(path.to_string());
    }

    let target = config_directory.join(path);
    let target_parts = target.components().collect::<Vec<_>>();
    let destination_parts = destination.components().collect::<Vec<_>>();
    let common = target_parts
        .iter()
        .zip(&destination_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let rebased = if common == 0 {
        target
    } else {
        let mut relative = PathBuf::new();

        for _ in &destination_parts[common..] {
            relative.push("..");
        }

        for part in &target_parts[common..] {
            relative.push(part.as_os_str());
        }

        relative
    };

    let serialized = rebased
        .to_str()
        .context("local import path must be UTF-8")?;

    if rebased.is_absolute() {
        // A different Windows volume requires an absolute fallback; verbatim paths need native separators.
        return Ok(serialized.to_string());
    }

    // Relative manifest paths must travel across hosts without rewriting literal Unix backslashes.
    Ok(serialized.replace(std::path::MAIN_SEPARATOR, "/"))
}

/// Reject an evidence directory that would replace an authored file or transaction control path.
fn validate_asset_prefix(assets: &Path) -> Result<()> {
    sourcefield_workspace::validate_output_path(assets)?;
    let first = assets
        .components()
        .next()
        .and_then(|part| {
            if let Component::Normal(name) = part {
                name.to_str()
            } else {
                None
            }
        })
        .context("assets directory requires a normalized relative path")?;
    ensure!(
        !["profile.toml", "organization.toml"]
            .iter()
            .any(|name| first.eq_ignore_ascii_case(name)),
        "assets directory conflicts with an authored export artifact"
    );

    Ok(())
}

/// Check the explicit new export location without creating or adopting it.
fn export_destination(destination: &Path) -> Result<PathBuf> {
    let parent = checked_workspace_root(
        destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )?;
    let destination = parent.join(
        destination
            .file_name()
            .context("export needs a directory name")?,
    );
    ensure!(
        !destination.exists(),
        "organization extraction destination must be new"
    );

    Ok(destination)
}

/// Publish authored sources and generated evidence only after complete composition succeeds.
fn publish_export(destination: &Path, assets: &Path, candidate: ExportCandidate) -> Result<()> {
    let config_text = toml::to_string_pretty(&candidate.consumer)?;
    let capture = serde_json::to_vec_pretty(&candidate.capture)?;
    let layout = serde_json::to_vec_pretty(&candidate.layout)?;
    let directory = NewExportDirectory::create(destination)?;
    let root = checked_workspace_root(destination)?;
    let mut transaction = Transaction::begin(&root)?;

    // Authored sources survive omission; captures and layout must be replaceable on the next generation.
    transaction.stage_authored("profile.toml", config_text.as_bytes(), None)?;
    transaction.stage_authored("organization.toml", candidate.manifest.as_bytes(), None)?;
    transaction.stage(assets.join("import-capture.json"), &capture)?;
    transaction.stage(assets.join("layout.json"), &layout)?;
    transaction.validate()?;
    transaction.commit()?;
    directory.keep();

    Ok(())
}

/// Exercise inline export without filesystem import resolution in local unit tests.
#[cfg(test)]
fn export(config: &Config, scope: &str, destination: &Path) -> Result<()> {
    let destination = export_destination(destination)?;
    let resolved = crate::imports::ResolvedImports {
        config: config.clone(),
        layout: Default::default(),
        capture: crate::imports::ImportCapture {
            schema_version: 1,
            imports: Vec::new(),
        },
    };
    let candidate = prepare_export(config, &resolved, Path::new("."), scope, &destination)?;

    publish_export(&destination, Path::new("assets"), candidate)
}

/// Own only a newly created export directory until successful publication.
///
/// Removal is deliberately nonrecursive: an interrupted transaction journal or concurrent
/// user file must survive for recovery rather than being erased as cleanup.
struct NewExportDirectory<'a> {
    path: &'a Path,
    published: bool,
}

impl<'a> NewExportDirectory<'a> {
    /// Explicit export paths may be outside the consumer root; create_new semantics prevent adoption.
    fn create(path: &'a Path) -> Result<Self> {
        fs::create_dir(path)?;

        Ok(Self {
            path,
            published: false,
        })
    }

    /// Successful publication transfers ownership of the exported authored inputs to the caller.
    fn keep(mut self) {
        self.published = true;
    }
}

impl Drop for NewExportDirectory<'_> {
    fn drop(&mut self) {
        if !self.published {
            // remove_dir refuses nonempty directories, preserving recovery and concurrent writes.
            let _ = fs::remove_dir(self.path);
        }
    }
}

/// Preserve approved coordinates/radii/weights while moving only canonical facts into the manifest.
#[cfg(test)]
fn split(config: &Config, scope: &str) -> Result<(OrganizationManifest, Config)> {
    split_with_lookup(config, config, scope)
}

/// Split consumer-owned facts while consulting resolved definitions without retaining their copies.
fn split_with_lookup(
    config: &Config,
    lookup: &Config,
    scope: &str,
) -> Result<(OrganizationManifest, Config)> {
    let domain = config
        .domains
        .iter()
        .find(|item| item.id == scope && item.kind == "organization")
        .context("selected organization domain does not exist")?;

    sourcefield_core::validate_config(lookup)?;
    let mut consumer = config.clone();
    let (icons, icon_references) = extract_icons(config, lookup, scope, &mut consumer)?;
    // Affinity itself is an authored fact, even when no project currently references
    // that technology. Selecting only reference targets silently erased these facts.
    let mut technology_ids = config
        .technologies
        .iter()
        .filter(|technology| {
            technology
                .affinities
                .iter()
                .any(|affinity| affinity == scope)
        })
        .map(|technology| technology.id.clone())
        .collect::<BTreeSet<_>>();

    let mut projects = Vec::new();
    let mut publications = Vec::new();

    consumer
        .layout
        .overrides
        .entry(format!("domain:{scope}"))
        .or_insert(domain.anchor);

    for project in config.projects.iter().filter(|item| item.domain == scope) {
        technology_ids.extend(project_references(project).cloned());
        let local_id = local_id(scope, &project.id);
        let key = format!("project:{scope}/{local_id}");
        let original = format!("project:{}", project.id);
        let anchor = consumer
            .layout
            .overrides
            .remove(&original)
            .unwrap_or(project.anchor);
        let weight = consumer
            .layout
            .weights
            .remove(&original)
            .unwrap_or(project.weight);
        let radius = consumer.layout.radii.remove(&original).or(project.radius);
        consumer.layout.overrides.insert(key.clone(), anchor);
        consumer.layout.weights.insert(key.clone(), weight);

        if let Some(radius) = radius {
            consumer.layout.radii.insert(key, radius);
        }

        let mut value = serde_json::to_value(project)?;
        let object = value.as_object_mut().context("project is not an object")?;

        for key in ["domain", "anchor", "weight", "radius"] {
            object.remove(key);
        }

        let mut project: sourcefield_core::OrganizationProject = serde_json::from_value(value)?;
        project.id = local_id.into();
        if let Some(icon) = &mut project.icon
            && let Some(local) = icon_references.get(icon)
        {
            *icon = local.clone();
        }
        local_references(scope, &mut project.implemented_with);
        local_references(scope, &mut project.integrates);
        local_references(scope, &mut project.targets);

        for component in &mut project.components {
            local_references(scope, &mut component.integrates);
            local_references(scope, &mut component.targets);
        }

        projects.push(project);
    }

    for group in config
        .publications
        .iter()
        .filter(|item| item.domain == scope)
    {
        technology_ids.extend(group.technologies.iter().cloned());
        let original = format!("publication:{}", group.id);
        let anchor = consumer
            .layout
            .overrides
            .remove(&original)
            .unwrap_or(group.anchor);
        consumer.layout.overrides.insert(
            format!("publication:{scope}/{}", local_id(scope, &group.id)),
            anchor,
        );
        let mut value = serde_json::to_value(group)?;
        let object = value
            .as_object_mut()
            .context("publication is not an object")?;

        object.remove("domain");
        object.remove("anchor");
        object.insert(
            "owner".into(),
            serde_json::Value::String(
                sourcefield_core::publication_owner(config, group)
                    .unwrap_or(&domain.owner)
                    .to_string(),
            ),
        );

        for package in object
            .get_mut("packages")
            .and_then(serde_json::Value::as_array_mut)
            .context("package array missing")?
        {
            let package = package.as_object_mut().context("package object missing")?;
            let id = package
                .get("id")
                .and_then(serde_json::Value::as_str)
                .context("package ID missing")?
                .to_string();

            if let Some(anchor) = package.remove("anchor").filter(|value| !value.is_null()) {
                consumer
                    .layout
                    .overrides
                    .entry(format!("package:{id}"))
                    .or_insert(serde_json::from_value(anchor)?);
            }
        }

        let mut publication: sourcefield_core::OrganizationPublication =
            serde_json::from_value(value)?;
        publication.id = local_id(scope, &publication.id).to_string();
        // Inline package positions use normalized ID order; canonical manifests
        // assign implicit anchors in declaration order during composition.
        publication
            .packages
            .sort_by(|left, right| left.id.cmp(&right.id));
        local_references(scope, &mut publication.technologies);
        publications.push(publication);
    }

    let consumer_references = config
        .projects
        .iter()
        .filter(|project| project.domain != scope)
        .flat_map(project_references)
        .chain(
            config
                .publications
                .iter()
                .filter(|group| group.domain != scope)
                .flat_map(|group| &group.technologies),
        )
        .chain(config.shared_technologies.values())
        .collect::<BTreeSet<_>>();

    consumer.domains.retain(|item| item.id != scope);
    consumer.projects.retain(|item| item.domain != scope);
    consumer.publications.retain(|item| item.domain != scope);
    consumer.technologies.retain(|item| {
        !technology_ids.contains(&item.id)
            || item.affinities.iter().any(|affinity| affinity != scope)
            || consumer_references.contains(&item.id)
    });

    for technology in &mut consumer.technologies {
        if technology_ids.contains(&technology.id) {
            consumer.shared_technologies.insert(
                format!("{scope}/{}", local_id(scope, &technology.id)),
                technology.id.clone(),
            );
        }

        technology.affinities.retain(|affinity| affinity != scope);
    }

    let technologies = config
        .technologies
        .iter()
        .filter(|item| technology_ids.contains(&item.id))
        .map(|technology| {
            let mut canonical = technology.clone();
            canonical.id = local_id(scope, &canonical.id).to_string();
            // Other consumers' ownership domains are not canonical organization facts.
            // Explicit shared bindings restore those consumer-owned affinities on compose.
            canonical.affinities.retain(|affinity| affinity == scope);

            canonical
        })
        .collect();

    let maintainer = domain.maintainer.clone().or_else(|| {
        (config.profile.variant == sourcefield_core::ProfileVariant::Organization)
            .then(|| config.profile.maintainer.clone())
            .flatten()
    });

    let manifest = OrganizationManifest {
        icons,
        schema_version: sourcefield_core::CONFIG_SCHEMA_VERSION,
        id: domain.id.clone(),
        owner: domain.owner.clone(),
        label: domain.label.clone(),
        summary: domain.summary.clone(),
        maintainer,
        technologies,
        projects,
        publications,
    };

    Ok((manifest, consumer))
}

/// Keep one namespace boundary when exporting either authored or already composed content.
fn local_id<'a>(scope: &str, id: &'a str) -> &'a str {
    id.strip_prefix(scope)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .unwrap_or(id)
}

/// Restore organization-local technology references before the next composition qualifies them.
fn local_references(scope: &str, references: &mut [String]) {
    for reference in references {
        *reference = local_id(scope, reference).to_string();
    }
}

/// Export only consumed definitions and retain a local copy for surviving personal references.
fn extract_icons(
    config: &Config,
    lookup: &Config,
    scope: &str,
    consumer: &mut Config,
) -> Result<(sourcefield_core::IconCatalog, BTreeMap<String, String>)> {
    let mut icons = sourcefield_core::IconCatalog::new();
    let mut references = BTreeMap::new();
    let mut local_keys = BTreeSet::new();
    let selected = config
        .projects
        .iter()
        .filter(|project| project.domain == scope)
        .filter_map(|project| project.icon.as_deref())
        .filter(|key| !key.starts_with("builtin:"))
        .collect::<BTreeSet<_>>();

    let prefix = format!("{scope}/");
    let retained_keys = config
        .icons
        .keys()
        .filter(|key| !selected.contains(key.as_str()))
        .filter_map(|key| key.strip_prefix(&prefix).map(str::to_string))
        .collect::<BTreeSet<_>>();

    // Reserve authored names first so a borrowed namespace cannot take a local icon's name.
    for key in &selected {
        let local = local_id(scope, key);

        if !local.contains('/') {
            sourcefield_core::validate_local_icon_id(local)?;
            ensure!(
                local_keys.insert(local.to_string()),
                "extracted icon local key collision: {local}"
            );
        }
    }

    local_keys.extend(retained_keys.iter().cloned());

    for key in selected {
        let authored_local = local_id(scope, key);
        let foreign = authored_local.contains('/');
        let local = if foreign || retained_keys.contains(authored_local) {
            foreign_icon_key(key, &local_keys)
        } else {
            authored_local.to_string()
        };
        let definition = lookup.icons.get(key).context("selected icon is missing")?;
        local_keys.insert(local.clone());
        icons.insert(local.clone(), definition.clone());
        references.insert(key.to_string(), local.clone());

        if foreign {
            continue;
        }

        consumer.icons.remove(key);
        let shared = consumer
            .projects
            .iter()
            .any(|project| project.domain != scope && project.icon.as_deref() == Some(key));

        if shared {
            let local = personal_icon_key(&consumer.icons, authored_local, definition);
            consumer.icons.insert(local.clone(), definition.clone());

            for project in &mut consumer.projects {
                if project.domain != scope && project.icon.as_deref() == Some(key) {
                    project.icon = Some(local.clone());
                }
            }
        }
    }

    Ok((icons, references))
}

/// Copy a foreign definition under a readable local name or a deterministic collision-free alias.
fn foreign_icon_key(reference: &str, occupied: &BTreeSet<String>) -> String {
    let preferred = reference.replace('/', "-");

    if sourcefield_core::validate_local_icon_id(&preferred).is_ok()
        && !occupied.contains(&preferred)
    {
        return preferred;
    }

    for index in 1..=occupied.len() + 1 {
        let candidate = format!("imported-icon-{index}");

        if !occupied.contains(&candidate) {
            return candidate;
        }
    }

    unreachable!("finite catalog cannot exhaust imported icon keys")
}

/// Deterministically retain a personal copy without overwriting an existing local definition.
fn personal_icon_key(
    catalog: &sourcefield_core::IconCatalog,
    local: &str,
    definition: &sourcefield_core::IconDefinition,
) -> String {
    if catalog
        .get(local)
        .is_none_or(|existing| existing == definition)
    {
        return local.to_string();
    }

    for index in 1..=catalog.len() + 1 {
        let suffix = format!("-personal-{index}");
        let prefix = &local[..local.len().min(64 - suffix.len())];
        let candidate = format!("{prefix}{suffix}");

        if catalog
            .get(&candidate)
            .is_none_or(|existing| existing == definition)
        {
            return candidate;
        }
    }

    unreachable!("finite catalog cannot exhaust personal icon keys")
}

/// Include approved component integrations and targets, not only top-level project references.
fn project_references(project: &sourcefield_core::ProjectConfig) -> impl Iterator<Item = &String> {
    project
        .implemented_with
        .iter()
        .chain(&project.integrates)
        .chain(&project.targets)
        .chain(
            project
                .components
                .iter()
                .flat_map(|component| component.integrates.iter().chain(&component.targets)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sourcefield_core::{EdgeKind, Snapshot, build_state, compose_organizations};

    struct ExtractionDirectory(std::path::PathBuf);

    impl ExtractionDirectory {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "sourcefield-extraction-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));

            fs::create_dir(&path).unwrap();

            Self(fs::canonicalize(path).unwrap())
        }
    }

    impl Drop for ExtractionDirectory {
        fn drop(&mut self) {
            // Cleanup must not replace an assertion failure with a second panic during unwinding.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn fixture_cleanup_removes_directory() {
        // Arrange
        let fixture = ExtractionDirectory::new();
        let path = fixture.0.clone();

        // Act
        drop(fixture);

        // Assert
        assert!(!path.exists());
    }

    #[test]
    fn fixture_cleanup_does_not_panic_when_directory_is_missing() {
        // Arrange
        let fixture = ExtractionDirectory::new();
        fs::remove_dir_all(&fixture.0).unwrap();

        // Act
        let result = std::panic::catch_unwind(|| drop(fixture));

        // Assert
        assert!(result.is_ok(), "fixture cleanup must not introduce a panic");
    }

    #[test]
    fn rebased_relative_imports_use_portable_separators() {
        // Arrange
        let root = Path::new("workspace");
        let cases = [
            ("config", "export", "beta.toml", "../config/beta.toml"),
            (
                "config/nested",
                "out/profile",
                "../beta.toml",
                "../../config/nested/../beta.toml",
            ),
            (
                "config with spaces",
                "export",
                "nested/beta.toml",
                "../config with spaces/nested/beta.toml",
            ),
        ];

        // Act
        let results = cases.map(|(config, destination, source, expected)| {
            (
                rebase_local_path(&root.join(config), &root.join(destination), source),
                expected,
            )
        });

        // Assert
        for (result, expected) in results {
            assert_eq!(result.unwrap(), expected);
        }
    }

    #[test]
    fn authored_absolute_import_path_remains_unchanged() {
        // Arrange
        let source = std::env::temp_dir().join("canonical").join("beta.toml");
        let source = source.to_str().unwrap();

        // Act
        let result = rebase_local_path(Path::new("config"), Path::new("export"), source);

        // Assert
        assert_eq!(result.unwrap(), source);
    }

    #[cfg(unix)]
    #[test]
    fn rebased_import_preserves_literal_unix_backslash() {
        // Arrange
        let config = Path::new("/workspace/config");
        let destination = Path::new("/workspace/export");

        // Act
        let result = rebase_local_path(config, destination, r"beta\signal.toml");

        // Assert
        assert_eq!(result.unwrap(), r"../config/beta\signal.toml");
    }

    #[cfg(unix)]
    #[test]
    fn rebased_import_rejects_non_utf8_directory() {
        use std::os::unix::ffi::OsStringExt;

        // Arrange
        let config = Path::new("/workspace").join(std::ffi::OsString::from_vec(vec![b'b', 0xff]));

        // Act
        let result = rebase_local_path(&config, Path::new("/workspace/export"), "beta.toml");

        // Assert
        assert!(result.unwrap_err().to_string().contains("must be UTF-8"));
    }

    #[cfg(windows)]
    #[test]
    fn rebased_import_from_verbatim_base_uses_portable_relative_path() {
        // Arrange
        let config = Path::new(r"\\?\C:\workspace\config");
        let destination = Path::new(r"\\?\C:\workspace\export");

        // Act
        let result = rebase_local_path(config, destination, r"nested\beta.toml");

        // Assert
        assert_eq!(result.unwrap(), "../config/nested/beta.toml");
    }

    #[cfg(windows)]
    #[test]
    fn authored_windows_absolute_paths_keep_native_prefixes() {
        // Arrange
        let sources = [r"C:\canonical\beta.toml", r"\\?\C:\canonical\beta.toml"];
        let config = Path::new(r"\\?\C:\workspace\config");
        let destination = Path::new(r"\\?\C:\workspace\export");

        // Act
        let results = sources.map(|source| rebase_local_path(config, destination, source));

        // Assert
        for (result, expected) in results.into_iter().zip(sources) {
            assert_eq!(result.unwrap(), expected);
        }
    }

    #[cfg(windows)]
    #[test]
    fn different_volume_fallback_keeps_native_verbatim_path() {
        // Arrange
        let config = Path::new(r"\\?\C:\workspace\config");
        let destination = Path::new(r"\\?\D:\export");

        // Act
        let result = rebase_local_path(config, destination, "beta.toml");

        // Assert
        assert_eq!(result.unwrap(), r"\\?\C:\workspace\config\beta.toml");
    }

    #[tokio::test]
    async fn generation_preserves_extracted_authored_configuration_and_manifest() {
        // Arrange
        let directory = ExtractionDirectory::new();
        let root = directory.0.join("consumer");
        export(&config(), "sample-labs", &root).unwrap();
        let profile = fs::read(root.join("profile.toml")).unwrap();
        let organization = fs::read(root.join("organization.toml")).unwrap();
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // Act
        crate::generate(crate::GenerateOptions {
            root: &root,
            locked: false,
            runtime: None,
            adopt_existing: false,
            config_path: &root.join("profile.toml"),
            fallback_snapshot_path: &fixture_root.join("config/offline-snapshot.json"),
            assets_dir: &root.join("assets"),
            docs_dir: &root.join("docs"),
            readme_paths: &[],
            offline: true,
            strict_live: false,
            private_counts: false,
            no_history: false,
        })
        .await
        .unwrap();

        // Assert
        assert_eq!(
            fs::read(root.join("profile.toml")).expect("authored profile must survive generation"),
            profile
        );
        assert_eq!(
            fs::read(root.join("organization.toml"))
                .expect("canonical manifest must survive generation"),
            organization
        );
    }

    fn config() -> Config {
        toml::from_str(include_str!("../../../config/profile.toml")).unwrap()
    }

    fn normalized_endpoint(value: &str) -> String {
        value.replace("sample-labs/", "")
    }

    #[test]
    fn extraction_preserves_all_technology_facts_and_edges_without_duplicate_shared_nodes() {
        // Arrange
        let config = config();
        let expected = build_state(&config, &Snapshot::default(), "same-time").unwrap();
        let (manifest, consumer) = split(&config, "sample-labs").unwrap();

        // Act
        let (resolved, _) =
            compose_organizations(&consumer, &[manifest], &Default::default()).unwrap();

        let actual = build_state(&resolved, &Snapshot::default(), "same-time").unwrap();

        // Assert
        let technology_facts = |config: &Config| {
            config
                .technologies
                .iter()
                .map(|technology| {
                    let mut technology = technology.clone();
                    technology.id = normalized_endpoint(&technology.id);
                    technology.affinities.sort();

                    (
                        technology.id.clone(),
                        serde_json::to_string(&technology).unwrap(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };

        let edges = |state: &sourcefield_core::ProfileState| {
            state
                .edges
                .iter()
                .map(|edge| {
                    (
                        normalized_endpoint(&edge.from),
                        normalized_endpoint(&edge.to),
                        serde_json::to_string(&edge.kind).unwrap(),
                    )
                })
                .collect::<BTreeSet<_>>()
        };

        assert_eq!(technology_facts(&resolved), technology_facts(&config));
        assert_eq!(actual.nodes.len(), expected.nodes.len());
        assert_eq!(edges(&actual), edges(&expected));
        assert!(
            actual
                .edges
                .iter()
                .any(|edge| edge.kind == EdgeKind::SharedCapability)
        );
    }

    #[test]
    fn extraction_preserves_affinity_without_project_reference() {
        // Arrange
        let mut config = config();
        let mut extra = config.technologies[0].clone();
        extra.id = "affinity-only".into();
        extra.affinities = vec!["sample-labs".into()];
        config.technologies.push(extra);

        // Act
        let (manifest, _) = split(&config, "sample-labs").unwrap();

        // Assert
        assert!(
            manifest
                .technologies
                .iter()
                .any(|technology| technology.id == "affinity-only")
        );
    }

    #[test]
    fn extraction_preserves_component_only_reference_without_inventing_affinity() {
        // Arrange
        let mut config = config();
        let mut extra = config.technologies[0].clone();
        extra.id = "component-only".into();
        extra.affinities.clear();
        config.technologies.push(extra);
        let mut component = config.projects[0].components[0].clone();
        component.targets = vec!["component-only".into()];
        component.integrates.clear();
        config
            .projects
            .iter_mut()
            .find(|project| project.domain == "sample-labs")
            .unwrap()
            .components
            .push(component);
        let (manifest, consumer) = split(&config, "sample-labs").unwrap();

        // Act
        let (resolved, _) =
            compose_organizations(&consumer, &[manifest], &Default::default()).unwrap();

        // Assert
        let technology = resolved
            .technologies
            .iter()
            .find(|technology| technology.id == "sample-labs/component-only")
            .unwrap();

        assert!(technology.affinities.is_empty());
        assert!(
            resolved
                .projects
                .iter()
                .flat_map(|project| &project.components)
                .any(|component| component
                    .targets
                    .iter()
                    .any(|target| target == "sample-labs/component-only"))
        );
    }

    #[test]
    fn extraction_rejects_missing_organization_scope() {
        // Arrange
        let config = config();

        // Act
        let result = split(&config, "absent-scope");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn canonical_recomposition_preserves_main_project_geometry() {
        // Arrange
        let config = config();
        let (manifest, consumer) = split(&config, "sample-labs").unwrap();

        // Act
        let (resolved, _) =
            compose_organizations(&consumer, &[manifest], &Default::default()).unwrap();

        // Assert
        for expected in &config.projects {
            let actual = resolved
                .projects
                .iter()
                .find(|project| normalized_endpoint(&project.id) == expected.id)
                .unwrap();

            assert_eq!(actual.anchor, expected.anchor);
            assert_eq!(actual.radius, expected.radius);
            assert_eq!(actual.weight, expected.weight);
        }

        assert_eq!(resolved.render.height, config.render.height);
    }
    #[test]
    fn failed_export_setup_removes_only_its_empty_directory() {
        let directory = ExtractionDirectory::new();
        let target = directory.0.join("new-export");

        {
            let _guard = NewExportDirectory::create(&target).unwrap();
        }

        assert!(!target.exists());
    }

    #[test]
    fn failed_export_cleanup_preserves_concurrent_content() {
        let directory = ExtractionDirectory::new();
        let target = directory.0.join("new-export");
        let guard = NewExportDirectory::create(&target).unwrap();
        fs::write(target.join("authored.txt"), "keep").unwrap();

        drop(guard);

        assert_eq!(
            fs::read_to_string(target.join("authored.txt")).unwrap(),
            "keep"
        );
    }
    #[cfg(unix)]
    #[test]
    fn export_rejects_symlink_parent_before_creating_destination() {
        let directory = ExtractionDirectory::new();
        let actual = directory.0.join("actual");
        fs::create_dir(&actual).unwrap();
        let alias = directory.0.join("alias");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();

        let result = export(&config(), "sample-labs", &alias.join("export"));

        assert!(result.is_err());
        assert!(!actual.join("export").exists());
    }

    fn icon() -> sourcefield_core::IconDefinition {
        serde_json::from_value(serde_json::json!({
            "radius": 16,
            "elements": [{"geometry": {"shape": "circle", "center": [0, 0], "radius": 4}}]
        }))
        .unwrap()
    }

    #[test]
    fn extraction_keeps_local_icon_copy_for_personal_projects() {
        // Arrange
        let mut config = config();
        config.icons.insert("signal".into(), icon());
        config
            .projects
            .iter_mut()
            .find(|project| project.domain == "sample-labs")
            .unwrap()
            .icon = Some("signal".into());
        config
            .projects
            .iter_mut()
            .find(|project| project.domain != "sample-labs")
            .unwrap()
            .icon = Some("signal".into());

        // Act
        let (manifest, consumer) = split(&config, "sample-labs").unwrap();
        let (resolved, _) = compose_organizations(
            &consumer,
            std::slice::from_ref(&manifest),
            &Default::default(),
        )
        .unwrap();

        // Assert
        assert_eq!(manifest.icons.get("signal"), config.icons.get("signal"));
        assert_eq!(consumer.icons.get("signal"), config.icons.get("signal"));
        assert_eq!(resolved.icons.len(), 2);
        assert!(
            resolved
                .projects
                .iter()
                .any(|project| project.domain == "sample-labs"
                    && project.icon.as_deref() == Some("sample-labs/signal"))
        );
        assert!(
            resolved
                .projects
                .iter()
                .any(|project| project.domain != "sample-labs"
                    && project.icon.as_deref() == Some("signal"))
        );
    }

    #[test]
    fn extracting_composed_shared_icon_preserves_other_local_definition() {
        // Arrange
        let mut config = config();
        config.icons.insert("signal".into(), icon());
        config
            .projects
            .iter_mut()
            .find(|project| project.domain == "sample-labs")
            .unwrap()
            .icon = Some("signal".into());
        let (manifest, consumer) = split(&config, "sample-labs").unwrap();
        let (mut composed, _) =
            compose_organizations(&consumer, &[manifest], &Default::default()).unwrap();
        composed
            .projects
            .iter_mut()
            .find(|project| project.domain != "sample-labs")
            .unwrap()
            .icon = Some("sample-labs/signal".into());
        let mut other = icon();
        other.radius = 17.0;
        composed.icons.insert("signal".into(), other.clone());

        // Act
        let (manifest, consumer) = split(&composed, "sample-labs").unwrap();
        let (resolved, _) = compose_organizations(
            &consumer,
            std::slice::from_ref(&manifest),
            &Default::default(),
        )
        .unwrap();
        let state = build_state(&resolved, &Snapshot::default(), "same-time").unwrap();

        // Assert
        assert_eq!(
            manifest.projects[0].id,
            local_id(
                "sample-labs",
                &composed
                    .projects
                    .iter()
                    .find(|project| project.domain == "sample-labs")
                    .unwrap()
                    .id
            )
        );
        assert_eq!(consumer.icons.get("signal"), Some(&other));
        assert_eq!(consumer.icons.get("signal-personal-1"), Some(&icon()));
        assert!(
            consumer
                .projects
                .iter()
                .any(|project| project.icon.as_deref() == Some("signal-personal-1"))
        );
        assert!(
            state
                .nodes
                .iter()
                .all(|node| !node.id.contains("sample-labs/sample-labs/"))
        );
        assert_eq!(resolved.icons.get("sample-labs/signal"), Some(&icon()));
    }

    #[test]
    fn extraction_rejects_ambiguous_local_icon_names() {
        // Arrange
        let mut config = config();
        config.icons.insert("signal".into(), icon());
        config.icons.insert("sample-labs/signal".into(), icon());
        let mut projects = config
            .projects
            .iter_mut()
            .filter(|project| project.domain == "sample-labs");
        projects.next().unwrap().icon = Some("signal".into());
        projects.next().unwrap().icon = Some("sample-labs/signal".into());

        // Act
        let result = split(&config, "sample-labs");

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("local key collision")
        );
    }

    #[test]
    fn extraction_rekeys_original_layout_overrides() {
        // Arrange
        let mut config = config();
        let project = config
            .projects
            .iter()
            .find(|project| project.domain == "sample-labs")
            .unwrap();
        let project_key = format!("project:{}", project.id);
        config
            .layout
            .overrides
            .insert(project_key.clone(), project.anchor);
        config.layout.radii.insert(project_key.clone(), 47.0);
        config
            .layout
            .weights
            .insert(project_key.clone(), project.weight);
        let publication = config
            .publications
            .iter()
            .find(|group| group.domain == "sample-labs")
            .unwrap();
        let publication_key = format!("publication:{}", publication.id);
        config
            .layout
            .overrides
            .insert(publication_key.clone(), publication.anchor);

        // Act
        let (manifest, consumer) = split(&config, "sample-labs").unwrap();
        let result = compose_organizations(&consumer, &[manifest], &Default::default());

        // Assert
        assert!(result.is_ok(), "{result:?}");
        assert!(!consumer.layout.overrides.contains_key(&project_key));
        assert!(!consumer.layout.radii.contains_key(&project_key));
        assert_eq!(
            consumer
                .layout
                .radii
                .get(&project_key.replacen("project:", "project:sample-labs/", 1)),
            Some(&47.0)
        );
        assert!(!consumer.layout.weights.contains_key(&project_key));
        assert!(!consumer.layout.overrides.contains_key(&publication_key));
    }
}
