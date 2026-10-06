use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    Canvas, Config, Edge, EdgeKind, Node, NodeKind, ProfileState, Snapshot, SnapshotMode,
    StateProfile, StateStats,
};

/// Failure to construct the deterministic public state.
#[derive(Debug, Error)]
pub enum GraphError {
    /// Configuration or generated graph exceeds the supported contract.
    #[error(transparent)]
    Validation(#[from] crate::ValidationError),
    /// Semantic input could not be serialized.
    #[error("failed to serialize semantic state: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Build the canonical graph from approved configuration and observed metadata.
///
/// The timestamp is presentation metadata and does not change the semantic hash.
/// Call [`crate::validate_config`] before building and [`crate::validate_state`] before publishing.
/// The caller must clear private aggregates unless effective collection authorization permits them;
/// the builder consumes that authorized snapshot without reinterpreting CLI or environment policy.
pub fn build_state(
    config: &Config,
    snapshot: &Snapshot,
    generated_at: impl Into<String>,
) -> Result<ProfileState, GraphError> {
    let prepared = prepare_profile(config, snapshot)?;

    build_prepared_state(&prepared, generated_at)
}

/// Validated, normalized content reused across graph, README and persisted-input outputs.
#[derive(Debug)]
pub struct PreparedProfile {
    config: Config,
    snapshot: Snapshot,
}

impl PreparedProfile {
    /// Borrow the expanded canonical inventory without another full configuration clone.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Borrow the publication-scoped observation snapshot.
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
}

/// Scope, expand, normalize and validate inputs exactly once for a generation transaction.
pub fn prepare_profile(
    config: &Config,
    snapshot: &Snapshot,
) -> Result<PreparedProfile, GraphError> {
    if snapshot.schema_version != 1 {
        return Err(crate::ValidationError::UnsupportedVersion.into());
    }

    crate::validate_config(config)?;
    let snapshot = normalized_snapshot(crate::scope_snapshot(config, snapshot));
    let mut config = normalized_config(crate::expanded_config(config, &snapshot));
    let footer = crate::personal_footer_layout(
        &config.presentation,
        &config.interests,
        &config.learning,
        config.render.show_interests_in_readme,
    );
    if config.profile.variant == crate::ProfileVariant::Personal {
        config.render.height = config.render.height.saturating_add(
            (footer.extra_height * config.render.width as f32 / 1800.0).ceil() as u32,
        );
    }

    crate::validate_config(&config)?;

    Ok(PreparedProfile { config, snapshot })
}

/// Build a graph from the same prepared input borrowed by all other output projections.
pub fn build_prepared_state(
    prepared: &PreparedProfile,
    generated_at: impl Into<String>,
) -> Result<ProfileState, GraphError> {
    let config = prepared.config();
    let snapshot = prepared.snapshot();
    let generated_at = generated_at.into();
    let semantic_hash = semantic_hash(config, snapshot)?;
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for domain in &config.domains {
        nodes.push(Node {
            icon: None,
            display_stack: Vec::new(),
            label_prefix: None,
            id: format!("domain:{}", domain.id),
            label: domain.label.clone(),
            surface_label: domain.label.clone(),
            kind: NodeKind::Domain,
            domain: Some(domain.id.clone()),
            x: domain.anchor[0],
            y: domain.anchor[1],
            radius: crate::DOMAIN_RADIUS,
            weight: 1.0,
            summary: domain_summary(config, &domain.id),
            details: Vec::new(),
            tags: vec![domain.kind.clone(), domain.owner.clone()],
            url: account_url(&domain.owner),
            visibility: None,
            show_in_readme: true,
            visual: Some("field".to_string()),
            scope: Some(
                if domain.kind == "organization" {
                    "organization"
                } else {
                    "personal"
                }
                .into(),
            ),
            family: None,
        });
    }

    for project in &config.projects {
        let project_id = format!("project:{}", project.id);
        nodes.push(Node {
            icon: project.icon.clone(),
            display_stack: project.display_stack.clone(),
            label_prefix: project.label_prefix.clone(),
            id: project_id.clone(),
            label: project.label.clone(),
            surface_label: project.surface_label.clone(),
            kind: NodeKind::Project,
            domain: Some(project.domain.clone()),
            x: project.anchor[0],
            y: project.anchor[1],
            radius: crate::project_radius(project),
            weight: project.weight,
            summary: project.summary.clone(),
            details: project
                .components
                .iter()
                .map(|component| format!("{}: {}", component.label, component.summary))
                .collect(),
            tags: project.tags.clone(),
            url: project.repository.as_deref().map(repository_url),
            visibility: Some(project.visibility),
            show_in_readme: project.show_in_readme,
            visual: Some(project.visual.clone()),
            scope: Some(project.status.clone()),
            family: None,
        });
        edges.push(Edge {
            from: format!("domain:{}", project.domain),
            to: project_id.clone(),
            kind: EdgeKind::Contains,
            weight: project.weight,
            show_in_readme: project.show_in_readme,
        });

        let count = project.components.len().max(1);
        for (index, component) in project.components.iter().enumerate() {
            let angle = ring_angle(index, count, -0.9);
            let orbit = project.weight * 20.0 + 78.0;
            let component_id = format!("component:{}:{}", project.id, component.id);
            nodes.push(Node {
                icon: None,
                display_stack: Vec::new(),
                label_prefix: None,
                id: component_id.clone(),
                label: component.label.clone(),
                surface_label: component.label.clone(),
                kind: NodeKind::Component,
                domain: Some(project.domain.clone()),
                x: project.anchor[0] + angle.cos() * orbit,
                y: project.anchor[1] + angle.sin() * orbit,
                radius: 11.0 + project.weight * 3.0,
                weight: 0.35,
                summary: component.summary.clone(),
                details: component.details.clone(),
                tags: component
                    .targets
                    .iter()
                    .chain(component.integrates.iter())
                    .cloned()
                    .collect(),
                url: None,
                visibility: Some(project.visibility),
                show_in_readme: false,
                visual: Some(component.kind.clone()),
                scope: Some(component.scope.clone()),
                family: None,
            });
            edges.push(Edge {
                from: project_id.clone(),
                to: component_id.clone(),
                kind: EdgeKind::Component,
                weight: 0.55,
                show_in_readme: project.show_in_readme,
            });
            for (technologies, kind) in [
                (&component.integrates, EdgeKind::Integrates),
                (&component.targets, EdgeKind::Targets),
            ] {
                for technology in technologies {
                    edges.push(Edge {
                        from: format!("technology:{technology}"),
                        to: component_id.clone(),
                        kind,
                        weight: 0.25,
                        show_in_readme: false,
                    });
                }
            }
        }

        for technology in &project.implemented_with {
            edges.push(Edge {
                from: format!("technology:{technology}"),
                to: project_id.clone(),
                kind: EdgeKind::ImplementedWith,
                weight: 0.75,
                show_in_readme: project.show_in_readme,
            });
        }

        for technology in &project.integrates {
            edges.push(Edge {
                from: format!("technology:{technology}"),
                to: project_id.clone(),
                kind: EdgeKind::Integrates,
                weight: 0.35,
                show_in_readme: false,
            });
        }

        for technology in &project.targets {
            edges.push(Edge {
                from: format!("technology:{technology}"),
                to: project_id.clone(),
                kind: EdgeKind::Targets,
                weight: 0.35,
                show_in_readme: false,
            });
        }
    }

    let package_map = snapshot
        .packages
        .iter()
        .map(|package| (package.id.to_ascii_lowercase(), package))
        .collect::<BTreeMap<_, _>>();

    for publication in &config.publications {
        let publication_id = format!("publication:{}", publication.id);
        nodes.push(Node {
            icon: None,
            display_stack: Vec::new(),
            label_prefix: None,
            id: publication_id.clone(),
            label: publication.label.clone(),
            surface_label: publication.surface_label.clone(),
            kind: NodeKind::Publication,
            domain: Some(publication.domain.clone()),
            x: publication.anchor[0],
            y: publication.anchor[1],
            radius: 76.0,
            weight: 1.0,
            summary: publication.summary.clone(),
            details: publication
                .packages
                .iter()
                .map(|package| package.id.clone())
                .collect(),
            tags: vec![publication.registry.clone()],
            url: config
                .projects
                .iter()
                .find(|project| project.id == publication.id)
                .and_then(|project| project.repository.as_ref())
                .map(|repository| format!("https://github.com/{repository}")),
            visibility: Some(crate::Visibility::Public),
            show_in_readme: publication.show_in_readme,
            visual: Some("package-field".to_string()),
            scope: Some(publication.registry.clone()),
            family: None,
        });
        edges.push(Edge {
            from: format!("domain:{}", publication.domain),
            to: publication_id.clone(),
            kind: EdgeKind::Contains,
            weight: 1.0,
            show_in_readme: publication.show_in_readme,
        });

        for (index, package) in publication.packages.iter().enumerate() {
            let live = package_map.get(&package.id.to_ascii_lowercase()).copied();
            let package_id = format!("package:{}", package.id);
            let mut details = vec![format!("Family: {}", package.family)];
            if let Some(version) = live.and_then(|value| value.version.as_ref()) {
                details.push(format!("Observed version: {version}"));
            }

            if let Some(downloads) = live.and_then(|value| value.total_downloads) {
                details.push(format!("Downloads: {downloads}"));
            }

            nodes.push(Node {
                icon: None,
                display_stack: Vec::new(),
                label_prefix: None,
                id: package_id.clone(),
                label: package.id.clone(),
                surface_label: crate::package_label(publication, &package.id),
                kind: NodeKind::Package,
                domain: Some(publication.domain.clone()),
                x: crate::package_anchor(publication.anchor, package, index)[0],
                y: crate::package_anchor(publication.anchor, package, index)[1],
                radius: 15.0,
                weight: 0.45,
                summary: package.summary.clone(),
                details,
                tags: vec![publication.registry.clone(), package.family.clone()],
                url: Some(package.url.clone()),
                visibility: Some(crate::Visibility::Public),
                show_in_readme: publication.show_in_readme,
                visual: Some("package".to_string()),
                scope: Some(publication.registry.clone()),
                family: Some(package.family.clone()),
            });
            edges.push(Edge {
                from: publication_id.clone(),
                to: package_id,
                kind: EdgeKind::Publishes,
                weight: 0.5,
                show_in_readme: publication.show_in_readme,
            });
        }

        for technology in &publication.technologies {
            edges.push(Edge {
                from: format!("technology:{technology}"),
                to: publication_id.clone(),
                kind: EdgeKind::ImplementedWith,
                weight: 0.7,
                show_in_readme: publication.show_in_readme,
            });
        }
    }

    for technology in &config.technologies {
        let (x, y) = technology_position(technology, config);

        let technology_id = format!("technology:{}", technology.id);
        nodes.push(Node {
            icon: None,
            display_stack: Vec::new(),
            label_prefix: None,
            id: technology_id.clone(),
            label: technology.label.clone(),
            surface_label: technology.label.clone(),
            kind: NodeKind::Technology,
            domain: affinity_domain(&technology.affinities),
            x,
            y,
            radius: if technology.show_in_readme { 22.0 } else { 9.0 },
            weight: if technology.show_in_readme {
                0.55
            } else {
                0.18
            },
            summary: format!(
                "{} capability in the {} layer.",
                technology.label, technology.category
            ),
            details: technology
                .affinities
                .iter()
                .map(|affinity| format!("Affinity: {affinity}"))
                .collect(),
            tags: vec![technology.category.clone()],
            url: None,
            visibility: None,
            show_in_readme: technology.show_in_readme,
            visual: Some(technology.category.clone()),
            scope: None,
            family: None,
        });

        for affinity in &technology.affinities {
            edges.push(Edge {
                from: format!("domain:{affinity}"),
                to: technology_id.clone(),
                kind: if technology.cross_domain {
                    EdgeKind::SharedCapability
                } else {
                    EdgeKind::Affinity
                },
                weight: if technology.show_in_readme {
                    0.35
                } else {
                    0.12
                },
                show_in_readme: technology.show_in_readme,
            });
        }
    }

    for (index, interest) in config.interests.iter().enumerate() {
        nodes.push(Node {
            icon: None,
            display_stack: Vec::new(),
            label_prefix: None,
            id: format!("interest:{}", interest.id),
            label: interest.label.clone(),
            surface_label: interest.label.clone(),
            kind: NodeKind::Interest,
            domain: None,
            x: 650.0 + index as f32 * 150.0,
            y: config.render.height as f32 - 80.0,
            radius: 10.0,
            weight: 0.18,
            summary: interest.summary.clone(),
            details: Vec::new(),
            tags: vec!["interest".to_string()],
            url: None,
            visibility: None,
            show_in_readme: config.render.show_interests_in_readme && interest.show_in_readme,
            visual: Some("horizon".to_string()),
            scope: None,
            family: None,
        });
    }

    if config.collection.visualize_discovered_repositories {
        add_discovered_repositories(config, snapshot, &mut nodes, &mut edges);
    }

    let organization_public_repositories = config
        .collection
        .github_organizations
        .iter()
        .map(|login| {
            snapshot
                .organizations
                .iter()
                .find(|account| account.login.eq_ignore_ascii_case(login))
                .filter(|_| source_available(snapshot, &format!("github:org:{login}")))
                .map(|account| account.public_repositories)
        })
        .try_fold(0u32, |total, count| {
            count.map(|count| total.saturating_add(count))
        });

    // A partial subtotal must never look like the download total for all published packages.
    let package_downloads = config
        .publications
        .iter()
        .flat_map(|publication| &publication.packages)
        .map(|package| {
            package_map
                .get(&package.id.to_ascii_lowercase())
                .and_then(|item| item.total_downloads)
        })
        .try_fold(0u64, |total, count| {
            count.map(|count| total.saturating_add(count))
        })
        .filter(|_| {
            config
                .publications
                .iter()
                .any(|group| !group.packages.is_empty())
        });

    for node in &mut nodes {
        // Discovery glyph sizes reflect stars, but their edge padding must not: otherwise
        // changing a count moves edge-adjacent centers despite their stable identity seed.
        let padding = if node.visual.as_deref() == Some("repository") {
            12.0
        } else {
            node.radius
        };

        node.x = node.x.clamp(padding, config.render.width as f32 - padding);
        node.y = node.y.clamp(padding, config.render.height as f32 - padding);
    }

    let state = ProfileState {
        icons: config.icons.clone(),
        organizations: config
            .domains
            .iter()
            .filter(|domain| domain.kind == "organization")
            .map(|domain| crate::OrganizationIdentity {
                id: domain.id.clone(),
                owner: domain.owner.clone(),
                label: domain.label.clone(),
                maintainer: domain.maintainer.clone(),
            })
            .collect(),
        schema: crate::STATE_SCHEMA_VERSION,
        presentation: config.presentation.clone(),
        interests: config.interests.clone(),
        learning: config.learning.clone(),
        sources: snapshot.sources.clone(),
        mode: snapshot.mode,
        generated_at,
        semantic_hash,
        canvas: Canvas {
            show_activity_orbit: config.render.show_activity_orbit,
            show_state_hash: config.render.show_state_hash,
            show_interests_in_readme: config.render.show_interests_in_readme,
            show_technology_labels: config.render.show_technology_labels,
            show_details: config.render.detail_level == "detailed",
            width: config.render.width,
            height: config.render.height,
            motion_seconds: config.render.motion_seconds,
        },
        profile: StateProfile {
            variant: config.profile.variant,
            maintainer: config.profile.maintainer.clone(),
            username: config.profile.username.clone(),
            display_name: config.profile.display_name.clone(),
            organization: config.profile.organization.clone(),
            headline: config.profile.headline.clone(),
            tagline: config.profile.tagline.clone(),
            pages_url: config.profile.pages_url.clone(),
            source_url: config.profile.source_url.clone(),
        },
        stats: StateStats {
            personal_public_repositories: (!snapshot.user.login.is_empty()
                && source_available(snapshot, "github:user"))
            .then_some(snapshot.user.public_repositories),
            organization_public_repositories,
            followers: (!snapshot.user.login.is_empty()
                && source_available(snapshot, "github:user"))
            .then_some(snapshot.user.followers),
            package_count: config
                .publications
                .iter()
                .map(|publication| publication.packages.len() as u32)
                .sum(),
            package_downloads,
            contribution_total: snapshot.contributions.as_ref().map(|value| value.total),
            // The pipeline resolves CLI, environment, and config opt-in before handing off data.
            // Rechecking only config here would silently discard an authorized CLI-only count.
            private_repository_count: snapshot.private_repository_count,
        },
        nodes,
        edges,
        activity: snapshot
            .contributions
            .as_ref()
            .map(|value| value.days.clone())
            .unwrap_or_default(),
        packages: snapshot.packages.clone(),
        warnings: snapshot.warnings.clone(),
    };

    crate::validate_state(&state)?;

    Ok(state)
}

/// Add only public, uncurated repository nodes under their configured owner domain.
fn add_discovered_repositories(
    config: &Config,
    snapshot: &Snapshot,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) {
    let configured = config
        .projects
        .iter()
        .filter_map(|project| project.repository.as_deref())
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();

    for repository in &snapshot.repositories {
        if configured.contains(&repository.full_name.to_ascii_lowercase()) {
            continue;
        }

        let Some(ownership) = config
            .domains
            .iter()
            .find(|domain| domain.owner.eq_ignore_ascii_case(&repository.owner))
        else {
            continue;
        };

        let domain = ownership.id.as_str();
        let seed = seeded_u64(
            "repository-position-v1",
            &repository.full_name.to_ascii_lowercase(),
        );
        let angle = (seed % 6283) as f32 / 1000.0;
        let radius = 205.0 + ((seed >> 13) % 95) as f32;
        let anchor = ownership.anchor;
        let node_id = format!("repository:{}", repository.full_name);
        nodes.push(Node {
            icon: None,
            display_stack: Vec::new(),
            label_prefix: None,
            id: node_id.clone(),
            label: repository.name.clone(),
            surface_label: repository.name.clone(),
            kind: NodeKind::Project,
            domain: Some(domain.to_string()),
            x: anchor[0] + angle.cos() * radius,
            y: anchor[1] + angle.sin() * radius * 0.68,
            radius: 8.0 + (repository.stars.min(16) as f32).sqrt(),
            weight: 0.15,
            summary: repository.description.clone().unwrap_or_default(),
            details: repository
                .primary_language
                .iter()
                .map(|language| format!("Primary language: {language}"))
                .collect(),
            tags: repository.topics.clone(),
            url: Some(repository.url.clone()),
            visibility: Some(crate::Visibility::Public),
            show_in_readme: false,
            visual: Some("repository".to_string()),
            scope: None,
            family: None,
        });
        edges.push(Edge {
            from: format!("domain:{domain}"),
            to: node_id,
            kind: EdgeKind::Contains,
            weight: 0.12,
            show_in_readme: false,
        });
    }
}

/// Hash semantic input after canonical ordering, excluding volatile observation metadata.
fn semantic_hash(config: &Config, snapshot: &Snapshot) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct SemanticSnapshot<'a> {
        schema_version: u32,
        sources: &'a [crate::SourceStatus],
        mode: SnapshotMode,
        fetched_at: &'static str,
        user: &'a crate::AccountSnapshot,
        organizations: &'a [crate::AccountSnapshot],
        repositories: &'a [crate::RepositorySnapshot],
        contributions: &'a Option<crate::ContributionSnapshot>,
        private_repository_count: Option<u32>,
        packages: &'a [crate::PackageSnapshot],
        warnings: &'static [String],
    }

