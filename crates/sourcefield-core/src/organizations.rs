//! Canonical organization facts and deterministic consumer-owned placement.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ComponentConfig, Config, DomainConfig, PackageConfig, ProjectConfig, PublicationConfig,
    TechnologyConfig, ValidationError, Visibility, validate_config,
};

/// Supported authored configuration and organization manifest version.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
/// Unified generated state format produced after the one-time migration.
pub const STATE_SCHEMA_VERSION: u32 = 3;
/// Persisted placement assignments keyed by fully scoped graph node ID.
pub type LayoutAssignments = BTreeMap<String, [f32; 2]>;

/// Canonical organization content, independent of consumer geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationManifest {
    /// Organization-local icon definitions qualified together with project references.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icons: crate::IconCatalog,
    /// Public maintainer attribution shared by all consumers of this organization.
    #[serde(default)]
    pub maintainer: Option<crate::MaintainerConfig>,
    /// Explicit content format version.
    pub schema_version: u32,
    /// Stable consumer-independent namespace; changing the display name does not change it.
    pub id: String,
    /// Public GitHub organization account.
    pub owner: String,
    /// Approved organization display name.
    pub label: String,
    /// Approved public organization description.
    pub summary: String,
    /// Organization-local technology definitions.
    #[serde(default)]
    pub technologies: Vec<TechnologyConfig>,
    /// Organization-local project definitions without coordinates.
    #[serde(default)]
    pub projects: Vec<OrganizationProject>,
    /// Organization-local registry groups without coordinates.
    #[serde(default)]
    pub publications: Vec<OrganizationPublication>,
}

/// Canonical project facts shared by every consumer profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationProject {
    /// Optional built-in or organization-local icon key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Stable organization-local project identifier.
    pub id: String,
    /// Full accessible project label.
    pub label: String,
    /// Compact visual project label.
    pub surface_label: String,
    /// Optional display namespace.
    #[serde(default)]
    pub label_prefix: Option<String>,
    /// Approved public or curated private boundary.
    pub visibility: Visibility,
    /// Approved project lifecycle label.
    pub status: String,
    /// Decorative glyph identifier.
    pub visual: String,
    /// Approved public summary.
    pub summary: String,
    /// Public repository identity; absent for curated private content.
    #[serde(default)]
    pub repository: Option<String>,
    /// Approved display stack in authored order.
    #[serde(default)]
    pub display_stack: Vec<String>,
    /// Organization-local implementation technology identifiers.
    #[serde(default)]
    pub implemented_with: Vec<String>,
    /// Organization-local integration technology identifiers.
    #[serde(default)]
    pub integrates: Vec<String>,
    /// Organization-local target technology identifiers.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Approved categorization labels.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Include this project in the managed README projection.
    #[serde(default)]
    pub show_in_readme: bool,
    /// Approved high-level component descriptions.
    #[serde(default)]
    pub components: Vec<ComponentConfig>,
}

/// Canonical registry publication facts owned by one organization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationPublication {
    /// Explicit compact labels for package IDs whose names do not share a uniform prefix.
    #[serde(default)]
    pub label_overrides: std::collections::BTreeMap<String, String>,
    /// Stable organization-local group identifier, matching its publishing project.
    pub id: String,
    /// Full accessible package group label.
    pub label: String,
    /// Compact visual heading.
    pub surface_label: String,
    /// Public registry identifier.
    pub registry: String,
    /// Verified owner in that registry.
    pub owner: String,
    /// Approved group summary.
    pub summary: String,
    /// Allowed exact package roots and their dot-separated descendants.
    #[serde(default)]
    pub discovery_prefixes: Vec<String>,
    /// Exact prefix removed from compact package labels.
    #[serde(default)]
    pub label_prefix: String,
    /// Optional display label for the package matching the full group label.
    #[serde(default)]
    pub core_label: Option<String>,
    /// Organization-local technology identifiers.
    #[serde(default)]
    pub technologies: Vec<String>,
    /// Include this package group in the managed README projection.
    #[serde(default)]
    pub show_in_readme: bool,
    /// Explicitly selected package facts.
    #[serde(default)]
    pub packages: Vec<OrganizationPackage>,
}

/// A selected public package without consumer-owned positioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationPackage {
    /// Public registry package ID.
    pub id: String,
    /// Approved family label.
    pub family: String,
    /// Public HTTPS package URL.
    pub url: String,
    /// Approved public description.
    #[serde(default)]
    pub summary: String,
}

