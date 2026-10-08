use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_weight() -> f32 {
    0.5
}

fn default_repository_limit() -> usize {
    100
}

fn default_history_limit() -> usize {
    24
}

fn default_motion_seconds() -> u32 {
    32
}

fn default_detail_level() -> String {
    "abstract".to_string()
}

/// Authored public profile content and collection/rendering policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Shared authored icon definitions, referenced by project icon keys.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub icons: crate::IconCatalog,
    /// Authored configuration schema version (serialized as schema_version).
    #[serde(rename = "schema_version")]
    pub version: u32,
    /// Consumer-owned geometry overrides keyed by canonical graph node ID.
    #[serde(default)]
    pub layout: LayoutConfig,
    /// Explicit canonical organization imports.
    #[serde(default)]
    pub imports: Vec<OrganizationImport>,
    /// Bind an imported `organization/local-technology` to one existing consumer technology.
    /// Definitions must agree on label/category; composition restores the imported affinity.
    #[serde(default)]
    pub shared_technologies: std::collections::BTreeMap<String, String>,
    /// Approved profile identity.
    pub profile: ProfileConfig,
    /// Approved supplementary text and hardware labels.
    #[serde(default)]
    pub presentation: PresentationConfig,
    /// Remote collection and history policy.
    pub collection: CollectionConfig,
    /// Rendering policy.
    pub render: RenderConfig,
    /// Authored ownership groups.
    #[serde(default)]
    pub domains: Vec<DomainConfig>,
    /// Authored technology definitions.
    #[serde(default)]
    pub technologies: Vec<TechnologyConfig>,
    /// Approved project definitions.
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
    /// Public registry publication groups.
    #[serde(default)]
    pub publications: Vec<PublicationConfig>,
    /// Approved personal interests.
    #[serde(default)]
    pub interests: Vec<InterestConfig>,
    /// Approved learning memberships.
    #[serde(default)]
    pub learning: Vec<LearningConfig>,
}

/// Public identity and navigation links.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    /// Explicit presentation variant; never inferred from an account name.
    pub variant: ProfileVariant,
    /// Optional public maintainer attribution.
    #[serde(default)]
    pub maintainer: Option<MaintainerConfig>,
    /// Public GitHub profile handle.
    pub username: String,
    /// Approved personal display name.
    pub display_name: String,
    /// Public organization handle.
    pub organization: String,
    /// Approved concise professional and personal focus.
    pub headline: String,
    /// Approved complete identity line.
    pub tagline: String,
    /// HTTPS URL of the interactive profile.
    pub pages_url: String,
    /// HTTPS URL of this profile repository.
    pub source_url: String,
}

/// Opt-in remote collection boundaries and retention limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionConfig {
    /// Exact NuGet owner whose published packages may be discovered.
    #[serde(default)]
    pub nuget_owner: Option<String>,
    /// GitHub user whose public metadata may be collected.
    #[serde(default)]
    pub github_user: String,
    /// Organizations whose public metadata may be collected.
    #[serde(default)]
    pub github_organizations: Vec<String>,
    /// Whether public repository enumeration is requested.
    #[serde(default = "default_true")]
    pub discover_public_repositories: bool,
    /// Whether discovered public repositories add graph nodes.
    #[serde(default)]
    pub visualize_discovered_repositories: bool,
    /// Whether discovered forks are included.
    #[serde(default)]
    pub include_forks: bool,
    /// Whether archived public repositories are included.
    #[serde(default)]
    pub include_archived: bool,
    /// Whether authenticated contribution collection is requested.
    #[serde(default = "default_true")]
    pub collect_contributions: bool,
    /// Explicit opt-in for a private repository aggregate; names are never collected.
    #[serde(default)]
    pub collect_private_repository_count: bool,
    /// Maximum public repositories retained per collection operation.
    #[serde(default = "default_repository_limit")]
    pub repository_limit: usize,
    /// Maximum semantic snapshots retained in published history.
    #[serde(default = "default_history_limit")]
    pub history_limit: usize,
}