    #[derive(Serialize)]
    struct Payload<'a> {
        config: &'a Config,
        snapshot: SemanticSnapshot<'a>,
    }

    struct HashWriter(Sha256);

    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);

            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let payload = Payload {
        config,
        snapshot: SemanticSnapshot {
            schema_version: snapshot.schema_version,
            sources: &snapshot.sources,
            mode: snapshot.mode,
            fetched_at: "",
            user: &snapshot.user,
            organizations: &snapshot.organizations,
            repositories: &snapshot.repositories,
            contributions: &snapshot.contributions,
            private_repository_count: snapshot.private_repository_count,
            packages: &snapshot.packages,
            warnings: &[],
        },
    };

    let mut writer = HashWriter(Sha256::new());

    // Stream canonical JSON directly into the digest: a second full snapshot clone and
    // serialized byte buffer otherwise scale memory with the same graph twice again.
    serde_json::to_writer(&mut writer, &payload)?;
    let digest = writer.0.finalize();

    Ok(hex::encode_upper(&digest[..8]))
}

/// Evenly distribute component glyphs on their owning project orbit.
fn ring_angle(index: usize, count: usize, offset: f32) -> f32 {
    std::f32::consts::TAU * index as f32 / count as f32 + offset
}

/// Place technology satellites around configured ownership anchors.
fn technology_position(technology: &crate::TechnologyConfig, config: &Config) -> (f32, f32) {
    let owners = config
        .domains
        .iter()
        .filter(|domain| technology.affinities.contains(&domain.id))
        .collect::<Vec<_>>();

    let center = if owners.is_empty() {
        [
            config.render.width as f32 * 0.5,
            config.render.width as f32 * 0.5,
        ]
    } else {
        [
            owners.iter().map(|domain| domain.anchor[0]).sum::<f32>() / owners.len() as f32,
            owners.iter().map(|domain| domain.anchor[1]).sum::<f32>() / owners.len() as f32,
        ]
    };

    // Placement depends on identity and authored ownership, never changing metrics or node order.
    // Width sets the coordinate scale; unrelated lower-page growth must not move existing nodes.
    let seed = seeded_u64("technology-position-v1", &technology.id);
    let angle = (seed % 6283) as f32 / 1000.0;
    let radius = (config.render.width as f32 * 0.12) + ((seed >> 12) % 100) as f32;

    (
        center[0] + angle.cos() * radius,
        center[1] + angle.sin() * radius * 0.68,
    )
}