/// Parse a strict versioned manifest; composed references are checked during composition.
pub fn parse_organization(text: &str) -> Result<OrganizationManifest, crate::ConfigError> {
    let manifest: OrganizationManifest = toml::from_str(text)?;

    if manifest.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(crate::ConfigError::Validation(
            ValidationError::UnsupportedVersion,
        ));
    }

    validate_local_id(&manifest.id).map_err(crate::ConfigError::Validation)?;
    validate_manifest_icons(&manifest).map_err(crate::ConfigError::Validation)?;

    Ok(manifest)
}

/// Compose selected organization facts and stable geometry without mutating inputs.
///
/// Previous assignments retain existing project positions. New projects take the first free
/// slot in stable ID order. Authored overrides win; stale overrides fail rather than silently
/// hiding a rename or removal. Removed generated assignments are deliberately discarded.
pub fn compose_organizations(
    base: &Config,
    organizations: &[OrganizationManifest],
    previous: &LayoutAssignments,
) -> Result<(Config, LayoutAssignments), ValidationError> {
    crate::validate_icon_catalog(&base.icons)?;
    let mut config = base.clone();
    let mut assignments = BTreeMap::new();
    let mut unused_bindings = base
        .shared_technologies
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    if organizations.is_empty() {
        if !unused_bindings.is_empty() {
            return Err(ValidationError::InvalidValue(
                "shared technology bindings require selected canonical organizations".into(),
            ));
        }

        apply_overrides(&mut config, &mut assignments)?;
        validate_config(&config)?;

        return Ok((config, assignments));
    }

    let mut scopes = config
        .domains
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();

    let mut field_bottom = config
        .projects
        .iter()
        .map(|item| item.anchor[1] + 150.0)
        .chain(config.domains.iter().map(|item| item.anchor[1] + 150.0))
        .fold(280.0_f32, f32::max);

    for organization in organizations {
        if organization.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedVersion);
        }

        validate_local_id(&organization.id)?;
        validate_manifest_icons(organization)?;

        for (key, definition) in &organization.icons {
            let key = scoped(&organization.id, key);

            if config
                .icons
                .insert(key.clone(), definition.clone())
                .is_some()
            {
                return Err(ValidationError::DuplicateId(key));
            }
        }

        crate::validate_icon_catalog(&config.icons)?;

        if !scopes.insert(organization.id.clone()) {
            return Err(ValidationError::DuplicateId(organization.id.clone()));
        }

        let start_y = field_bottom + 120.0;
        let domain_key = format!("domain:{}", organization.id);
        let shift = if base.layout.overrides.contains_key(&domain_key) {
            0.0
        } else {
            previous
                .get(&domain_key)
                .map_or(0.0, |anchor| (start_y - anchor[1]).max(0.0))
        };

        let project_prefix = format!("project:{}/", organization.id);
        let previous = previous
            .iter()
            .filter(|(key, _)| key.as_str() == domain_key || key.starts_with(&project_prefix))
            .map(|(key, anchor)| (key.clone(), [anchor[0], anchor[1] + shift]))
            .collect::<LayoutAssignments>();

        // Preserve positions within each organization while moving the whole region down
        // when a preceding organization grows. This avoids cross-organization collisions.

        let domain_anchor = placement(
            &domain_key,
            [config.render.width as f32 / 2.0, start_y],
            base,
            &previous,
        );

        // Empty organizations still own visible domain geometry and must reserve their region.
        field_bottom = field_bottom.max(domain_anchor[1] + 160.0);
        assignments.insert(domain_key, domain_anchor);
        config.domains.push(DomainConfig {
            maintainer: organization.maintainer.clone(),
            id: organization.id.clone(),
            label: organization.label.clone(),
            owner: organization.owner.clone(),
            kind: "organization".into(),
            anchor: domain_anchor,
            summary: organization.summary.clone(),
        });

        if !config
            .collection
            .github_organizations
            .iter()
            .any(|owner| owner.eq_ignore_ascii_case(&organization.owner))
        {
            config
                .collection
                .github_organizations
                .push(organization.owner.clone());
        }

        let mut technology_ids = BTreeSet::new();

        for technology in &organization.technologies {
            validate_local_id(&technology.id)?;
            if technology
                .affinities
                .iter()
                .any(|affinity| affinity != &organization.id)
            {
                return Err(ValidationError::UnknownDomain {
                    domain: technology
                        .affinities
                        .iter()
                        .find(|affinity| *affinity != &organization.id)
                        .cloned()
                        .unwrap_or_default(),
                    owner: format!("{}/{}", organization.id, technology.id),
                });
            }

            let canonical_id = scoped(&organization.id, &technology.id);

            if !technology_ids.insert(technology.id.as_str()) {
                return Err(ValidationError::DuplicateId(canonical_id));
            }

            if let Some(target) = base.shared_technologies.get(&canonical_id) {
                let shared = config
                    .technologies
                    .iter_mut()
                    .find(|item| item.id == *target)
                    .ok_or_else(|| ValidationError::UnknownTechnology {
                        technology: target.clone(),
                        owner: canonical_id.clone(),
                    })?;

                if shared.label != technology.label || shared.category != technology.category {
                    return Err(ValidationError::InvalidValue(format!(
                        "shared technology definition conflicts: {canonical_id} -> {target}"
                    )));
                }

                if technology.affinities.contains(&organization.id)
                    && !shared.affinities.contains(&organization.id)
                {
                    shared.affinities.push(organization.id.clone());
                }

                shared.cross_domain |= technology.cross_domain;
                unused_bindings.remove(&canonical_id);
            } else {
                let mut technology = technology.clone();
                technology.id = canonical_id;
                config.technologies.push(technology);
            }
        }

        let mut projects = organization.projects.iter().collect::<Vec<_>>();
        projects.sort_by(|left, right| left.id.cmp(&right.id));
        let mut occupied = vec![(domain_anchor, 95.0)];

        // Reserve all established positions before finding slots: insertion order must not
        // allow a new project to steal the position of an existing project later in the list.
        for project in &projects {
            let key = format!("project:{}/{}", organization.id, project.id);

            if let Some(anchor) = base
                .layout
                .overrides
                .get(&key)
                .or_else(|| previous.get(&key))
            {
                occupied.push((*anchor, 95.0));
            }
        }

        for project in projects {
            validate_local_id(&project.id)?;
            let id = scoped(&organization.id, &project.id);
            let key = format!("project:{id}");
            let anchor = match base
                .layout
                .overrides
                .get(&key)
                .or_else(|| previous.get(&key))
            {
                Some(anchor) => *anchor,
                None => free_slot(config.render.width, start_y + 210.0, &occupied)?,
            };

            occupied.push((anchor, 95.0));
            field_bottom = field_bottom
                .max(anchor[1] + 160.0)
                .max(domain_anchor[1] + 160.0);
            assignments.insert(key, anchor);
            let map_ids = |ids: &[String]| {
                ids.iter()
                    .map(|id| technology_reference(base, &organization.id, id))
                    .collect()
            };

            let mut components = project.components.clone();

            for component in &mut components {
                component.integrates = map_ids(&component.integrates);
                component.targets = map_ids(&component.targets);
            }

            config.projects.push(ProjectConfig {
                icon: project.icon.as_ref().map(|icon| {
                    if icon.starts_with("builtin:") {
                        icon.clone()
                    } else {
                        scoped(&organization.id, icon)
                    }
                }),
                id,
                label: project.label.clone(),
                surface_label: project.surface_label.clone(),
                domain: organization.id.clone(),
                visibility: project.visibility,
                status: project.status.clone(),
                visual: project.visual.clone(),
                anchor,
                weight: 0.5,
                radius: None,
                summary: project.summary.clone(),
                repository: project.repository.clone(),
                implemented_with: map_ids(&project.implemented_with),
                integrates: map_ids(&project.integrates),
                targets: map_ids(&project.targets),
                tags: project.tags.clone(),
                show_in_readme: project.show_in_readme,
                components,
                display_stack: project.display_stack.clone(),
                label_prefix: project.label_prefix.clone(),
            });
        }
    }

    if config.profile.variant == crate::ProfileVariant::Organization {
        if organizations.len() > 1 {
            return Err(ValidationError::InvalidValue(
                "organization profile must select exactly one organization".into(),
            ));
        }

        if let Some(organization) = organizations.first() {
            config.profile.maintainer = organization.maintainer.clone();
        }
    }

    let mut group_y = field_bottom + 160.0;
    let mut column = 0;
    let mut row_height = 0.0_f32;

    for organization in organizations {
        for publication in &organization.publications {
            validate_local_id(&publication.id)?;
            let id = scoped(&organization.id, &publication.id);
            let key = format!("publication:{id}");
            let x = config.render.width as f32 * (column as f32 + 0.5) / 3.0;
            let anchor = placement(&key, [x, group_y], base, &BTreeMap::new());
            assignments.insert(key, anchor);
            let packages = publication
                .packages
                .iter()
                .enumerate()
                .map(|(index, package)| PackageConfig {
                    id: package.id.clone(),
                    family: package.family.clone(),
                    summary: package.summary.clone(),
                    url: package.url.clone(),
                    anchor: Some(crate::default_package_anchor(anchor, index)),
                })
                .collect();

            config.publications.push(PublicationConfig {
                label_overrides: publication.label_overrides.clone(),
                id,
                label: publication.label.clone(),
                surface_label: publication.surface_label.clone(),
                domain: organization.id.clone(),
                registry: publication.registry.clone(),
                owner: Some(publication.owner.clone()),
                anchor,
                summary: publication.summary.clone(),
                technologies: publication
                    .technologies
                    .iter()
                    .map(|id| technology_reference(base, &organization.id, id))
                    .collect(),
                show_in_readme: publication.show_in_readme,
                discovery_prefixes: publication.discovery_prefixes.clone(),
                packages,
                label_prefix: publication.label_prefix.clone(),
                core_label: publication.core_label.clone(),
            });
            row_height = row_height
                .max(150.0 + publication.packages.len() as f32 * crate::PACKAGE_ROW_SPACING);
            column += 1;

            if column == 3 {
                group_y += row_height;
                row_height = 0.0;
                column = 0;
            }
        }
    }

    if let Some(source) = unused_bindings.first() {
        return Err(ValidationError::InvalidValue(format!(
            "shared technology binding references absent canonical technology: {source}"
        )));
    }

    apply_overrides(&mut config, &mut assignments)?;
    validate_project_collisions(&config, organizations)?;
    validate_cross_domain_collisions(&config)?;
    // The approved separator is 115 pixels below the last package center: 51 to
    // the version baseline plus a 64-pixel gap. Footer blocks differ by variant.
    let package_padding = match config.profile.variant {
        crate::ProfileVariant::Personal => 458.0,
        crate::ProfileVariant::Organization => 180.0,
    };

    let bottom = config
        .projects
        .iter()
        .map(|item| item.anchor[1] + 180.0)
        .chain(config.domains.iter().map(|item| item.anchor[1] + 180.0))
        .chain(
            config
                .publications
                .iter()
                .map(|group| group.anchor[1] + package_padding + 50.0),
        )
        .chain(config.publications.iter().flat_map(|group| {
            group
                .packages
                .iter()
                .filter_map(|package| package.anchor.map(|anchor| anchor[1] + package_padding))
        }))
        .fold(config.render.height as f32, f32::max);

    if !bottom.is_finite() || bottom > 100_000.0 {
        return Err(ValidationError::InvalidValue(
            "layout exceeds 100000-pixel safety boundary".into(),
        ));
    }

    config.render.height = config.render.height.max(bottom.ceil() as u32);
    validate_config(&config)?;

    Ok((config, assignments))
}