/// Shared output dimensions and visible presentation switches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConfig {
    /// Design-space canvas width in pixels.
    pub width: u32,
    /// Design-space canvas height in pixels.
    pub height: u32,
    /// Duration in seconds of the base decorative animation cycle.
    #[serde(default = "default_motion_seconds")]
    pub motion_seconds: u32,
    /// Initial detail policy: abstract or detailed.
    #[serde(default = "default_detail_level")]
    pub detail_level: String,
    /// Whether the activity visualization is rendered.
    #[serde(default = "default_true")]
    pub show_activity_orbit: bool,
    /// Whether the semantic state identifier is rendered.
    #[serde(default = "default_true")]
    pub show_state_hash: bool,
    /// Whether approved interests appear in the README rendering.
    #[serde(default)]
    pub show_interests_in_readme: bool,
    /// Whether technology labels appear in the visual field.
    #[serde(default = "default_true")]
    pub show_technology_labels: bool,
}

/// An ownership group in the authored project field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainConfig {
    /// Optional repository count policy and text for this ownership domain.
    // Omission preserves legacy serialized inputs and their semantic identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_caption: Option<crate::RepositoryCaptionConfig>,
    /// Canonical public maintainer attribution for this ownership domain.
    #[serde(default)]
    pub maintainer: Option<MaintainerConfig>,
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// GitHub account that owns the object.
    pub owner: String,
    /// Semantic category of this object.
    pub kind: String,
    /// Authored center in design-space pixels.
    pub anchor: [f32; 2],
    /// Approved public description; not a source-code or private architecture excerpt.
    pub summary: String,
}

/// A labeled technology and its ownership affinities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnologyConfig {
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Technology layer used by semantic filtering.
    pub category: String,
    /// Ownership domain identifiers associated with the technology.
    #[serde(default)]
    pub affinities: Vec<String>,
    /// Whether this technology connects multiple ownership groups.
    #[serde(default)]
    pub cross_domain: bool,
    /// Whether this item participates in the README presentation.
    #[serde(default)]
    pub show_in_readme: bool,
}

/// Approved public description of a project, including private projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// Optional built-in or authored icon key; visual retains its palette and fallback role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Optional authored glyph radius in design pixels.
    #[serde(default)]
    pub radius: Option<f32>,
    /// Approved stack labels in display order, distinct from implementation graph edges.
    #[serde(default)]
    pub display_stack: Vec<String>,
    /// Optional visual namespace placed above the compact title.
    #[serde(default)]
    pub label_prefix: Option<String>,
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Compact visual title; the full label remains available to assistive consumers.
    pub surface_label: String,
    /// Required owning domain identifier.
    pub domain: String,
    /// Whether content is public repository metadata or manually approved private-project metadata.
    pub visibility: Visibility,
    /// Authored project lifecycle label.
    pub status: String,
    /// Decorative glyph style; does not encode project facts.
    pub visual: String,
    /// Authored center in design-space pixels.
    pub anchor: [f32; 2],
    /// Normalized visual or relationship weight between zero and one.
    #[serde(default = "default_weight")]
    pub weight: f32,
    /// Approved public description; not a source-code or private architecture excerpt.
    pub summary: String,
    /// Public owner/repository shorthand; private projects must leave it absent.
    #[serde(default)]
    pub repository: Option<String>,
    /// Technology identifiers used to implement this project.
    #[serde(default)]
    pub implemented_with: Vec<String>,
    /// Technology identifiers consumed as integrations.
    #[serde(default)]
    pub integrates: Vec<String>,
    /// Technology identifiers analyzed or targeted, distinct from implementation languages.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Public categorization labels.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether this item participates in the README presentation.
    #[serde(default)]
    pub show_in_readme: bool,
    /// Approved component descriptions.
    #[serde(default)]
    pub components: Vec<ComponentConfig>,
}

/// An approved high-level component label, without private implementation details.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentConfig {
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Semantic category of this object.
    pub kind: String,
    /// Approved high-level component or node scope.
    pub scope: String,
    /// Approved public description; not a source-code or private architecture excerpt.
    pub summary: String,
    /// Technology identifiers consumed as integrations.
    #[serde(default)]
    pub integrates: Vec<String>,
    /// Technology identifiers analyzed or targeted, distinct from implementation languages.
    #[serde(default)]
    pub targets: Vec<String>,
    /// Approved supplementary public text.
    #[serde(default)]
    pub details: Vec<String>,
}