/// Derive repeatable decorative placement without a random runtime dependency.
fn seeded_u64(hash: &str, id: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(hash.as_bytes());
    hasher.update(id.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap_or_default())
}

fn affinity_domain(affinities: &[String]) -> Option<String> {
    match affinities {
        [single] => Some(single.clone()),
        _ => None,
    }
}

fn account_url(owner: &str) -> Option<String> {
    (!owner.is_empty()).then(|| format!("https://github.com/{owner}"))
}

fn repository_url(full_name: &str) -> String {
    format!("https://github.com/{full_name}")
}

/// Whether a metadata source has a complete value rather than an unavailable or partial value.
fn source_available(snapshot: &Snapshot, source: &str) -> bool {
    snapshot
        .sources
        .iter()
        .find(|item| item.source.eq_ignore_ascii_case(source))
        .map(|item| {
            matches!(
                item.status,
                crate::DataStatus::Live | crate::DataStatus::Preview | crate::DataStatus::Fallback
            )
        })
        .unwrap_or(true)
}

/// Canonicalize unordered graph declarations; presentation sequences retain their authored order.
fn normalized_config(mut value: Config) -> Config {
    value.domains.sort_by(|a, b| a.id.cmp(&b.id));
    value.projects.sort_by(|a, b| a.id.cmp(&b.id));
    value.technologies.sort_by(|a, b| a.id.cmp(&b.id));
    value.publications.sort_by(|a, b| a.id.cmp(&b.id));
    value.collection.github_organizations.sort();
    for project in &mut value.projects {
        project.components.sort_by(|a, b| a.id.cmp(&b.id));
        project.implemented_with.sort();
        project.integrates.sort();
        project.targets.sort();
        project.tags.sort();
        for component in &mut project.components {
            component.integrates.sort();
            component.targets.sort();
        }
    }

    for technology in &mut value.technologies {
        technology.affinities.sort();
    }

    for publication in &mut value.publications {
        publication.technologies.sort();
        publication.packages.sort_by(|a, b| a.id.cmp(&b.id));
    }

    value
}

/// Normalize collection order for both hashing and geometry to preserve layout identity.
fn normalized_snapshot(mut value: Snapshot) -> Snapshot {
    value
        .repositories
        .sort_by(|a, b| a.full_name.cmp(&b.full_name));
    value.organizations.sort_by(|a, b| a.login.cmp(&b.login));
    value.packages.sort_by(|a, b| a.id.cmp(&b.id));
    value.sources.sort_by(|a, b| a.source.cmp(&b.source));
    for repository in &mut value.repositories {
        repository.topics.sort();
    }

    if let Some(contributions) = &mut value.contributions {
        contributions.days.sort_by(|a, b| a.date.cmp(&b.date));
    }

    value
}

/// Describe authored ownership counts, independently of optional remote repository discovery.
fn domain_summary(config: &Config, domain: &str) -> String {
    let projects = config
        .projects
        .iter()
        .filter(|project| project.domain == domain);

    let public = projects
        .clone()
        .filter(|project| project.visibility == crate::Visibility::Public)
        .count();

    let private = projects
        .filter(|project| project.visibility == crate::Visibility::PrivateAbstract)
        .count();

    if public == 0 {
        format!("{private} private projects")
    } else {
        format!("{public} public / {private} private")
    }
}
