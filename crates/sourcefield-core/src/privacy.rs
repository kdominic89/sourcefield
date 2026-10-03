//! Publication boundary applied before hashing or projecting any collected metadata.

use crate::{
    AccountSnapshot, Config, DataStatus, ProfileVariant, Snapshot, SnapshotMode, SourceStatus,
    package_matches, publication_owner,
};

/// Remove metadata outside the explicitly selected owners and publication groups.
///
/// Organization profiles never publish personal contribution or private-count aggregates.
/// Filtering the raw snapshot as well as graph nodes prevents unrelated data leaking through
/// downloadable JSON, warnings, or a later renderer that exposes additional fields.
pub fn scope_snapshot(config: &Config, snapshot: &Snapshot) -> Snapshot {
    let mut scoped = snapshot.clone();
    let owner_selected = |owner: &str| {
        if config.profile.variant == ProfileVariant::Organization {
            return owner.eq_ignore_ascii_case(&config.profile.organization);
        }

        (config.profile.variant == ProfileVariant::Personal
            && owner.eq_ignore_ascii_case(&config.collection.github_user))
            || config
                .collection
                .github_organizations
                .iter()
                .any(|selected| selected.eq_ignore_ascii_case(owner))
    };

    scoped.organizations.retain(|account| {
        owner_selected(&account.login)
            && config
                .collection
                .github_organizations
                .iter()
                .any(|owner| owner.eq_ignore_ascii_case(&account.login))
    });
    scoped.repositories.retain(|repository| {
        owner_selected(&repository.owner)
            && repository
                .full_name
                .eq_ignore_ascii_case(&format!("{}/{}", repository.owner, repository.name))
    });
    scoped.packages.retain(|package| {
        config.publications.iter().any(|group| {
            let selected = group
                .packages
                .iter()
                .any(|known| known.id.eq_ignore_ascii_case(&package.id))
                || package_matches(group, &package.id);

            let owner_matches = match (publication_owner(config, group), package.owner.as_deref()) {
                (Some(expected), Some(actual)) => expected.eq_ignore_ascii_case(actual),
                (None, _) => true,
                // Explicit package registration observations do not include owner claims; an
                // owner-scoped discovery package must carry a verified owner to be admitted.
                (Some(_), None) => group
                    .packages
                    .iter()
                    .any(|known| known.id.eq_ignore_ascii_case(&package.id)),
            };

            selected && owner_matches
        })
    });
    // A failed authored package has no observation payload; retain its approved source key
    // so removing unavailable data cannot also remove the explanation for its absence.
    let package_sources = scoped
        .packages
        .iter()
        .map(|package| format!("nuget:{}", package.id.to_ascii_lowercase()))
        .chain(
            config
                .publications
                .iter()
                .flat_map(|group| &group.packages)
                .map(|package| format!("nuget:{}", package.id.to_ascii_lowercase())),
        )
        .collect::<std::collections::BTreeSet<_>>();

    let repository_owner_selected = |owner: &str| {
        config.collection.discover_public_repositories
            && owner_selected(owner)
            && ((config.profile.variant == ProfileVariant::Personal
                && owner.eq_ignore_ascii_case(&config.collection.github_user))
                || config
                    .collection
                    .github_organizations
                    .iter()
                    .any(|selected| selected.eq_ignore_ascii_case(owner)))
    };

    let repository_collection_selected = repository_owner_selected(&config.collection.github_user)
        || config
            .collection
            .github_organizations
            .iter()
            .any(|owner| repository_owner_selected(owner));

    let nuget_selected = config
        .publications
        .iter()
        .any(|group| group.registry == "nuget");

    scoped.sources.retain(|source| {
        if let Some(owner) = source.source.strip_prefix("github:repositories:") {
            return repository_owner_selected(owner);
        }

        if let Some(owner) = source
            .source
            .strip_prefix("github:org:")
            .or_else(|| source.source.strip_prefix("github:organization:"))
        {
            return owner_selected(owner)
                && config
                    .collection
                    .github_organizations
                    .iter()
                    .any(|selected| selected.eq_ignore_ascii_case(owner));
        }

        if let Some(owner) = source.source.strip_prefix("nuget:owner:") {
            return config.publications.iter().any(|group| {
                publication_owner(config, group)
                    .is_some_and(|selected| selected.eq_ignore_ascii_case(owner))
            });
        }

        match source.source.as_str() {
            // Earlier captures combine all repository owners. Keep that fixed, owner-free
            // status when repository collection applies: a missing aggregate cannot prove
            // that this selected organization's inventory was healthy.
            "github:repositories" => repository_collection_selected,
            "nuget:service-index" => nuget_selected,
            "collection:partial" => snapshot.mode == SnapshotMode::Partial,
            "github:user" | "github:private-count" => {
                config.profile.variant == ProfileVariant::Personal
            }
            "github:contributions" => {
                config.profile.variant == ProfileVariant::Personal
                    && config.collection.collect_contributions
            }
            _ => package_sources.contains(&source.source.to_ascii_lowercase()),
        }
    });

    // Never promote Partial to Live merely because its original reasons were filtered out.
    // Historical captures may omit statuses or combine selected and unselected failures.
    // A fixed marker preserves that uncertainty without publishing private identities,
    // unselected source strings, or untrusted warning text. Re-scoping is idempotent.
    if scoped.mode == SnapshotMode::Partial
        && !scoped.sources.iter().any(|source| {
            matches!(
                source.status,
                DataStatus::Missing | DataStatus::Partial | DataStatus::Fallback
            )
        })
    {
        scoped
            .sources
            .retain(|source| source.source != "collection:partial");
        scoped.sources.push(SourceStatus {
            source: "collection:partial".into(),
            status: DataStatus::Partial,
        });
    }

    if config.profile.variant == ProfileVariant::Organization {
        scoped.user = AccountSnapshot::default();
        scoped.contributions = None;
        scoped.private_repository_count = None;
        // Diagnostics can contain source identifiers; organization outputs expose only
        // structured, selected source statuses rather than caller-wide diagnostics.
        scoped.warnings.clear();
    }

    scoped
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::{DataStatus, SnapshotMode, SourceStatus};
    use serde_json::json;

    fn organization_config() -> Config {
        serde_json::from_value(json!({
            "schema_version":1,
            "profile":{"variant":"organization", "username":"example-labs", "organization":"example-labs",
                "display_name":"Example", "headline":"Tools", "tagline":"Tools", "pages_url":"https://example.invalid",
                "source_url":"https://github.com/example-labs/.github"},
            "collection":{"github_user":"", "github_organizations":["example-labs"], "nuget_owner":"example-labs"},
            "render":{"width":1200,"height":1200},
            "publications":[{"id":"tools","label":"Example.Tools","surface_label":"Tools","domain":"example-labs",
                "registry":"nuget","anchor":[100,100],"summary":"Tools","packages":[
                    {"id":"Example.Tools","family":"tools","url":"https://www.nuget.org/packages/Example.Tools/"}]}]
        })).unwrap()
    }

    fn snapshot(mode: SnapshotMode, sources: &[(&str, DataStatus)]) -> Snapshot {
        Snapshot {
            mode,
            sources: sources
                .iter()
                .map(|(source, status)| SourceStatus {
                    source: (*source).into(),
                    status: *status,
                })
                .collect(),
            warnings: vec!["SECRET private-owner private-name".into()],
            ..Snapshot::default()
        }
    }

    #[test]
    fn selected_organization_repository_failure_survives_scoping() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Partial,
            &[("github:repositories:example-labs", DataStatus::Missing)],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert!(
            scoped
                .sources
                .iter()
                .any(|source| source.source == "github:repositories:example-labs"
                    && source.status == DataStatus::Missing)
        );
        assert!(scoped.warnings.is_empty());
    }

    #[test]
    fn unrelated_private_failures_leave_only_a_fixed_partial_explanation() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Partial,
            &[
                ("github:private-count", DataStatus::Missing),
                ("github:repositories:private-owner", DataStatus::Missing),
                ("github:org:private-owner", DataStatus::Missing),
            ],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert_eq!(scoped.sources.len(), 1);
        assert_eq!(scoped.sources[0].source, "collection:partial");
        let serialized = serde_json::to_string(&scoped).unwrap();
        assert!(!serialized.contains("private-owner"));
        assert!(!serialized.contains("private-count"));
        assert!(!serialized.contains("SECRET"));
    }

    #[test]
    fn selected_nuget_service_failure_survives_without_package_payloads() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Partial,
            &[("nuget:service-index", DataStatus::Missing)],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.sources[0].source, "nuget:service-index");
        assert_eq!(scoped.sources[0].status, DataStatus::Missing);
    }

    #[test]
    fn authored_package_failure_survives_without_observed_package() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Partial,
            &[("nuget:example.tools", DataStatus::Missing)],
        );

        let scoped = scope_snapshot(&config, &input);

        assert!(scoped.packages.is_empty());
        assert_eq!(scoped.sources[0].source, "nuget:example.tools");
        assert_eq!(scoped.sources[0].status, DataStatus::Missing);
    }

    #[test]
    fn ambiguous_legacy_repository_failure_is_retained_conservatively() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Partial,
            &[("github:repositories", DataStatus::Missing)],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert_eq!(scoped.sources[0].source, "github:repositories");
        assert_eq!(scoped.sources[0].status, DataStatus::Missing);
    }

    #[test]
    fn legacy_partial_without_statuses_gets_a_sanitized_explanation() {
        let config = organization_config();
        let input = snapshot(SnapshotMode::Partial, &[]);

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert_eq!(scoped.sources[0].source, "collection:partial");
        assert_eq!(scoped.sources[0].status, DataStatus::Partial);
        assert!(scoped.warnings.is_empty());
    }

    #[test]
    fn healthy_zero_inventory_remains_live_without_a_partial_marker() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Live,
            &[
                ("github:org:example-labs", DataStatus::Live),
                ("github:repositories:example-labs", DataStatus::Live),
                ("nuget:service-index", DataStatus::Live),
            ],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Live);
        assert!(scoped.repositories.is_empty());
        assert_eq!(scoped.sources.len(), 3);
        assert!(
            !scoped
                .sources
                .iter()
                .any(|source| source.source == "collection:partial")
        );
    }

    #[test]
    fn unselected_nuget_service_is_removed_without_promoting_partial() {
        let mut config = organization_config();
        config.publications.clear();
        let input = snapshot(
            SnapshotMode::Partial,
            &[("nuget:service-index", DataStatus::Missing)],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert_eq!(scoped.sources.len(), 1);
        assert_eq!(scoped.sources[0].source, "collection:partial");
    }
    #[test]
    fn existing_sanitized_partial_marker_is_not_duplicated() {
        let config = organization_config();
        let input = snapshot(
            SnapshotMode::Partial,
            &[("collection:partial", DataStatus::Partial)],
        );

        let scoped = scope_snapshot(&config, &input);

        assert_eq!(scoped.mode, SnapshotMode::Partial);
        assert_eq!(scoped.sources.len(), 1);
        assert_eq!(scoped.sources[0].source, "collection:partial");
        assert_eq!(scoped.sources[0].status, DataStatus::Partial);
    }
}
