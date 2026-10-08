use std::collections::BTreeSet;

use thiserror::Error;

use crate::{Config, NodeKind, ProfileState, Visibility};

/// A configuration or generated state violates the public graph contract.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// Only the currently supported configuration schema can be interpreted.
    #[error("configuration version must be 1")]
    UnsupportedVersion,
    /// Identifiers must be unique within their graph namespace.
    #[error("duplicate identifier: {0}")]
    DuplicateId(String),
    /// A graph object references a domain that is not configured.
    #[error("unknown domain '{domain}' referenced by {owner}")]
    UnknownDomain {
        /// Missing domain identifier.
        domain: String,
        /// Object containing the invalid reference.
        owner: String,
    },
    /// A graph object references a technology that is not configured.
    #[error("unknown technology '{technology}' referenced by {owner}")]
    UnknownTechnology {
        /// Missing technology identifier.
        technology: String,
        /// Object containing the invalid reference.
        owner: String,
    },
    /// An edge references an absent endpoint.
    #[error("edge references missing node: {0}")]
    MissingNode(String),
    /// The design canvas is below the supported minimum.
    #[error("canvas must be at least 1200 x 640")]
    CanvasTooSmall,
    /// A field contains an unsafe, empty, or otherwise unsupported value.
    #[error("invalid value: {0}")]
    InvalidValue(String),
    /// Curated private project metadata cannot contain a repository URL.
    #[error("private project must not contain a repository URL: {0}")]
    PrivateUrl(String),
}

/// Check graph references, finite geometry, safe links, and the curated private-content boundary.
///
/// Project identities and endpoint counts are content, not schema invariants. This permits
/// legitimate additions and renames without changing validation code.
pub fn validate_config(config: &Config) -> Result<(), ValidationError> {
    validate_configuration(config, ValidationPhase::Resolved)
}

/// Admit selected imported technology affinities and icon namespaces before composition.
pub(crate) fn validate_authored_config(config: &Config) -> Result<(), ValidationError> {
    validate_configuration(config, ValidationPhase::Authored)
}

/// Authored inputs may name selected imports that are unavailable until composition.
#[derive(Clone, Copy)]
enum ValidationPhase {
    Authored,
    Resolved,
}