/// Locate a deterministic nonoverlapping project slot, leaving room for its title.
fn free_slot(
    width: u32,
    top: f32,
    occupied: &[([f32; 2], f32)],
) -> Result<[f32; 2], ValidationError> {
    for row in 0..400 {
        for column in 0..3 {
            let anchor = [
                width as f32 * (column as f32 + 0.5) / 3.0,
                top + row as f32 * 240.0,
            ];

            if occupied.iter().all(|(other, radius)| {
                (other[0] - anchor[0]).hypot(other[1] - anchor[1]) >= radius + 95.0
            }) {
                return Ok(anchor);
            }
        }
    }

    Err(ValidationError::InvalidValue(
        "no project placement below layout safety boundary".into(),
    ))
}

fn placement(
    key: &str,
    default: [f32; 2],
    base: &Config,
    previous: &LayoutAssignments,
) -> [f32; 2] {
    base.layout
        .overrides
        .get(key)
        .or_else(|| previous.get(key))
        .copied()
        .unwrap_or(default)
}

fn scoped(scope: &str, id: &str) -> String {
    format!("{scope}/{id}")
}

/// Preserve organization scopes and organization-local IDs across their shared consumers.
pub(crate) fn validate_local_id(id: &str) -> Result<(), ValidationError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(ValidationError::InvalidValue(format!(
            "invalid local identifier: {id}"
        )));
    }

    Ok(())
}