/// A group of public registry packages belonging to one publishing repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationConfig {
    /// Explicit compact labels for package IDs whose names do not share a uniform prefix.
    #[serde(default)]
    pub label_overrides: std::collections::BTreeMap<String, String>,
    /// Verified registry owner for this package group.
    #[serde(default)]
    pub owner: Option<String>,
    /// Exact prefix removed from compact package labels.
    #[serde(default)]
    pub label_prefix: String,
    /// Compact label used when the package ID equals the group label.
    #[serde(default)]
    pub core_label: Option<String>,
    /// Approved package ID roots; only exact IDs or dot-separated descendants match.
    #[serde(default)]
    pub discovery_prefixes: Vec<String>,
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Compact visual title; the full label remains available to assistive consumers.
    pub surface_label: String,
    /// Required owning domain identifier.
    pub domain: String,
    /// Public package registry identifier.
    pub registry: String,
    /// Authored center in design-space pixels.
    pub anchor: [f32; 2],
    /// Approved public description; not a source-code or private architecture excerpt.
    pub summary: String,
    /// Technology identifiers associated with this publication.
    #[serde(default)]
    pub technologies: Vec<String>,
    /// Whether this item participates in the README presentation.
    #[serde(default)]
    pub show_in_readme: bool,
    /// Authored public package definitions.
    #[serde(default)]
    pub packages: Vec<PackageConfig>,
}

/// An explicitly linked public package and its approved description.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConfig {
    /// Optional authored package glyph center in design pixels.
    #[serde(default)]
    pub anchor: Option<[f32; 2]>,
    /// Approved public description; not a source-code or private architecture excerpt.
    #[serde(default)]
    pub summary: String,
    /// Stable identifier within its namespace.
    pub id: String,
    /// Package role or family label.
    pub family: String,
    /// Required public HTTPS destination.
    pub url: String,
}

/// An interest label distinct from a claim of professional expertise.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterestConfig {
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Approved public description; not a source-code or private architecture excerpt.
    pub summary: String,
    /// Whether this item participates in the README presentation.
    #[serde(default)]
    pub show_in_readme: bool,
}

/// A learning membership link, not a certification claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearningConfig {
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Required public HTTPS destination.
    pub url: String,
    /// Identifier of the associated configured interest.
    pub interest: String,
}

/// Publication boundary for curated project metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    /// Public repository metadata may include its public link.
    Public,
    /// Only manually approved names and summaries are public; repository links are absent.
    PrivateAbstract,
}

/// Collected or offline metadata with explicit per-source provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// Explicit collected-input schema, distinct from generated graph schema.
    pub schema_version: u32,
    /// Per-source completeness and provenance; missing sources must not imply fresh metadata.
    #[serde(default)]
    pub sources: Vec<SourceStatus>,
    /// Overall snapshot provenance.
    #[serde(default)]
    pub mode: SnapshotMode,
    /// Metadata collection timestamp; excluded from semantic identity.
    #[serde(default)]
    pub fetched_at: String,
    /// Observed public user account.
    #[serde(default)]
    pub user: AccountSnapshot,
    /// Observed public organization accounts.
    #[serde(default)]
    pub organizations: Vec<AccountSnapshot>,
    /// Discovered public repository metadata.
    #[serde(default)]
    pub repositories: Vec<RepositorySnapshot>,
    /// Optional contribution data; absent when not collected.
    #[serde(default)]
    pub contributions: Option<ContributionSnapshot>,
    /// Optional owned-private-repository aggregate authorized by the collecting caller.
    /// The pipeline must clear this value unless effective config, CLI, or environment opt-in
    /// and the required credential permit publication.
    #[serde(default)]
    pub private_repository_count: Option<u32>,
    /// Observed public package metadata.
    #[serde(default)]
    pub packages: Vec<PackageSnapshot>,
    /// Sanitized public diagnostics; never raw upstream errors.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Overall provenance of a metadata snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotMode {
    /// All requested data was collected from its live source.
    Live,
    /// Only part of the requested data is available.
    Partial,
    /// Historical data substituted after a live collection failure.
    Fallback,
    /// Historical offline sample, not current live data.
    #[default]
    Preview,
}

/// Public account metadata returned by the collection source.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountSnapshot {
    /// GitHub account login returned by the source.
    #[serde(default)]
    pub login: String,
    /// Approved personal display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Optional public account profile URL.
    #[serde(default)]
    pub profile_url: Option<String>,
    /// Observed public repository count; source status determines availability.
    #[serde(default)]
    pub public_repositories: u32,
    /// Observed follower count, or unknown where the state uses an optional value.
    #[serde(default)]
    pub followers: u32,
}

