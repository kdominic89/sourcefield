//! Owner-scoped publication discovery, cached provenance, and README projection.
use crate::{
    Config, DataStatus, PackageConfig, PublicationConfig, Snapshot, SnapshotMode, SourceStatus,
};

/// Resolve a group-specific owner, falling back to the authored default owner.
pub fn publication_owner<'a>(
    config: &'a Config,
    publication: &'a PublicationConfig,
) -> Option<&'a str> {
    publication
        .owner
        .as_deref()
        .or(config.collection.nuget_owner.as_deref())
}

/// Match an approved package root without admitting lookalike prefixes.
pub fn package_matches(publication: &PublicationConfig, id: &str) -> bool {
    if !valid_package_id(id) {
        return false;
    }

    publication
        .discovery_prefixes
        .iter()
        .any(|prefix| package_id_has_root(id, prefix))
}

/// Expand only verified owner packages within configured publication families.
/// Authored package positions remain stable; newly published adapters append below them.
pub fn expanded_config(config: &Config, snapshot: &Snapshot) -> Config {
    let mut expanded = config.clone();
    let mut observed = snapshot.packages.iter().collect::<Vec<_>>();

    observed.sort_by(|left, right| left.id.cmp(&right.id));

    for package in observed {
        if package.version.is_none() {
            continue;
        }

        let Some(group) = expanded.publications.iter_mut().find(|group| {
            package_matches(group, &package.id)
                && package
                    .owner
                    .as_deref()
                    .zip(publication_owner(config, group))
                    .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
        }) else {
            continue;
        };

        if group
            .packages
            .iter()
            .any(|known| known.id.eq_ignore_ascii_case(&package.id))
        {
            continue;
        }

        // Graph normalization already orders packages by ID. Resolve every existing implicit
        // anchor in that same order before insertion can change its ordinal position.
        group.packages.sort_by(|left, right| left.id.cmp(&right.id));
        for (index, package) in group.packages.iter_mut().enumerate() {
            package.anchor = Some(crate::package_anchor(group.anchor, package, index));
        }

        let x = group
            .packages
            .first()
            .and_then(|package| package.anchor)
            .map_or_else(
                || crate::default_package_anchor(group.anchor, 0)[0],
                |anchor| anchor[0],
            );
        let y = group
            .packages
            .iter()
            .filter_map(|package| package.anchor)
            .map(|anchor| anchor[1])
            .reduce(f32::max)
            .map_or_else(
                || crate::default_package_anchor(group.anchor, 0)[1],
                |last| last + crate::PACKAGE_ROW_SPACING,
            );

        let label = package_label(group, &package.id);
        group.packages.push(PackageConfig {
            id: package.id.clone(),
            family: group.id.clone(),
            summary: format!("{label} package"),
            url: format!("https://www.nuget.org/packages/{}/", package.id),
            anchor: Some([x, y]),
        });
    }

    // A growing row shifts all subsequent rows together; using only the global maximum
    // package count would allow an earlier row to overlap a later group's heading.
    let mut rows = config
        .publications
        .iter()
        .map(|group| group.anchor[1])
        .collect::<Vec<_>>();

    rows.sort_by(f32::total_cmp);
    rows.dedup();
    let mut shift = 0.0_f32;

    for row in rows {
        let original = config
            .publications
            .iter()
            .filter(|group| group.anchor[1] == row)
            .map(|group| group.packages.len())
            .max()
            .unwrap_or(0);

        let current = expanded
            .publications
            .iter()
            .filter(|group| {
                config
                    .publications
                    .iter()
                    .any(|original| original.id == group.id && original.anchor[1] == row)
            })
            .map(|group| group.packages.len())
            .max()
            .unwrap_or(0);

        for group in &mut expanded.publications {
            if config
                .publications
                .iter()
                .any(|old| old.id == group.id && old.anchor[1] == row)
            {
                group.anchor[1] += shift;

                for package in &mut group.packages {
                    if let Some(anchor) = &mut package.anchor {
                        anchor[1] += shift;
                    }
                }
            }
        }

        shift += current.saturating_sub(original) as f32 * crate::PACKAGE_ROW_SPACING;
    }

    expanded.render.height = expanded.render.height.saturating_add(shift as u32);

    expanded
}

