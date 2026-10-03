//! Redacted, actionable strict-publication diagnostics from approved source identities.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use sourcefield_collector::CollectorWarning;
use sourcefield_core::{Config, DataStatus, Snapshot, SnapshotMode};

use crate::observations::{FALLBACK_WARNING, PREVIEW_WARNING, PRIVATE_COUNT_WARNING};

/// Refuse incomplete strict publication while exposing only approved source names and fixed text.
pub(crate) fn require_live(config: &Config, snapshot: &Snapshot, strict: bool) -> Result<()> {
    if !strict
        || (snapshot.mode == SnapshotMode::Live
            && snapshot.warnings.is_empty()
            && snapshot
                .sources
                .iter()
                .all(|source| source.status == DataStatus::Live))
    {
        return Ok(());
    }

    // Captures and adapter diagnostics are untrusted input. Never interpolate arbitrary warnings
    // or a source identifier merely because it resembles a valid owner or package name.
    let mut approved = BTreeSet::from([
        "github:user".to_string(),
        "github:repositories".to_string(),
        "github:contributions".to_string(),
        "github:private-count".to_string(),
        "nuget:service-index".to_string(),
        "collection:partial".to_string(),
    ]);
    approved.insert(format!(
        "github:repositories:{}",
        config.collection.github_user
    ));
    for owner in &config.collection.github_organizations {
        approved.insert(format!("github:org:{owner}"));
        approved.insert(format!("github:organization:{owner}"));
        approved.insert(format!("github:repositories:{owner}"));
    }

    for group in &config.publications {
        if let Some(owner) = sourcefield_core::publication_owner(config, group) {
            approved.insert(format!("nuget:owner:{}", owner.to_ascii_lowercase()));
        }

        for package in &group.packages {
            approved.insert(format!("nuget:{}", package.id.to_ascii_lowercase()));
        }
    }

    let mut details = BTreeSet::new();
    let mut withheld = 0;
    for source in &snapshot.sources {
        if source.status == DataStatus::Live {
            continue;
        }

        if approved.contains(&source.source) {
            details.insert(format!("{}={:?}", source.source, source.status));
        } else {
            withheld += 1;
        }
    }

    for warning in &snapshot.warnings {
        if let Some(known) = CollectorWarning::parse(warning)
            && let Some(message) = approved_collector_warning(known, &approved, config)
        {
            details.insert(message);
            continue;
        }

        let message = match warning.as_str() {
            PRIVATE_COUNT_WARNING => "private aggregate requested without PROFILE_TOKEN",
            FALLBACK_WARNING => "live collection failed; only a dated fallback is available",
            PREVIEW_WARNING => "offline preview is not a live observation",
            _ => {
                withheld += 1;
                continue;
            }
        };

        details.insert(message.to_string());
    }

    if withheld > 0 {
        details.insert(format!(
            "{withheld} additional diagnostic(s) withheld to protect private data"
        ));
    }

    if details.is_empty() {
        details.insert("no detailed source status is available".into());
    }

    bail!(
        "strict-live requires every requested source to be complete and live; mode={:?}; {}",
        snapshot.mode,
        details.into_iter().collect::<Vec<_>>().join("; ")
    )
}

/// Authorize parsed payloads against configuration before formatting any identifier.
fn approved_collector_warning(
    warning: CollectorWarning<'_>,
    approved: &BTreeSet<String>,
    config: &Config,
) -> Option<String> {
    match warning {
        CollectorWarning::RepositoryLimit => Some(
            "repository_limit or pagination bound reached; full inventory was not proven".into(),
        ),
        CollectorWarning::SourceUnavailable(source) => approved
            .contains(source)
            .then(|| CollectorWarning::SourceUnavailable(source).to_string()),
        CollectorWarning::NugetNoExactMatch(package)
        | CollectorWarning::NugetMetadataUnavailable(package) => {
            // Package IDs have their own namespace: a service source must not authorize a
            // similarly named package. Validate before scanning or allocating for capture text.
            if !sourcefield_core::valid_package_id(package)
                || !config
                    .publications
                    .iter()
                    .flat_map(|group| &group.packages)
                    .any(|configured| configured.id.eq_ignore_ascii_case(package))
            {
                return None;
            }

            Some(warning.to_string())
        }
    }
}