/// Public repository metadata; never populated from private repository discovery.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositorySnapshot {
    /// GitHub account that owns the object.
    #[serde(default)]
    pub owner: String,
    /// Public repository name.
    #[serde(default)]
    pub name: String,
    /// Public repository owner/name identity.
    #[serde(default)]
    pub full_name: String,
    /// Public repository URL; defaults to empty for an unpopulated snapshot.
    #[serde(default)]
    pub url: String,
    /// Optional public repository description.
    #[serde(default)]
    pub description: Option<String>,
    /// Optional primary language reported by GitHub.
    #[serde(default)]
    pub primary_language: Option<String>,
    /// Public repository topics.
    #[serde(default)]
    pub topics: Vec<String>,
    /// Observed public star count.
    #[serde(default)]
    pub stars: u32,
    /// Observed public fork count.
    #[serde(default)]
    pub forks: u32,
    /// Whether GitHub reports the repository as archived.
    #[serde(default)]
    pub archived: bool,
    /// Whether the public repository is a fork.
    #[serde(default)]
    pub fork: bool,
    /// Optional upstream push timestamp.
    #[serde(default)]
    pub pushed_at: Option<String>,
}

/// Contribution totals and daily activity provided by GitHub.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContributionSnapshot {
    /// Contribution total returned by GitHub.
    #[serde(default)]
    pub total: u32,
    /// Commit contribution count.
    #[serde(default)]
    pub commits: u32,
    /// Issue contribution count.
    #[serde(default)]
    pub issues: u32,
    /// Pull-request contribution count.
    #[serde(default)]
    pub pull_requests: u32,
    /// Review contribution count.
    #[serde(default)]
    pub reviews: u32,
    /// Restricted contribution aggregate without private item identities.
    #[serde(default)]
    pub restricted: u32,
    /// Daily activity observations.
    #[serde(default)]
    pub days: Vec<ActivityDay>,
}

/// One day of contribution activity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityDay {
    /// Activity date as an upstream calendar string.
    #[serde(default)]
    pub date: String,
    /// Number of contributions on this date.
    #[serde(default)]
    pub count: u32,
    /// Upstream activity intensity from zero through four.
    #[serde(default)]
    pub level: u8,
}

/// Observed package metadata; unavailable values remain absent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSnapshot {
    /// Verified NuGet owner returned by the owner-scoped search.
    #[serde(default)]
    pub owner: Option<String>,
    /// Stable identifier within its namespace.
    #[serde(default)]
    pub id: String,
    /// Observed package version, absent when no version was collected.
    #[serde(default)]
    pub version: Option<String>,
    /// Observed total package downloads, absent when unavailable.
    #[serde(default)]
    pub total_downloads: Option<u64>,
    /// Optional upstream update timestamp.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Optional canonical NuGet package page observed from the source.
    #[serde(default)]
    pub url: Option<String>,
}

/// Canonical graph and presentation consumed by all output surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileState {
    /// Shared authored icon definitions, stored once for all project nodes.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub icons: crate::IconCatalog,
    /// Selected organization identities and canonical maintainer attribution.
    #[serde(default)]
    pub organizations: Vec<OrganizationIdentity>,
    /// Approved supplementary text and hardware labels.
    #[serde(default)]
    pub presentation: PresentationConfig,
    /// Approved personal interests.
    #[serde(default)]
    pub interests: Vec<InterestConfig>,
    /// Approved learning memberships.
    #[serde(default)]
    pub learning: Vec<LearningConfig>,
    /// Per-source completeness and provenance; missing sources must not imply fresh metadata.
    #[serde(default)]
    pub sources: Vec<SourceStatus>,
    /// Generated state schema version.
    #[serde(rename = "schema_version")]
    pub schema: u32,
    /// Overall snapshot provenance.
    pub mode: SnapshotMode,
    /// Artifact generation timestamp; excluded from semantic identity.
    pub generated_at: String,
    /// Short deterministic digest of normalized semantic input.
    pub semantic_hash: String,
    /// Shared coordinate and rendering contract.
    pub canvas: Canvas,
    /// Approved profile identity.
    pub profile: StateProfile,
    /// Availability-aware public counters.
    pub stats: StateStats,
    /// Canonical graph objects.
    pub nodes: Vec<Node>,
    /// Typed graph relationships.
    pub edges: Vec<Edge>,
    /// Observed daily contribution activity.
    pub activity: Vec<ActivityDay>,
    /// Observed public package metadata.
    pub packages: Vec<PackageSnapshot>,
    /// Sanitized public diagnostics; never raw upstream errors.
    pub warnings: Vec<String>,
}