/// Share admission checks while deferring only references to explicitly selected canonical input.
fn validate_configuration(
    config: &Config,
    references: ValidationPhase,
) -> Result<(), ValidationError> {
    crate::validate_icon_catalog(&config.icons)?;

    for icon in config
        .projects
        .iter()
        .filter_map(|project| project.icon.as_deref())
    {
        if matches!(references, ValidationPhase::Authored)
            && crate::resolve_icon(&config.icons, icon).is_none()
            && let Some((namespace, _)) = icon.split_once('/')
            && config.imports.iter().any(|import| import.id == namespace)
        {
            crate::icons::validate_icon_id(icon)?;
        } else {
            crate::validate_icon_reference(&config.icons, icon)?;
        }
    }

    crate::validate_icon_usage(
        &config.icons,
        config
            .projects
            .iter()
            .filter_map(|project| project.icon.as_deref())
            .filter(|icon| crate::resolve_icon(&config.icons, icon).is_some()),
    )?;
    crate::xml_text::validate(config)?;

    let nodes = config
        .domains
        .len()
        .saturating_add(config.technologies.len())
        .saturating_add(
            config
                .projects
                .iter()
                .map(|project| 1 + project.components.len())
                .sum::<usize>(),
        )
        .saturating_add(
            config
                .publications
                .iter()
                .map(|group| 1 + group.packages.len())
                .sum::<usize>(),
        )
        .saturating_add(config.interests.len());

    let relationships = config
        .projects
        .iter()
        .map(|project| {
            project.implemented_with.len()
                + project.integrates.len()
                + project.targets.len()
                + project
                    .components
                    .iter()
                    .map(|component| 1 + component.integrates.len() + component.targets.len())
                    .sum::<usize>()
        })
        .sum::<usize>()
        + config
            .technologies
            .iter()
            .map(|technology| technology.affinities.len())
            .sum::<usize>();

    if nodes > crate::MAX_GRAPH_NODES || relationships > crate::MAX_GRAPH_EDGES {
        return Err(ValidationError::InvalidValue(
            "configuration exceeds runtime graph capacity".into(),
        ));
    }

    if config.version != 1 {
        return Err(ValidationError::UnsupportedVersion);
    }

    if config.profile.variant == crate::ProfileVariant::Organization
        && (config
            .domains
            .iter()
            .any(|domain| domain.kind != "organization")
            || config
                .domains
                .iter()
                .filter(|domain| domain.kind == "organization")
                .count()
                > 1)
    {
        return Err(ValidationError::InvalidValue(
            "organization profile contains unrelated ownership domains".into(),
        ));
    }

    if config.profile.variant == crate::ProfileVariant::Organization {
        let owner = &config.profile.organization;

        if !config.profile.username.eq_ignore_ascii_case(owner)
            || config
                .collection
                .github_organizations
                .iter()
                .any(|selected| !selected.eq_ignore_ascii_case(owner))
            || config
                .domains
                .iter()
                .any(|domain| !domain.owner.eq_ignore_ascii_case(owner))
        {
            return Err(ValidationError::InvalidValue(
                "organization identity and selected owners differ".into(),
            ));
        }

        if !config.domains.is_empty() && config.collection.github_organizations.len() != 1 {
            return Err(ValidationError::InvalidValue(
                "organization profile requires exactly its selected owner".into(),
            ));
        }
    }

    if config.render.width < 1200
        || config.render.height < 640
        || config.render.width > 100000
        || config.render.height > 100000
    {
        return Err(ValidationError::CanvasTooSmall);
    }

    if config.render.motion_seconds == 0
        || !matches!(config.render.detail_level.as_str(), "abstract" | "detailed")
        || config.collection.repository_limit == 0
        || config.collection.repository_limit > 1000
        || config.collection.history_limit == 0
        || config.collection.history_limit > crate::MAX_HISTORY_ENTRIES
    {
        return Err(ValidationError::InvalidValue(
            "render or collection settings".into(),
        ));
    }

    if let Some(maintainer) = &config.profile.maintainer {
        validate_maintainer(maintainer)?;
    }

    let mut import_ids = BTreeSet::new();

    for import in &config.imports {
        insert(&mut import_ids, &import.id)?;
    }

    safe_url(&config.profile.pages_url)?;
    safe_url(&config.profile.source_url)?;
    validate_github_handle(&config.profile.username)?;
    validate_github_handle(&config.profile.organization)?;
    if config.profile.variant == crate::ProfileVariant::Personal
        || !config.collection.github_user.is_empty()
    {
        validate_github_handle(&config.collection.github_user)?;
    }

    if let Some(owner) = &config.collection.nuget_owner {
        validate_github_handle(owner)?;
    }

    for organization in &config.collection.github_organizations {
        validate_github_handle(organization)?;
    }

    let mut ids = BTreeSet::new();
    let domains = config
        .domains
        .iter()
        .map(|domain| domain.id.as_str())
        .collect::<BTreeSet<_>>();

    let technologies = config
        .technologies
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();

    for (canonical_id, target) in &config.shared_technologies {
        if canonical_id
            .split_once('/')
            .is_none_or(|(scope, id)| scope.is_empty() || id.is_empty() || id.contains('/'))
        {
            return Err(ValidationError::InvalidValue(format!(
                "invalid shared technology binding: {canonical_id}"
            )));
        }

        technology_reference(&technologies, target, canonical_id)?;
    }

    for domain in &config.domains {
        insert(&mut ids, &format!("domain:{}", domain.id))?;
        validate_github_handle(&domain.owner)?;

        if let Some(caption) = &domain.repository_caption {
            crate::repository_captions::validate_config(caption)?;
        }

        if let Some(maintainer) = &domain.maintainer {
            validate_maintainer(maintainer)?;
        }

        anchor(domain.anchor, config)?;
    }

    for technology in &config.technologies {
        insert(&mut ids, &format!("technology:{}", technology.id))?;
        for affinity in &technology.affinities {
            // Consumer capabilities may cross into selected canonical domains before the manifests
            // are loaded. Project and publication ownership still requires an inline domain.
            if matches!(references, ValidationPhase::Authored) && import_ids.contains(affinity) {
                continue;
            }

            domain_reference(&domains, affinity, &technology.id)?;
        }
    }

    for project in &config.projects {
        insert(&mut ids, &format!("project:{}", project.id))?;
        domain_reference(&domains, &project.domain, &project.id)?;
        anchor(project.anchor, config)?;
        if !project.weight.is_finite()
            || !(0.0..=1.0).contains(&project.weight)
            || project
                .radius
                .is_some_and(|radius| !radius.is_finite() || !(1.0..=200.0).contains(&radius))
        {
            return Err(ValidationError::InvalidValue(
                "project weight or radius".into(),
            ));
        }

        if let Some(repository) = &project.repository {
            if project.visibility == Visibility::PrivateAbstract {
                return Err(ValidationError::PrivateUrl(project.id.clone()));
            }

            repository_name(repository)?;

            if config.profile.variant == crate::ProfileVariant::Organization
                && !repository.split_once('/').is_some_and(|(owner, _)| {
                    owner.eq_ignore_ascii_case(&config.profile.organization)
                })
            {
                return Err(ValidationError::InvalidValue(
                    "organization project repository has unrelated owner".into(),
                ));
            }
        }

        for (references, relation) in [
            (&project.implemented_with, "implemented_with"),
            (&project.integrates, "integrates"),
            (&project.targets, "targets"),
        ] {
            technology_references(&technologies, references, &project.id, relation)?;
        }

        for component in &project.components {
            insert(
                &mut ids,
                &format!("component:{}:{}", project.id, component.id),
            )?;
            for (references, relation) in [
                (&component.integrates, "integrates"),
                (&component.targets, "targets"),
            ] {
                technology_references(&technologies, references, &component.id, relation)?;
            }
        }
    }

    let mut packages = BTreeSet::new();
    let mut discovery_roots = Vec::<&str>::new();
    for publication in &config.publications {
        if publication.registry != "nuget" {
            return Err(ValidationError::InvalidValue(
                "unsupported package registry".into(),
            ));
        }

        if let Some(owner) = &publication.owner {
            validate_github_handle(owner)?;
        }

        insert(&mut ids, &format!("publication:{}", publication.id))?;
        domain_reference(&domains, &publication.domain, &publication.id)?;
        anchor(publication.anchor, config)?;
        for technology in &publication.technologies {
            technology_reference(&technologies, technology, &publication.id)?;
        }

        for prefix in &publication.discovery_prefixes {
            if !crate::valid_package_id(prefix)
                || discovery_roots.iter().any(|known| {
                    crate::registry::package_id_has_root(known, prefix)
                        || crate::registry::package_id_has_root(prefix, known)
                })
            {
                return Err(ValidationError::InvalidValue(
                    "ambiguous or invalid package discovery root".into(),
                ));
            }

            discovery_roots.push(prefix);
        }

        for id in publication.label_overrides.keys() {
            if !crate::valid_package_id(id) {
                return Err(ValidationError::InvalidValue(
                    "NuGet package label override ID".into(),
                ));
            }
        }

        for package in &publication.packages {
            insert(&mut packages, &package.id.to_ascii_lowercase())?;
            package_link(&package.id, &package.url)?;
            if let Some(value) = package.anchor {
                anchor(value, config)?;
            }
        }
    }

    let interests = config
        .interests
        .iter()
        .map(|interest| interest.id.as_str())
        .collect::<BTreeSet<_>>();

    for interest in &config.interests {
        insert(&mut ids, &format!("interest:{}", interest.id))?;
    }

    for learning in &config.learning {
        insert(&mut ids, &format!("learning:{}", learning.id))?;
        safe_url(&learning.url)?;
        if !interests.contains(learning.interest.as_str()) {
            return Err(ValidationError::InvalidValue(
                "learning interest reference".into(),
            ));
        }
    }

    crate::surface_labels::validate_surface_labels(config)?;

    Ok(())
}