/// Resolve overrides only against selected content so stale identifiers cannot hide mistakes.
fn apply_overrides(
    config: &mut Config,
    assignments: &mut LayoutAssignments,
) -> Result<(), ValidationError> {
    let mut radius_ids = config.layout.radii.keys().cloned().collect::<BTreeSet<_>>();
    let mut weight_ids = config
        .layout
        .weights
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    for project in &mut config.projects {
        let key = format!("project:{}", project.id);

        if let Some(radius) = config.layout.radii.get(&key) {
            project.radius = Some(*radius);
            radius_ids.remove(&key);
        }

        if let Some(weight) = config.layout.weights.get(&key) {
            project.weight = *weight;
            weight_ids.remove(&key);
        }
    }

    if !radius_ids.is_empty() || !weight_ids.is_empty() {
        return Err(ValidationError::InvalidValue(
            "project radius or weight override references absent item".into(),
        ));
    }

    let mut remaining = config
        .layout
        .overrides
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    for (key, anchor) in config
        .domains
        .iter_mut()
        .map(|item| (format!("domain:{}", item.id), &mut item.anchor))
        .chain(
            config
                .projects
                .iter_mut()
                .map(|item| (format!("project:{}", item.id), &mut item.anchor)),
        )
        .chain(
            config
                .publications
                .iter_mut()
                .map(|item| (format!("publication:{}", item.id), &mut item.anchor)),
        )
    {
        if let Some(value) = config.layout.overrides.get(&key) {
            *anchor = *value;
            remaining.remove(&key);
        }

        assignments.insert(key, *anchor);
    }

    for package in config
        .publications
        .iter_mut()
        .flat_map(|group| &mut group.packages)
    {
        let key = format!("package:{}", package.id);

        if let Some(value) = config.layout.overrides.get(&key) {
            package.anchor = Some(*value);
            remaining.remove(&key);
            assignments.insert(key, *value);
        }
    }

    if let Some(key) = remaining.first() {
        return Err(ValidationError::InvalidValue(format!(
            "layout override references absent item: {key}"
        )));
    }

    Ok(())
}