/// Shared design coordinates and rendering policy for every consumer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    /// Whether the activity visualization is rendered.
    pub show_activity_orbit: bool,
    /// Whether the semantic state identifier is rendered.
    pub show_state_hash: bool,
    /// Whether approved interests appear in the README rendering.
    pub show_interests_in_readme: bool,
    /// Whether technology labels appear in the visual field.
    pub show_technology_labels: bool,
    /// Whether expanded approved descriptions are visible by default.
    pub show_details: bool,
    /// Design-space canvas width in pixels.
    pub width: u32,
    /// Design-space canvas height in pixels.
    pub height: u32,
    /// Duration in seconds of the base decorative animation cycle.
    pub motion_seconds: u32,
}

/// Public profile identity copied from approved configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateProfile {
    /// Explicit presentation variant; never inferred from an account name.
    pub variant: ProfileVariant,
    /// Optional public maintainer attribution.
    #[serde(default)]
    pub maintainer: Option<MaintainerConfig>,
    /// Public GitHub profile handle.
    pub username: String,
    /// Approved personal display name.
    pub display_name: String,
    /// Public organization handle.
    pub organization: String,
    /// Approved concise professional and personal focus.
    pub headline: String,
    /// Approved complete identity line.
    pub tagline: String,
    /// HTTPS URL of the interactive profile.
    pub pages_url: String,
    /// HTTPS URL of this profile repository.
    pub source_url: String,
}

/// Metadata counters; absence denotes unknown rather than zero.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateStats {
    /// Known personal public repository count, or None when unavailable.
    pub personal_public_repositories: Option<u32>,
    /// Known total for all configured organizations, or None when any is unavailable.
    pub organization_public_repositories: Option<u32>,
    /// Known public follower count, absent when unavailable.
    pub followers: Option<u32>,
    /// Number of authored and owner-discovered packages in the expanded inventory.
    pub package_count: u32,
    /// Total only when every configured package has a known download count.
    pub package_downloads: Option<u64>,
    /// Known contribution total, absent when collection is unavailable.
    pub contribution_total: Option<u32>,
    /// Optional owned-private-repository aggregate authorized by the collecting caller.
    /// The pipeline must clear this value unless effective config, CLI, or environment opt-in
    /// and the required credential permit publication.
    pub private_repository_count: Option<u32>,
}

/// Semantic category of a graph node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeKind {
    /// Ownership group.
    Domain,
    /// Authored or discovered project.
    Project,
    /// Approved high-level project component.
    Component,
    /// Technology capability or target.
    Technology,
    /// Public package group.
    Publication,
    /// Individual published package.
    Package,
    /// Personal interest.
    Interest,
}

/// A graph object at a stable design-space coordinate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Optional resolved repository caption, supported only on ownership-domain nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_caption: Option<String>,
    /// Optional project icon key resolved against the shared state catalog and built-ins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Approved stack labels in display order, distinct from implementation graph edges.
    #[serde(default)]
    pub display_stack: Vec<String>,
    /// Optional visual namespace placed above the compact title.
    #[serde(default)]
    pub label_prefix: Option<String>,
    /// Stable identifier within its namespace.
    pub id: String,
    /// Full public display label.
    pub label: String,
    /// Compact visual title; the full label remains available to assistive consumers.
    pub surface_label: String,
    /// Semantic category of this object.
    pub kind: NodeKind,
    /// Owning domain identifier, absent for cross-domain objects.
    pub domain: Option<String>,
    /// Horizontal coordinate in shared design pixels.
    pub x: f32,
    /// Vertical coordinate in shared design pixels.
    pub y: f32,
    /// Positive glyph radius in design pixels.
    pub radius: f32,
    /// Normalized visual or relationship weight between zero and one.
    pub weight: f32,
    /// Approved public description; not a source-code or private architecture excerpt.
    pub summary: String,
    /// Approved supplementary public text.
    #[serde(default)]
    pub details: Vec<String>,
    /// Public categorization labels.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Public HTTPS destination; absent for curated private project nodes.
    #[serde(default)]
    pub url: Option<String>,
    /// Whether content is public repository metadata or manually approved private-project metadata.
    #[serde(default)]
    pub visibility: Option<Visibility>,
    /// Whether this item participates in the README presentation.
    pub show_in_readme: bool,
    /// Decorative glyph style; does not encode project facts.
    #[serde(default)]
    pub visual: Option<String>,
    /// Approved high-level component or node scope.
    #[serde(default)]
    pub scope: Option<String>,
    /// Package role or family label.
    #[serde(default)]
    pub family: Option<String>,
}