/// Validate state before it reaches renderers or browser consumers.
///
/// HTTPS alone is deliberately required for links; arbitrary schemes could execute code when
/// a state node is rendered as an anchor. Text still requires output-context escaping.
pub fn validate_state(state: &ProfileState) -> Result<(), ValidationError> {
    crate::validate_icon_catalog(&state.icons)?;

    for node in &state.nodes {
        if let Some(caption) = &node.repository_caption {
            if node.kind != NodeKind::Domain {
                return Err(ValidationError::InvalidValue(
                    "repository captions are supported only on domain nodes".into(),
                ));
            }

            crate::repository_captions::validate_materialized(caption)?;
        }

        if node.icon.is_some() && node.kind != NodeKind::Project {
            return Err(ValidationError::InvalidValue(
                "icon references are supported only on project nodes".into(),
            ));
        }
    }

    crate::validate_icon_usage(
        &state.icons,
        state.nodes.iter().filter_map(|node| node.icon.as_deref()),
    )?;
    crate::xml_text::validate(state)?;

    if state.nodes.len() > crate::MAX_GRAPH_NODES || state.edges.len() > crate::MAX_GRAPH_EDGES {
        return Err(ValidationError::InvalidValue(
            "state exceeds 512 nodes or 8192 edges".into(),
        ));
    }

    if state.schema != crate::STATE_SCHEMA_VERSION {
        return Err(ValidationError::UnsupportedVersion);
    }

    if let Some(maintainer) = &state.profile.maintainer {
        validate_maintainer(maintainer)?;
    }

    for organization in &state.organizations {
        if let Some(maintainer) = &organization.maintainer {
            validate_maintainer(maintainer)?;
        }
    }

    if state.canvas.width < 1200
        || state.canvas.height < 640
        || state.canvas.width > 100000
        || state.canvas.height > 100000
        || state.canvas.motion_seconds == 0
    {
        return Err(ValidationError::CanvasTooSmall);
    }

    safe_url(&state.profile.pages_url)?;
    safe_url(&state.profile.source_url)?;
    let mut node_ids = BTreeSet::new();
    for node in &state.nodes {
        insert(&mut node_ids, &node.id)?;
        if !node.x.is_finite()
            || !node.y.is_finite()
            || !node.radius.is_finite()
            || !node.weight.is_finite()
            || node.radius <= 0.0
            || node.x < 0.0
            || node.y < 0.0
            || node.x > state.canvas.width as f32
            || node.y > state.canvas.height as f32
        {
            return Err(ValidationError::InvalidValue("node geometry".into()));
        }

        if let Some(url) = &node.url {
            if node.visibility == Some(Visibility::PrivateAbstract)
                && matches!(node.kind, NodeKind::Project | NodeKind::Component)
            {
                return Err(ValidationError::PrivateUrl(node.id.clone()));
            }

            safe_url(url)?;
        }
    }

    for edge in &state.edges {
        if !node_ids.contains(&edge.from) {
            return Err(ValidationError::MissingNode(edge.from.clone()));
        }

        if !node_ids.contains(&edge.to) {
            return Err(ValidationError::MissingNode(edge.to.clone()));
        }

        if !edge.weight.is_finite() || !(0.0..=1.0).contains(&edge.weight) {
            return Err(ValidationError::InvalidValue("edge weight".into()));
        }
    }

    for package in &state.packages {
        if let Some(url) = &package.url {
            package_link(&package.id, url)?;
        }
    }

    for learning in &state.learning {
        safe_url(&learning.url)?;
    }

    Ok(())
}