/// Authored and retained placements are authoritative but must still fit without overlapping.
fn validate_project_collisions(
    config: &Config,
    organizations: &[OrganizationManifest],
) -> Result<(), ValidationError> {
    for organization in organizations {
        let projects = config
            .projects
            .iter()
            .filter(|project| project.domain == organization.id)
            .collect::<Vec<_>>();

        let domain = config
            .domains
            .iter()
            .find(|domain| domain.id == organization.id)
            .expect("composed domain");

        for (index, project) in projects.iter().enumerate() {
            let radius = crate::project_radius(project);

            if project.anchor[0] < radius
                || project.anchor[0] + radius > config.render.width as f32
                || (project.anchor[0] - domain.anchor[0])
                    .hypot(project.anchor[1] - domain.anchor[1])
                    < radius + crate::DOMAIN_RADIUS
            {
                return Err(ValidationError::InvalidValue(format!(
                    "project bounds or domain collision: {}",
                    project.id
                )));
            }

            for other in &projects[index + 1..] {
                if (project.anchor[0] - other.anchor[0]).hypot(project.anchor[1] - other.anchor[1])
                    < radius + crate::project_radius(other) + 32.0
                {
                    return Err(ValidationError::InvalidValue(format!(
                        "project collision: {} and {}",
                        project.id, other.id
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Cross-domain overrides are checked globally rather than only within each imported region.
fn validate_cross_domain_collisions(config: &Config) -> Result<(), ValidationError> {
    let nodes = config
        .domains
        .iter()
        .map(|domain| {
            (
                domain.id.as_str(),
                domain.id.as_str(),
                domain.anchor,
                crate::DOMAIN_RADIUS,
            )
        })
        .chain(config.projects.iter().map(|project| {
            (
                project.id.as_str(),
                project.domain.as_str(),
                project.anchor,
                crate::project_radius(project),
            )
        }))
        .collect::<Vec<_>>();

    for (index, (id, domain, center, radius)) in nodes.iter().enumerate() {
        for (other_id, other_domain, other, other_radius) in &nodes[index + 1..] {
            if domain != other_domain
                && (center[0] - other[0]).hypot(center[1] - other[1]) < radius + other_radius + 24.0
            {
                return Err(ValidationError::InvalidValue(format!(
                    "cross-domain collision: {id} and {other_id}"
                )));
            }
        }
    }

    Ok(())
}

/// Resolve explicit shared identities before graph references are built, avoiding duplicate nodes.
fn technology_reference(config: &Config, scope: &str, local_id: &str) -> String {
    let canonical_id = scoped(scope, local_id);

    config
        .shared_technologies
        .get(&canonical_id)
        .cloned()
        .unwrap_or(canonical_id)
}

/// A manifest can reference only its own local definitions or the reserved built-in catalog.
fn validate_manifest_icons(manifest: &OrganizationManifest) -> Result<(), ValidationError> {
    for key in manifest.icons.keys() {
        crate::validate_local_icon_id(key)?;
    }

    crate::validate_icon_catalog(&manifest.icons)?;

    for project in &manifest.projects {
        if let Some(icon) = &project.icon {
            if !icon.starts_with("builtin:") {
                crate::validate_local_icon_id(icon)?;
            }

            crate::validate_icon_reference(&manifest.icons, icon)?;
        }
    }

    Ok(())
}