/// Meaning of a graph relationship; ownership is separate from implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    /// Ownership relationship.
    Contains,
    /// Approved high-level project component.
    Component,
    /// Implementation-language relationship.
    ImplementedWith,
    /// External integration relationship.
    Integrates,
    /// Analysis or deployment target relationship.
    Targets,
    /// Publication-group membership.
    Publishes,
    /// Technology ownership affinity.
    Affinity,
    /// Technology shared across ownership groups.
    SharedCapability,
}

/// A directed, typed connection between existing graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    /// Source node identifier.
    pub from: String,
    /// Destination node identifier.
    pub to: String,
    /// Semantic category of this object.
    pub kind: EdgeKind,
    /// Normalized visual or relationship weight between zero and one.
    pub weight: f32,
    /// Whether this item participates in the README presentation.
    pub show_in_readme: bool,
}

/// Approved supplementary profile text in its authored display order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationConfig {
    /// Primary technology labels in authored display order.
    #[serde(default)]
    pub main_stack: Vec<String>,
    /// Supporting technology labels in authored display order.
    #[serde(default)]
    pub supporting_stack: Vec<String>,
    /// Operating-system labels in authored display order.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Approved context for platform usage.
    #[serde(default)]
    pub platform_note: String,
    /// Membership and learning context; may contain explicit line breaks.
    #[serde(default)]
    pub learning_note: String,
    /// User-approved hardware descriptions in display order.
    #[serde(default)]
    pub hardware: Vec<HardwareConfig>,
}

/// User-supplied private hardware description.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HardwareConfig {
    /// Full public display label.
    pub label: String,
    /// Approved hardware configuration or usage note.
    pub detail: String,
}

/// Completeness and provenance of one remote data source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceStatus {
    /// Stable source key, such as github:user or nuget:package.id.
    pub source: String,
    /// Completeness and provenance of this metadata source.
    pub status: DataStatus,
}

/// Provenance or availability of one metadata source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataStatus {
    /// All requested data was collected from its live source.
    Live,
    /// Historical offline sample, not current live data.
    Preview,
    /// Only part of the requested data is available.
    Partial,
    /// No complete observation is available.
    #[default]
    Missing,
    /// Historical data substituted after a live collection failure.
    Fallback,
}

/// Presentation structure selected explicitly by the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileVariant {
    /// Personal identity with private project and interest sections.
    Personal,
    /// Organization identity with optional maintainer attribution.
    Organization,
}

/// Public maintainer attribution approved by the organization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintainerConfig {
    /// Public account handle.
    pub username: String,
    /// Approved role description.
    pub role: String,
    /// Public HTTPS account link.
    pub url: String,
}

/// Consumer-owned geometry; canonical organization facts contain no coordinates.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutConfig {
    /// Authored project radii keyed by fully scoped project graph IDs.
    #[serde(default)]
    pub radii: std::collections::BTreeMap<String, f32>,
    /// Authored project visual weights keyed by fully scoped project graph IDs.
    #[serde(default)]
    pub weights: std::collections::BTreeMap<String, f32>,
    /// Explicit centers keyed by fully scoped graph node identifiers.
    #[serde(default)]
    pub overrides: std::collections::BTreeMap<String, [f32; 2]>,
}

/// A canonical organization source selected by the consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationImport {
    /// Stable scope expected in the imported manifest.
    pub id: String,
    /// Explicit local or remote content origin.
    pub source: OrganizationSource,
}

/// Supported authored organization import origins.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OrganizationSource {
    /// Local preview input relative to the configuration directory.
    Local {
        /// Relative manifest path.
        path: String,
    },
    /// Public GitHub source resolved to one immutable commit per collection.
    Remote {
        /// Public owner/repository identity.
        repository: String,
        /// Authored branch, tag, or immutable commit reference.
        #[serde(rename = "ref")]
        reference: String,
        /// Manifest path within the repository.
        path: String,
    },
}

/// Canonical public identity of an organization selected by this profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationIdentity {
    /// Stable scope independent of the current account handle.
    pub id: String,
    /// Public account handle.
    pub owner: String,
    /// Approved display label.
    pub label: String,
    /// Optional approved public maintainer attribution.
    pub maintainer: Option<MaintainerConfig>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sources: Vec::new(),
            mode: SnapshotMode::default(),
            fetched_at: String::new(),
            user: AccountSnapshot::default(),
            organizations: Vec::new(),
            repositories: Vec::new(),
            contributions: None,
            private_repository_count: None,
            packages: Vec::new(),
            warnings: Vec::new(),
        }
    }
}