/// Validate supplied attribution consistently at configuration, manifest and state boundaries.
pub(crate) fn validate_maintainer(
    maintainer: &crate::MaintainerConfig,
) -> Result<(), ValidationError> {
    validate_github_handle(&maintainer.username)?;

    if maintainer.role.trim().is_empty() {
        return Err(ValidationError::InvalidValue("maintainer role".into()));
    }

    safe_url(&maintainer.url)
}

/// Restrict authored outbound links to unambiguous absolute HTTPS URLs without credentials.
fn safe_url(value: &str) -> Result<(), ValidationError> {
    let valid = value.strip_prefix("https://").is_some_and(|rest| {
        let host = rest.split('/').next().unwrap_or_default();
        !host.is_empty()
            && host.contains('.')
            && host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte))
            && !value.chars().any(|character| {
                character.is_control()
                    || character.is_whitespace()
                    || matches!(character, '\\' | '<' | '>' | '"' | '\'')
            })
    });

    if !valid {
        return Err(ValidationError::InvalidValue("HTTPS link".into()));
    }

    Ok(())
}

/// Check that a public GitHub handle can safely form a URL path segment.
///
/// The validator name distinguishes its status result from functions that return account data.
fn validate_github_handle(value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(ValidationError::InvalidValue("GitHub account".into()));
    }

    Ok(())
}