/// Compact a row label while preserving its full ID in links and accessible text.
pub fn package_label(group: &PublicationConfig, id: &str) -> String {
    if let Some(label) = group.label_overrides.get(id) {
        return label.clone();
    }

    if id == group.label
        && let Some(label) = &group.core_label
    {
        return label.clone();
    }

    id.strip_prefix(&group.label_prefix)
        .unwrap_or(id)
        .to_string()
}

/// Whether a capture can serve as a dated observation after live collection fails.
/// Preview seeds and absent or malformed RFC 3339 timestamps are never fallback evidence.
/// Live, partial, and fallback captures retain their original observation date; this predicate
/// does not assert freshness or authorize private data. Per-source eligibility is checked separately.
pub fn usable_observation(snapshot: &Snapshot) -> bool {
    snapshot.mode != SnapshotMode::Preview
        && chrono::DateTime::parse_from_rfc3339(&snapshot.fetched_at).is_ok()
}

/// Restore NuGet observations only when their source failed; never label cached data live.
pub fn restore_package_cache(snapshot: &mut Snapshot, cache: &Snapshot, config: &Config) {
    if !usable_observation(cache) {
        return;
    }

    for cached in &cache.packages {
        let selected = config.publications.iter().find(|group| {
            let explicit = group
                .packages
                .iter()
                .any(|known| known.id.eq_ignore_ascii_case(&cached.id));

            let owner_matches = cached
                .owner
                .as_deref()
                .zip(publication_owner(config, group))
                .is_some_and(|(actual, expected)| actual.eq_ignore_ascii_case(expected));

            (explicit && (cached.owner.is_none() || owner_matches))
                || (package_matches(group, &cached.id) && owner_matches)
        });

        let owner_failed = selected
            .and_then(|group| publication_owner(config, group))
            .is_some_and(|owner| {
                snapshot.sources.iter().any(|source| {
                    source
                        .source
                        .eq_ignore_ascii_case(&format!("nuget:owner:{owner}"))
                        && source.status == DataStatus::Missing
                })
            });

        let source_key = format!("nuget:{}", cached.id.to_ascii_lowercase());
        // A usable capture can still contain an unusable individual package observation.
        if cache.sources.iter().any(|source| {
            source.source.eq_ignore_ascii_case(&source_key)
                && matches!(source.status, DataStatus::Preview | DataStatus::Missing)
        }) {
            continue;
        }

        let failed = snapshot
            .sources
            .iter()
            .any(|source| source.source == source_key && source.status == DataStatus::Missing);

        if selected.is_none() || cached.version.is_none() || !(failed || owner_failed) {
            continue;
        }

        if snapshot
            .packages
            .iter()
            .any(|package| package.id.eq_ignore_ascii_case(&cached.id) && package.version.is_some())
        {
            continue;
        }

        snapshot
            .packages
            .retain(|package| !package.id.eq_ignore_ascii_case(&cached.id));
        snapshot.packages.push(cached.clone());
        snapshot
            .sources
            .retain(|source| source.source != source_key);
        snapshot.sources.push(SourceStatus {
            source: source_key,
            status: DataStatus::Fallback,
        });

        snapshot.mode = SnapshotMode::Partial;
    }
}

/// Render package Markdown from an already expanded, validated inventory.
pub fn render_package_readme_prepared(config: &Config) -> String {
    let mut groups = config
        .publications
        .iter()
        .filter(|group| group.show_in_readme)
        .collect::<Vec<_>>();

    groups.sort_by(|left, right| {
        left.anchor[1]
            .total_cmp(&right.anchor[1])
            .then(left.anchor[0].total_cmp(&right.anchor[0]))
    });
    let mut output = String::from("<!-- sourcefield:packages:start -->\n### NuGet\n");
    for group in groups {
        let repository = config
            .projects
            .iter()
            .find(|project| project.id == group.id)
            .and_then(|project| project.repository.as_deref());

        output.push_str(&format!(
            "\n#### [{}](https://github.com/{})\n\n",
            markdown_text(&group.label),
            repository.unwrap_or(&config.profile.organization)
        ));
        let mut packages = group.packages.iter().collect::<Vec<_>>();
        packages.sort_by(|left, right| {
            left.anchor
                .map(|anchor| anchor[1])
                .unwrap_or(0.0)
                .total_cmp(&right.anchor.map(|anchor| anchor[1]).unwrap_or(0.0))
                .then(left.id.cmp(&right.id))
        });

        for package in packages {
            output.push_str(&format!(
                "- [{}]({})\n",
                markdown_text(&package.id),
                markdown_destination(&package.url)
            ));
        }
    }

    output.push_str("<!-- sourcefield:packages:end -->");
    output
}

