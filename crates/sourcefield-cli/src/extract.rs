//! Separate organization facts from consumer-owned presentation during migration.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use sourcefield_core::{Config, OrganizationImport, OrganizationManifest, OrganizationSource};
use sourcefield_workspace::{Transaction, checked_workspace_root};

/// Export one manifest and a consumer configuration into a new, independently validated tree.
pub(crate) fn export(config: &Config, scope: &str, destination: &Path) -> Result<()> {
    let (manifest, mut consumer) = split(config, scope)?;
    consumer.imports.push(OrganizationImport {
        id: scope.to_string(),
        source: OrganizationSource::Local {
            path: "organization.toml".into(),
        },
    });
    let config_text = toml::to_string_pretty(&consumer)?;
    let manifest_text = toml::to_string_pretty(&manifest)?;
    let (resolved, _) =
        sourcefield_core::compose_organizations(&consumer, &[manifest], &Default::default())?;

    sourcefield_core::validate_config(&resolved)?;
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
    let directory = NewExportDirectory::create(&destination)?;
    let root = checked_workspace_root(&destination)?;
    let mut transaction = Transaction::begin(&root)?;

    // These files become authored inputs. Granting generated-file ownership would let
    // the next generation delete them when they are absent from its output inventory.
    transaction.stage_authored("profile.toml", config_text.as_bytes(), None)?;
    transaction.stage_authored("organization.toml", manifest_text.as_bytes(), None)?;
    transaction.validate()?;
    transaction.commit()?;
    directory.keep();

    Ok(())
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
fn split(config: &Config, scope: &str) -> Result<(OrganizationManifest, Config)> {
    let domain = config
        .domains
        .iter()
        .find(|item| item.id == scope && item.kind == "organization")
        .context("selected organization domain does not exist")?;

    let mut consumer = config.clone();
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
        .insert(format!("domain:{scope}"), domain.anchor);

    for project in config.projects.iter().filter(|item| item.domain == scope) {
        technology_ids.extend(project_references(project).cloned());
        let key = format!("project:{scope}/{}", project.id);
        consumer
            .layout
            .overrides
            .insert(key.clone(), project.anchor);
        consumer.layout.weights.insert(key.clone(), project.weight);

        if let Some(radius) = project.radius {
            consumer.layout.radii.insert(key, radius);
        }

        let mut value = serde_json::to_value(project)?;
        let object = value.as_object_mut().context("project is not an object")?;

        for key in ["domain", "anchor", "weight", "radius"] {
            object.remove(key);
        }

        projects.push(serde_json::from_value(value)?);
    }

    for group in config
        .publications
        .iter()
        .filter(|item| item.domain == scope)
    {
        technology_ids.extend(group.technologies.iter().cloned());
        consumer
            .layout
            .overrides
            .insert(format!("publication:{scope}/{}", group.id), group.anchor);
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
                    .insert(format!("package:{id}"), serde_json::from_value(anchor)?);
            }
        }

        publications.push(serde_json::from_value(value)?);
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
            consumer
                .shared_technologies
                .insert(format!("{scope}/{}", technology.id), technology.id.clone());
        }

        technology.affinities.retain(|affinity| affinity != scope);
    }

    let technologies = config
        .technologies
        .iter()
        .filter(|item| technology_ids.contains(&item.id))
        .map(|technology| {
            let mut canonical = technology.clone();
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
            fs::remove_dir_all(&self.0).unwrap();
        }
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
}