/// Validate the owner/repository shorthand before constructing its public URL.
fn repository_name(value: &str) -> Result<(), ValidationError> {
    let Some((owner, name)) = value.split_once('/') else {
        return Err(ValidationError::InvalidValue("repository name".into()));
    };

    validate_github_handle(owner)?;
    if name.is_empty()
        || matches!(name, "." | "..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(ValidationError::InvalidValue("repository name".into()));
    }

    Ok(())
}

/// Reject invalid authored coordinates rather than silently altering the intended composition.
fn anchor(value: [f32; 2], config: &Config) -> Result<(), ValidationError> {
    if !value[0].is_finite()
        || !value[1].is_finite()
        || value[0] < 0.0
        || value[1] < 0.0
        || value[0] > config.render.width as f32
        || value[1] > config.render.height as f32
    {
        return Err(ValidationError::InvalidValue("anchor".into()));
    }

    Ok(())
}

fn domain_reference(
    domains: &BTreeSet<&str>,
    domain: &str,
    owner: &str,
) -> Result<(), ValidationError> {
    if !domains.contains(domain) {
        return Err(ValidationError::UnknownDomain {
            domain: domain.into(),
            owner: owner.into(),
        });
    }

    Ok(())
}

/// Shared endpoints across relation kinds are valid; repeats within one kind are not.
fn technology_references(
    technologies: &BTreeSet<&str>,
    references: &[String],
    owner: &str,
    relation: &str,
) -> Result<(), ValidationError> {
    let mut seen = BTreeSet::new();

    for technology in references {
        technology_reference(technologies, technology, owner)?;

        if !seen.insert(technology.as_str()) {
            return Err(ValidationError::InvalidValue(format!(
                "duplicate {relation} technology '{technology}' referenced by {owner}"
            )));
        }
    }

    Ok(())
}

fn technology_reference(
    technologies: &BTreeSet<&str>,
    technology: &str,
    owner: &str,
) -> Result<(), ValidationError> {
    if !technologies.contains(technology) {
        return Err(ValidationError::UnknownTechnology {
            technology: technology.into(),
            owner: owner.into(),
        });
    }

    Ok(())
}

fn insert(ids: &mut BTreeSet<String>, id: &str) -> Result<(), ValidationError> {
    if id.is_empty() || id.ends_with(':') || id.chars().any(char::is_control) {
        return Err(ValidationError::InvalidValue("identifier".into()));
    }

    if !ids.insert(id.to_string()) {
        return Err(ValidationError::DuplicateId(id.to_string()));
    }

    Ok(())
}

/// Bind authored NuGet links to the unversioned canonical page of the declared package.
/// Registry API, query, fragment, and alternate-host URLs are not publication destinations.
fn package_link(id: &str, url: &str) -> Result<(), ValidationError> {
    if !crate::valid_package_id(id) {
        return Err(ValidationError::InvalidValue(
            "NuGet package identifier".into(),
        ));
    }

    let prefix = "https://www.nuget.org/packages/";
    let canonical = url.strip_suffix('/').unwrap_or(url);
    if !canonical
        .get(..prefix.len())
        .is_some_and(|host| host.eq_ignore_ascii_case(prefix))
        || !canonical
            .get(prefix.len()..)
            .is_some_and(|package| package.eq_ignore_ascii_case(id))
    {
        return Err(ValidationError::InvalidValue(
            "canonical NuGet package URL".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ValidationError, validate_github_handle};

    #[test]
    fn github_handle_validation_accepts_existing_ascii_forms() {
        // Arrange
        let handles = ["sample-user", "sample-labs", "ExampleLabs", "123", "a-b"];

        // Act
        let results = handles.map(validate_github_handle);

        // Assert
        assert!(results.iter().all(Result::is_ok));
    }

    #[test]
    fn github_handle_validation_preserves_rejection_diagnostics() {
        // Arrange
        let handles = [
            "",
            "sample/user",
            "sample_user",
            "sample.user",
            "sample user",
            "sample\\user",
            "sample\nuser",
            "sample\u{e9}",
        ];

        // Act
        let results = handles.map(validate_github_handle);

        // Assert
        assert!(results.into_iter().all(|result| matches!(
            result,
            Err(ValidationError::InvalidValue(message)) if message == "GitHub account"
        )));
    }
}