/// Render approved projects from the same composed inventory used by the visual field.
pub fn render_project_readme(config: &Config) -> String {
    let mut output = String::from("<!-- sourcefield:projects:start -->\n### Projects\n");

    for domain in &config.domains {
        let projects = config
            .projects
            .iter()
            .filter(|project| project.domain == domain.id && project.show_in_readme)
            .collect::<Vec<_>>();

        if projects.is_empty() {
            continue;
        }

        output.push_str(&format!("\n#### {}\n\n", markdown_text(&domain.label)));

        for project in projects {
            let label = markdown_text(&project.label);
            let summary = markdown_text(&project.summary);

            if let Some(repository) = &project.repository {
                output.push_str(&format!(
                    "- [{label}](https://github.com/{repository}): {summary}\n"
                ));
            } else {
                output.push_str(&format!("- {label}: {summary}\n"));
            }
        }
    }

    output.push_str("<!-- sourcefield:projects:end -->");

    output
}

/// Escape authored prose so approved text cannot inject HTML or Markdown links.
fn markdown_text(value: &str) -> std::borrow::Cow<'_, str> {
    if !value.bytes().any(|byte| b"&<>\\[]*_`\n\r".contains(&byte)) {
        return std::borrow::Cow::Borrowed(value);
    }

    // One traversal avoids the previous chain of full-string replacement allocations.
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\\' | '[' | ']' | '*' | '_' | '`' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '\n' | '\r' => escaped.push(' '),
            _ => escaped.push(character),
        }
    }

    std::borrow::Cow::Owned(escaped)
}

/// Match case-insensitive NuGet roots at an exact ID or dot-delimited descendant boundary.
pub(crate) fn package_id_has_root(id: &str, root: &str) -> bool {
    id.eq_ignore_ascii_case(root)
        || (id
            .get(..root.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(root))
            && id.as_bytes().get(root.len()) == Some(&b'.'))
}

/// Accept Sourcefield's NuGet ID alphabet and NuGet.org's 100-character upper bound.
/// Identifiers are case-insensitive; Unicode labels belong in display text, not registry IDs.
pub fn valid_package_id(id: &str) -> bool {
    // NuGet's restricted ID grammar is ASCII word segments separated by one dot or dash.
    !id.is_empty()
        && id.len() <= 100
        && id.split(['.', '-']).all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// Encode Markdown destination delimiters even when callers bypass typed validation.
fn markdown_destination(value: &str) -> std::borrow::Cow<'_, str> {
    if !value.bytes().any(|byte| b"()[]<>\\ \n\r\t".contains(&byte)) {
        return std::borrow::Cow::Borrowed(value);
    }

    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '(' => escaped.push_str("%28"),
            ')' => escaped.push_str("%29"),
            '[' => escaped.push_str("%5B"),
            ']' => escaped.push_str("%5D"),
            '<' => escaped.push_str("%3C"),
            '>' => escaped.push_str("%3E"),
            '\\' => escaped.push_str("%5C"),
            ' ' => escaped.push_str("%20"),
            '\n' => escaped.push_str("%0A"),
            '\r' => escaped.push_str("%0D"),
            '\t' => escaped.push_str("%09"),
            _ => escaped.push(character),
        }
    }

    std::borrow::Cow::Owned(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PackageSnapshot, load_config};

    fn config() -> Config {
        let mut config = load_config(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/profile.toml"
        ))
        .unwrap();

        config.collection.nuget_owner = Some("sample-labs".into());
        config
            .publications
            .iter_mut()
            .find(|group| group.id == "safe-migrations")
            .unwrap()
            .discovery_prefixes = vec!["Sample.EntityFrameworkCore.SchemaTools".into()];
        config
    }

    fn package(id: &str, owner: &str) -> PackageSnapshot {
        PackageSnapshot {
            id: id.into(),
            owner: Some(owner.into()),
            version: Some("10.0.0-rc.1".into()),
            total_downloads: Some(2),
            ..PackageSnapshot::default()
        }
    }

    #[test]
    fn future_owned_adapter_expands_inventory_links_and_height() {
        let config = config();
        let package = package(
            "Sample.EntityFrameworkCore.SchemaTools.SqlServer",
            "sample-labs",
        );

        let snapshot = Snapshot {
            packages: vec![package],
            ..Snapshot::default()
        };

        let expanded = expanded_config(&config, &snapshot);
        let readme = render_package_readme_prepared(&expanded);

        assert_eq!(expanded.render.height, config.render.height + 90);
        assert!(readme.contains("[Sample.EntityFrameworkCore.SchemaTools.SqlServer](https://www.nuget.org/packages/Sample.EntityFrameworkCore.SchemaTools.SqlServer/)"));
        assert!(!readme.contains("10.0.0-rc.1"));
    }

    #[test]
    fn lookalikes_wrong_owners_and_unpublished_packages_do_not_expand() {
        let config = config();
        let mut unpublished = package(
            "Sample.EntityFrameworkCore.SchemaTools.Unpublished",
            "sample-labs",
        );

        unpublished.version = None;
        let snapshot = Snapshot {
            packages: vec![
                package(
                    "Sample.EntityFrameworkCore.SchemaToolsEvil.SqlServer",
                    "sample-labs",
                ),
                package(
                    "Sample.EntityFrameworkCore.SchemaTools.SqlServer",
                    "attacker",
                ),
                unpublished,
            ],
            ..Snapshot::default()
        };

        let expanded = expanded_config(&config, &snapshot);

        assert_eq!(expanded.render.height, config.render.height);
        assert_eq!(
            expanded
                .publications
                .iter()
                .map(|group| group.packages.len())
                .sum::<usize>(),
            config
                .publications
                .iter()
                .map(|group| group.packages.len())
                .sum::<usize>()
        );
    }

    #[test]
    fn failed_discovery_restores_known_owner_cache_with_fallback_provenance() {
        let config = config();
        let cache = Snapshot {
            mode: SnapshotMode::Live,
            fetched_at: "2026-10-01T00:00:00Z".into(),
            packages: vec![
                package(
                    "Sample.EntityFrameworkCore.SchemaTools.SqlServer",
                    "sample-labs",
                ),
                package("Sample.EntityFrameworkCore.SchemaTools.Evil", "attacker"),
            ],
            ..Snapshot::default()
        };

        let mut snapshot = Snapshot {
            sources: vec![SourceStatus {
                source: "nuget:owner:sample-labs".into(),
                status: DataStatus::Missing,
            }],
            mode: SnapshotMode::Partial,
            ..Snapshot::default()
        };

        restore_package_cache(&mut snapshot, &cache, &config);

        assert_eq!(snapshot.packages.len(), 1);
        assert_eq!(snapshot.mode, SnapshotMode::Partial);
        assert!(
            snapshot
                .sources
                .iter()
                .any(|source| source.status == DataStatus::Fallback)
        );
        assert!(
            snapshot
                .sources
                .iter()
                .any(|source| source.status == DataStatus::Missing)
        );
    }

    #[test]
    fn successful_empty_discovery_does_not_resurrect_cached_package() {
        let config = config();
        let cache = Snapshot {
            mode: SnapshotMode::Live,
            fetched_at: "2026-10-01T00:00:00Z".into(),
            packages: vec![package(
                "Sample.EntityFrameworkCore.SchemaTools.SqlServer",
                "sample-labs",
            )],
            ..Snapshot::default()
        };

        let mut snapshot = Snapshot::default();

        restore_package_cache(&mut snapshot, &cache, &config);

        assert!(snapshot.packages.is_empty());
    }
}
