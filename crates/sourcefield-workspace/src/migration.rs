//! Explicit, one-time conversion of inventoried legacy consumer files.
//!
//! Conversion writes an independent directory and preserves every original file in
//! `recovery/`. Unknown semantic fields survive through value-level transformation.
//! Normal runtime readers do not depend on this module.

use crate::{
    checked_workspace_root, copy_digest, create_parent, file_sha256, relative_key, safe_path,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sourcefield_io::read_utf8_bounded;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;

/// Consumer variant selected explicitly for legacy records that do not encode it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    /// A person's profile with personal and organization domains.
    Personal,
    /// An organization profile with optional maintainer attribution.
    Organization,
}

impl ProfileKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Organization => "organization",
        }
    }
}

/// Provenance and semantic mapping for one independently copied input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigratedFile {
    /// Source-relative file path, preserved in output and recovery trees.
    pub path: String,
    /// Original bytes, including any uncommitted edits.
    pub source_sha256: String,
    /// Converted output bytes or the unchanged original digest.
    pub output_sha256: String,
    /// Explicit field mapping applied to this document.
    pub mappings: Vec<String>,
}

/// Deterministic migration inventory; execution timestamps are deliberately excluded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    /// Migration report format, independently versioned from state schema.
    pub schema_version: u32,
    /// Explicit legacy consumer variant used where historical records lack one.
    pub variant: ProfileKind,
    /// Sorted full inventory of copied files and their transformations.
    pub files: Vec<MigratedFile>,
}

/// Convert selected files/directories into a new independent `output/` and `recovery/` tree.
///
/// Destination must not exist or lie within the source. Paths are explicit so callers
/// decide which runtime/config/history files form the complete matching recovery set.
/// Failures retain an incomplete destination for inspection, never change the source,
/// and never write a success report. Symlinks and overlapping selections are rejected.
pub fn migrate_files(
    source_root: impl AsRef<Path>,
    destination: impl AsRef<Path>,
    relative_paths: &[PathBuf],
    variant: ProfileKind,
) -> Result<MigrationReport> {
    let source = checked_workspace_root(source_root.as_ref())?;
    let destination = destination.as_ref();
    let parent = checked_workspace_root(
        destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
    )?;

    let name = destination
        .file_name()
        .context("migration destination needs a directory name")?;

    let destination = parent.join(name);

    ensure!(
        !destination.starts_with(&source),
        "migration destination must be outside source"
    );
    ensure!(
        !destination.exists(),
        "migration destination already exists"
    );
    ensure!(
        !relative_paths.is_empty(),
        "migration requires an explicit file inventory"
    );

    let mut paths = BTreeSet::new();

    for relative in relative_paths {
        let key = relative_key(relative)?;

        inventory(&source, &key, &mut paths)?;
    }

    fs::create_dir(&destination)?;
    fs::create_dir(destination.join("output"))?;
    fs::create_dir(destination.join("recovery"))?;

    let mut report = MigrationReport {
        schema_version: 1,
        variant,
        files: Vec::with_capacity(paths.len()),
    };

    for key in paths {
        let original = safe_path(&source, &key)?;
        let recovery = safe_path(&destination.join("recovery"), &key)?;
        let target = safe_path(&destination.join("output"), &key)?;

        create_parent(&recovery)?;
        create_parent(&target)?;

        let source_sha256 = copy_digest(File::open(&original)?, &recovery)?;
        let extension = original.extension().and_then(|value| value.to_str());
        let (output_sha256, mappings) = match extension {
            Some("json") => migrate_json_file(&recovery, &target, variant)?,
            Some("toml") => migrate_toml_file(&recovery, &target, variant)?,
            Some("md") if original.file_name().is_some_and(|name| name == "README.md") => {
                migrate_readme_file(&recovery, &target)?
            }
            _ => (copy_digest(File::open(&recovery)?, &target)?, Vec::new()),
        };

        ensure!(
            file_sha256(&original)? == source_sha256,
            "source changed during migration: {key}"
        );
        report.files.push(MigratedFile {
            path: key,
            source_sha256,
            output_sha256,
            mappings,
        });
    }

    validate_history_references(&destination.join("output"), &report)?;

    // Success is recorded last; a directory without this report is not a publishable migration.
    let bytes = serde_json::to_vec_pretty(&report)?;

    copy_digest(bytes.as_slice(), &destination.join("migration-report.json"))?;

    Ok(report)
}

fn inventory(root: &Path, key: &str, files: &mut BTreeSet<String>) -> Result<()> {
    let path = safe_path(root, key)?;
    let metadata = fs::symlink_metadata(&path)?;

    ensure!(
        !metadata.file_type().is_symlink(),
        "migration rejects symlink: {key}"
    );

    if metadata.is_file() {
        ensure!(
            files.insert(key.to_owned()),
            "overlapping migration inventory: {key}"
        );
    } else if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().context("non-UTF-8 migration path")?;
            let child = format!("{key}/{name}");

            relative_key(Path::new(&child))?;
            inventory(root, &child, files)?;
        }
    } else {
        bail!("migration input is not a regular file or directory: {key}");
    }

    Ok(())
}

fn read_document(path: &Path) -> Result<String> {
    ensure!(
        fs::metadata(path)?.len() <= MAX_DOCUMENT_BYTES,
        "migration document exceeds 32 MiB limit: {}",
        path.display()
    );

    read_utf8_bounded(File::open(path)?, MAX_DOCUMENT_BYTES)
        .context("read bounded migration document")
}

/// Add ownership markers only around the exact legacy table shape, preserving original bytes.
fn migrate_readme_file(source: &Path, target: &Path) -> Result<(String, Vec<String>)> {
    const PROJECT_START: &str = "<!-- sourcefield:projects:start -->";
    const PROJECT_END: &str = "<!-- sourcefield:projects:end -->";
    const PACKAGE_START: &str = "<!-- sourcefield:packages:start -->";
    const PACKAGE_END: &str = "<!-- sourcefield:packages:end -->";
    const HEADING: &str = "### Projects";

    let text = read_document(source)?;

    if !text.contains("<!-- sourcefield:projects:")
        && !text.contains("<!-- sourcefield:packages:")
        && !text.lines().any(|line| line == HEADING)
    {
        return Ok((copy_digest(File::open(source)?, target)?, Vec::new()));
    }

    let project_start = unique_standalone_line(&text, PROJECT_START)?;
    let project_end = unique_standalone_line(&text, PROJECT_END)?;
    let package_start = unique_standalone_line(&text, PACKAGE_START)?
        .context("legacy README requires one packages start marker")?;

    let package_end = unique_standalone_line(&text, PACKAGE_END)?
        .context("legacy README requires one packages end marker")?;

    ensure!(
        package_start < package_end,
        "README package markers are reversed"
    );

    if project_start.is_some() || project_end.is_some() {
        let start = project_start.context("README has an unmatched projects end marker")?;
        let end = project_end.context("README has an unmatched projects start marker")?;

        ensure!(
            start < end && end < package_start,
            "README project markers are reversed or overlap packages"
        );

        return Ok((copy_digest(File::open(source)?, target)?, Vec::new()));
    }

    let heading = unique_standalone_line(&text, HEADING)?
        .context("legacy README requires one exact Projects heading")?;

    ensure!(
        heading < package_start,
        "legacy Projects heading follows package section"
    );

    let after_heading = heading + HEADING.len();
    let newline = if text[after_heading..].starts_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let table_start = after_heading + newline.len();

    ensure!(
        table_start <= package_start,
        "legacy Projects table is missing"
    );
    validate_legacy_project_table(&text[table_start..package_start])?;

    // Insertion rather than reserialization retains all authored and table bytes exactly.
    let mut output = String::with_capacity(
        text.len() + PROJECT_START.len() + PROJECT_END.len() + 2 * newline.len(),
    );

    output.push_str(&text[..heading]);
    output.push_str(PROJECT_START);
    output.push_str(newline);
    output.push_str(&text[heading..package_start]);
    output.push_str(PROJECT_END);
    output.push_str(newline);
    output.push_str(&text[package_start..]);

    Ok((copy_digest(output.as_bytes(), target)?, vec!["README legacy Projects table wrapped in sourcefield:projects markers; original bytes preserved".to_owned()]))
}

/// Reject duplicate and inline delimiters rather than guessing which section they delimit.
fn unique_standalone_line(text: &str, needle: &str) -> Result<Option<usize>> {
    let mut matches = text.match_indices(needle);
    let Some((position, _)) = matches.next() else {
        return Ok(None);
    };

    ensure!(
        matches.next().is_none(),
        "duplicate README heading or marker: {needle}"
    );

    let suffix = &text[position + needle.len()..];

    ensure!(
        (position == 0 || text.as_bytes()[position - 1] == b'\n')
            && (suffix.is_empty() || suffix.starts_with('\n') || suffix.starts_with("\r\n")),
        "README heading or marker is not a standalone exact line: {needle}"
    );

    Ok(Some(position))
}

/// Recognize the historical three-column table without absorbing neighboring authored prose.
fn validate_legacy_project_table(section: &str) -> Result<()> {
    let mut rows = 0;
    let mut finished = false;

    for line in section.lines() {
        if line.trim().is_empty() {
            if rows > 0 {
                finished = true;
            }

            continue;
        }

        ensure!(
            !finished,
            "legacy Projects table contains intervening prose or disjoint rows"
        );

        let row = line.trim_end();

        match rows {
            0 => ensure!(
                row == "| Project | What it does | Stack |",
                "unsupported legacy Projects table header"
            ),
            1 => ensure!(
                row == "| --- | --- | --- |",
                "unsupported legacy Projects table separator"
            ),
            _ => ensure!(
                row.starts_with('|')
                    && row.ends_with('|')
                    && row.bytes().filter(|byte| *byte == b'|').count() == 4,
                "legacy Projects section must contain only three-column table rows"
            ),
        }

        rows += 1;
    }

    ensure!(
        rows >= 3,
        "legacy Projects table needs a header, separator and project row"
    );

    Ok(())
}

fn migrate_json_file(
    source: &Path,
    target: &Path,
    variant: ProfileKind,
) -> Result<(String, Vec<String>)> {
    let text = read_document(source)?;
    let mut document: Value = serde_json::from_str(&text).context("invalid migration JSON")?;

    drop(text);
    let mappings = if document.get("states").is_some() {
        convert_index(&mut document)?
    } else if document.get("repositories").is_some() && document.get("user").is_some() {
        convert_snapshot(&mut document)?
    } else {
        convert_state(&mut document, variant)?
    };

    if document.get("profile").is_some() && document.get("nodes").is_some() {
        let state: sourcefield_core::ProfileState = sourcefield_core::ProfileState::deserialize(
            &document,
        )
        .context(
            "converted state is not understood by the current runtime; no fields were discarded",
        )?;

        sourcefield_core::validate_state(&state)
            .context("converted state failed runtime validation")?;
    }

    if mappings.is_empty() {
        return Ok((copy_digest(File::open(source)?, target)?, mappings));
    }

    let bytes = serde_json::to_vec_pretty(&document)?;

    Ok((copy_digest(bytes.as_slice(), target)?, mappings))
}

fn convert_snapshot(document: &mut Value) -> Result<Vec<String>> {
    let object = document
        .as_object_mut()
        .context("snapshot must be an object")?;

    if let Some(version) = object.get("schema_version") {
        ensure!(version.as_u64() == Some(1), "unsupported snapshot schema");

        return Ok(Vec::new());
    }

    object.insert("schema_version".to_owned(), Value::from(1));

    Ok(vec![
        "observed snapshot schema_version <- 1; observations unchanged".to_owned(),
    ])
}

fn convert_index(document: &mut Value) -> Result<Vec<String>> {
    let object = document
        .as_object_mut()
        .context("history index must be an object")?;

    ensure!(
        object.get("states").is_some_and(Value::is_array),
        "history states must be an array"
    );

    if let Some(version) = object.get("schema_version") {
        ensure!(
            version.as_u64() == Some(3),
            "unsupported history index schema"
        );

        return Ok(Vec::new());
    }

    object.insert("schema_version".to_owned(), Value::from(3));

    Ok(vec![
        "history index schema_version <- 3; state references unchanged".to_owned(),
    ])
}

/// Convert a legacy schema-2 state value in memory without dropping unknown fields.
/// Non-state documents pass unchanged. Existing schema-3 states are validated and preserved.
pub fn convert_state(document: &mut Value, variant: ProfileKind) -> Result<Vec<String>> {
    let Some(object) = document.as_object_mut() else {
        return Ok(Vec::new());
    };

    if !object.contains_key("nodes")
        && !object.contains_key("schema")
        && !object.contains_key("schema_version")
    {
        return Ok(Vec::new());
    }

    // Versioned non-state metadata is not interpreted as a historical graph document.
    if !object.contains_key("profile") && !object.contains_key("nodes") {
        return Ok(Vec::new());
    }

    if let Some(version) = object.get("schema_version") {
        ensure!(
            version.as_u64() == Some(3) && !object.contains_key("schema"),
            "unsupported or ambiguous state schema"
        );
        ensure!(
            object
                .get("profile")
                .and_then(|profile| profile.get("variant"))
                .and_then(Value::as_str)
                == Some(variant.as_str()),
            "migrated profile variant mismatch"
        );

        return Ok(Vec::new());
    }

    ensure!(
        object.get("schema").and_then(Value::as_u64) == Some(2),
        "legacy state must use schema 2"
    );
    ensure!(
        object.get("nodes").is_some_and(Value::is_array)
            && object.get("edges").is_some_and(Value::is_array),
        "legacy graph needs nodes and edges arrays"
    );

    let profile = object
        .get_mut("profile")
        .and_then(Value::as_object_mut)
        .context("legacy state needs a profile object")?;

    let mut mappings = vec!["schema:2 -> schema_version:3".to_owned()];

    convert_profile(profile, variant, &mut mappings)?;
    object.remove("schema");
    object.insert("schema_version".to_owned(), Value::from(3));
    migrate_organization_identities(object, &mut mappings)?;

    Ok(mappings)
}

fn migrate_organization_identities(
    object: &mut serde_json::Map<String, Value>,
    mappings: &mut Vec<String>,
) -> Result<()> {
    if object.contains_key("organizations") {
        return Ok(());
    }

    let mut organizations = Vec::new();
    let nodes = object
        .get("nodes")
        .and_then(Value::as_array)
        .context("missing historical nodes")?;

    for node in nodes {
        if node.get("kind").and_then(Value::as_str) != Some("domain")
            || node.get("scope").and_then(Value::as_str) != Some("organization")
        {
            continue;
        }

        let id = node
            .get("domain")
            .and_then(Value::as_str)
            .context("organization node needs domain")?;

        let label = node
            .get("label")
            .and_then(Value::as_str)
            .context("organization node needs label")?;

        let url = node
            .get("url")
            .and_then(Value::as_str)
            .context("organization node needs recorded account URL")?;

        let owner = url
            .strip_prefix("https://github.com/")
            .context("unsupported historical organization URL")?
            .trim_end_matches('/');

        ensure!(
            !owner.is_empty()
                && owner.len() <= 39
                && owner
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "invalid historical organization account"
        );

        let profile = &object["profile"];
        let maintainer = if profile.get("organization").and_then(Value::as_str) == Some(owner) {
            profile.get("maintainer").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        };

        organizations.push(
            serde_json::json!({"id": id, "owner": owner, "label": label, "maintainer": maintainer}),
        );
    }

    object.insert("organizations".to_owned(), Value::Array(organizations));
    mappings.push(
        "organizations <- recorded organization domain nodes and historical maintainer".to_owned(),
    );

    Ok(())
}

fn convert_profile(
    profile: &mut serde_json::Map<String, Value>,
    variant: ProfileKind,
    mappings: &mut Vec<String>,
) -> Result<()> {
    if let Some(existing) = profile.get("variant") {
        ensure!(
            existing.as_str() == Some(variant.as_str()),
            "profile variant conflicts with migration selection"
        );
    } else {
        profile.insert("variant".to_owned(), Value::from(variant.as_str()));
        mappings.push(format!(
            "profile.variant <- explicit {} selection",
            variant.as_str()
        ));
    }

    let username = profile.get("maintainer_username");
    let role = profile.get("maintainer_role");

    if username.is_some() || role.is_some() {
        ensure!(
            variant == ProfileKind::Organization,
            "personal legacy state has organization maintainer fields"
        );
        ensure!(
            !profile.contains_key("maintainer"),
            "ambiguous legacy/current maintainer fields"
        );

        let username = username
            .and_then(Value::as_str)
            .context("maintainer username must be a string")?;

        let role = role
            .and_then(Value::as_str)
            .context("maintainer role must be a string")?;

        ensure!(
            !username.is_empty()
                && username.len() <= 39
                && username
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "invalid maintainer handle"
        );

        let maintainer = serde_json::json!({ "username": username, "role": role, "url": format!("https://github.com/{username}") });

        profile.remove("maintainer_username");
        profile.remove("maintainer_role");
        profile.insert("maintainer".to_owned(), maintainer);
        mappings.push("profile.maintainer_username/maintainer_role -> profile.maintainer (URL derived from recorded handle)".to_owned());
    }

    Ok(())
}

fn migrate_toml_file(
    source: &Path,
    target: &Path,
    variant: ProfileKind,
) -> Result<(String, Vec<String>)> {
    let text = read_document(source)?;
    let mut document: toml::Value = toml::from_str(&text).context("invalid migration TOML")?;

    if document.get("profile").is_none() {
        return Ok((copy_digest(File::open(source)?, target)?, Vec::new()));
    }

    let table = document
        .as_table_mut()
        .context("profile TOML must be a table")?;

    if let Some(version) = table.get("schema_version") {
        ensure!(
            version.as_integer() == Some(1) && !table.contains_key("version"),
            "unsupported or ambiguous configuration schema"
        );
        ensure!(
            table
                .get("profile")
                .and_then(|profile| profile.get("variant"))
                .and_then(toml::Value::as_str)
                == Some(variant.as_str()),
            "migrated configuration variant mismatch"
        );

        return Ok((copy_digest(File::open(source)?, target)?, Vec::new()));
    }

    ensure!(
        table.get("version").and_then(toml::Value::as_integer) == Some(1),
        "legacy configuration must use version 1"
    );

    let mut profile: serde_json::Map<String, Value> = serde_json::to_value(table.get("profile"))
        .context("read legacy profile")?
        .as_object()
        .cloned()
        .context("profile must be a table")?;

    let mut mappings = vec!["version:1 -> schema_version:1".to_owned()];

    convert_profile(&mut profile, variant, &mut mappings)?;
    table.insert("profile".to_owned(), toml::Value::try_from(&profile)?);
    table.remove("version");
    table.insert("schema_version".to_owned(), toml::Value::Integer(1));
    migrate_publication_labels(table, &mut mappings)?;

    let output = toml::to_string_pretty(&document)?;

    Ok((copy_digest(output.as_bytes(), target)?, mappings))
}

fn migrate_publication_labels(
    table: &mut toml::map::Map<String, toml::Value>,
    mappings: &mut Vec<String>,
) -> Result<()> {
    let Some(publications) = table.get_mut("publications") else {
        return Ok(());
    };

    for publication in publications
        .as_array_mut()
        .context("publications must be an array")?
    {
        let publication = publication
            .as_table_mut()
            .context("publication must be a table")?;

        let label = publication
            .get("label")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();

        let is_safe = label == "Doka.EntityFrameworkCore.SafeMigrations";
        let is_nested = label == "Doka.EntityFrameworkCore.NestedSet";
        let prefix = match label {
            "Doka.EntityFrameworkCore.MySql" => "Doka.EntityFrameworkCore.",
            "Doka.EntityFrameworkCore.SafeMigrations" => "Doka.EntityFrameworkCore.SafeMigrations.",
            "Doka.EntityFrameworkCore.NestedSet" => "Doka.",
            _ => continue,
        };

        if !publication.contains_key("label_prefix") {
            publication.insert(
                "label_prefix".to_owned(),
                toml::Value::String(prefix.to_owned()),
            );
            mappings.push(format!(
                "publication.label_prefix <- legacy rendering rule {prefix}"
            ));
        }

        if is_nested {
            let overrides = publication
                .entry("label_overrides")
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
                .as_table_mut()
                .context("label_overrides must be a table")?;

            if !overrides.contains_key("Doka.NestedSet") {
                overrides.insert(
                    "Doka.NestedSet".to_owned(),
                    toml::Value::String("Doka.NestedSet".to_owned()),
                );
                mappings.push("publication.label_overrides[Doka.NestedSet] <- legacy standalone package label".to_owned());
            }
        }

        if is_safe && !publication.contains_key("core_label") {
            publication.insert(
                "core_label".to_owned(),
                toml::Value::String("Core".to_owned()),
            );
            mappings.push("publication.core_label <- legacy rendering rule Core".to_owned());
        }
    }

    Ok(())
}

fn validate_history_references(root: &Path, report: &MigrationReport) -> Result<()> {
    for entry in &report.files {
        if entry.path != "index.json" && !entry.path.ends_with("/index.json") {
            continue;
        }

        let path = safe_path(root, &entry.path)?;
        let document: Value = serde_json::from_str(&read_document(&path)?)?;
        let Some(states) = document.get("states").and_then(Value::as_array) else {
            continue;
        };

        let mut references = BTreeSet::new();

        for state in states {
            let relative = state
                .get("file")
                .and_then(Value::as_str)
                .context("history entry needs a file reference")?;

            let key = relative_key(Path::new(relative))?;

            ensure!(
                references.insert(key.clone()),
                "duplicate history file reference: {relative}"
            );

            let target = safe_path(path.parent().context("index parent missing")?, &key)?;

            ensure!(
                target.is_file(),
                "history reference is outside selected inventory: {relative}"
            );

            let target: Value = serde_json::from_str(&read_document(&target)?)?;

            ensure!(
                state.get("hash") == target.get("semantic_hash"),
                "history hash reference mismatch: {relative}"
            );
            ensure!(
                state.get("generated_at") == target.get("generated_at"),
                "history timestamp reference mismatch: {relative}"
            );

            ensure!(
                state.get("node_count").and_then(Value::as_u64)
                    == target
                        .get("nodes")
                        .and_then(Value::as_array)
                        .map(|nodes| nodes.len() as u64),
                "history node count mismatch: {relative}"
            );
            ensure!(
                state.get("edge_count").and_then(Value::as_u64)
                    == target
                        .get("edges")
                        .and_then(Value::as_array)
                        .map(|edges| edges.len() as u64),
                "history edge count mismatch: {relative}"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::TestRoot;

    fn legacy(variant: ProfileKind) -> Value {
        let mut value = serde_json::json!({
            "schema": 2,
            "profile": {
                "username": "example", "display_name": "Example", "organization": "example-org",
                "headline": "Example", "tagline": "Example", "pages_url": "https://example.com/",
                "source_url": "https://github.com/example/example"
            },
            "mode": "preview",
            "canvas": { "show_activity_orbit": true, "show_state_hash": false, "show_interests_in_readme": false,
                "show_technology_labels": true, "show_details": false, "width": 1800, "height": 1500, "motion_seconds": 32 },
            "stats": { "package_count": 0 },
            "activity": [], "packages": [], "warnings": [],
            "semantic_hash": "ABC123",
            "generated_at": "2026-01-02T03:04:05Z",
            "nodes": [],
            "edges": [],
            "unknown_extension": { "count": 7, "not_observed": null }
        });

        if variant == ProfileKind::Organization {
            value["profile"]["maintainer_username"] = Value::from("example");
            value["profile"]["maintainer_role"] = Value::from("Core Maintainer");
        }

        value
    }

    #[test]
    fn preserves_personal_semantics_and_unknown_fields() {
        let mut state = legacy(ProfileKind::Personal);
        let before = state.clone();

        let mappings = convert_state(&mut state, ProfileKind::Personal).unwrap();

        assert_eq!(state["schema_version"], 3);
        assert!(state.get("schema").is_none());
        assert_eq!(state["profile"]["variant"], "personal");
        assert_eq!(state["unknown_extension"], before["unknown_extension"]);
        assert_eq!(state["semantic_hash"], before["semantic_hash"]);
        assert_eq!(state["generated_at"], before["generated_at"]);
        assert_eq!(mappings.len(), 3);
    }

    #[test]
    fn maps_recorded_organization_maintainer_without_current_facts() {
        let mut state = legacy(ProfileKind::Organization);

        let mappings = convert_state(&mut state, ProfileKind::Organization).unwrap();

        assert_eq!(state["profile"]["maintainer"]["username"], "example");
        assert_eq!(state["profile"]["maintainer"]["role"], "Core Maintainer");
        assert_eq!(
            state["profile"]["maintainer"]["url"],
            "https://github.com/example"
        );
        assert!(state["profile"].get("maintainer_username").is_none());
        assert_eq!(mappings.len(), 4);
    }

    #[test]
    fn already_migrated_state_is_unchanged() {
        let mut state = legacy(ProfileKind::Personal);
        convert_state(&mut state, ProfileKind::Personal).unwrap();
        let before = state.clone();

        let mappings = convert_state(&mut state, ProfileKind::Personal).unwrap();

        assert!(mappings.is_empty());
        assert_eq!(state, before);
    }

    #[test]
    fn unsupported_state_schema_is_rejected() {
        let mut state = legacy(ProfileKind::Personal);
        state["schema"] = Value::from(99);

        let result = convert_state(&mut state, ProfileKind::Personal);

        assert!(result.is_err());
        assert_eq!(state["schema"], 99);
    }

    #[test]
    fn invalid_maintainer_cannot_create_external_link() {
        let mut state = legacy(ProfileKind::Organization);
        state["profile"]["maintainer_username"] = Value::from("example/../../attacker");

        let result = convert_state(&mut state, ProfileKind::Organization);

        assert!(result.is_err());
        assert_eq!(state["schema"], 2);
    }

    #[test]
    fn migration_repeats_deterministically_preserves_sources_and_runtime() {
        let root = TestRoot::new();
        let destinations = TestRoot::new();
        fs::create_dir(root.0.join("history")).unwrap();
        let mut fixture = legacy(ProfileKind::Personal);
        fixture.as_object_mut().unwrap().remove("unknown_extension");
        let state = serde_json::to_vec_pretty(&fixture).unwrap();
        fs::write(root.0.join("history/ABC123.json"), &state).unwrap();
        fs::write(root.0.join("history/index.json"), br#"{"states":[{"file":"ABC123.json","hash":"ABC123","generated_at":"2026-01-02T03:04:05Z","node_count":0,"edge_count":0}]}"#).unwrap();
        fs::write(root.0.join("app.js"), b"old matching runtime").unwrap();
        let inputs = vec![PathBuf::from("history"), PathBuf::from("app.js")];

        let first = migrate_files(
            &root.0,
            destinations.0.join("first"),
            &inputs,
            ProfileKind::Personal,
        )
        .unwrap();

        let second = migrate_files(
            &root.0,
            destinations.0.join("second"),
            &inputs,
            ProfileKind::Personal,
        )
        .unwrap();

        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(fs::read(root.0.join("history/ABC123.json")).unwrap(), state);
        assert_eq!(
            fs::read(destinations.0.join("first/recovery/history/ABC123.json")).unwrap(),
            state
        );
        assert_eq!(
            fs::read(destinations.0.join("first/recovery/app.js")).unwrap(),
            b"old matching runtime"
        );
        let index: Value = serde_json::from_slice(
            &fs::read(destinations.0.join("first/output/history/index.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(index["schema_version"], 3);
        assert_eq!(index["states"][0]["file"], "ABC123.json");
    }

    #[test]
    fn missing_archive_reference_prevents_success_report() {
        let root = TestRoot::new();
        let destinations = TestRoot::new();
        fs::create_dir(root.0.join("history")).unwrap();
        let index = br#"{"states":[{"file":"missing.json","hash":"ABC123","generated_at":"2026-01-02T03:04:05Z"}]}"#;
        fs::write(root.0.join("history/index.json"), index).unwrap();

        let result = migrate_files(
            &root.0,
            destinations.0.join("failed"),
            &[PathBuf::from("history")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert!(!destinations.0.join("failed/migration-report.json").exists());
        assert_eq!(fs::read(root.0.join("history/index.json")).unwrap(), index);
    }

    #[test]
    fn malformed_document_preserves_source_and_has_no_success_report() {
        let root = TestRoot::new();
        let destinations = TestRoot::new();
        fs::write(root.0.join("state.json"), b"{broken").unwrap();

        let result = migrate_files(
            &root.0,
            destinations.0.join("failed"),
            &[PathBuf::from("state.json")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert!(!destinations.0.join("failed/migration-report.json").exists());
        assert_eq!(fs::read(root.0.join("state.json")).unwrap(), b"{broken");
    }

    #[test]
    fn converts_config_variant_and_preserves_independent_fields() {
        let root = TestRoot::new();
        let destinations = TestRoot::new();
        fs::write(root.0.join("profile.toml"), "version = 1\n[profile]\nusername = 'example'\nmaintainer_username = 'example'\nmaintainer_role = 'Maintainer'\n[custom]\nvalue = 42\n").unwrap();

        migrate_files(
            &root.0,
            destinations.0.join("converted"),
            &[PathBuf::from("profile.toml")],
            ProfileKind::Organization,
        )
        .unwrap();

        let value: toml::Value = toml::from_str(
            &fs::read_to_string(destinations.0.join("converted/output/profile.toml")).unwrap(),
        )
        .unwrap();

        assert_eq!(value["schema_version"].as_integer(), Some(1));
        assert_eq!(value["profile"]["variant"].as_str(), Some("organization"));
        assert_eq!(
            value["profile"]["maintainer"]["username"].as_str(),
            Some("example")
        );
        assert_eq!(value["custom"]["value"].as_integer(), Some(42));
    }

    #[test]
    fn unknown_state_fields_block_runtime_migration_instead_of_disappearing() {
        let root = TestRoot::new();
        let destinations = TestRoot::new();
        let bytes = serde_json::to_vec(&legacy(ProfileKind::Personal)).unwrap();
        fs::write(root.0.join("state.json"), &bytes).unwrap();

        let result = migrate_files(
            &root.0,
            destinations.0.join("failed"),
            &[PathBuf::from("state.json")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert!(!destinations.0.join("failed/migration-report.json").exists());
        assert_eq!(fs::read(root.0.join("state.json")).unwrap(), bytes);
    }

    #[test]
    fn migrated_organization_identity_uses_recorded_domain() {
        let mut state = legacy(ProfileKind::Organization);
        state["profile"]["organization"] = Value::from("old-org");
        state["nodes"] = serde_json::json!([{
            "kind": "domain", "scope": "organization", "domain": "stable-id",
            "label": "Old Organization", "url": "https://github.com/old-org"
        }]);

        convert_state(&mut state, ProfileKind::Organization).unwrap();

        assert_eq!(state["organizations"][0]["id"], "stable-id");
        assert_eq!(state["organizations"][0]["owner"], "old-org");
        assert_eq!(state["organizations"][0]["label"], "Old Organization");
        assert_eq!(
            state["organizations"][0]["maintainer"]["username"],
            "example"
        );
    }

    #[test]
    fn adds_snapshot_version_without_replacing_observations() {
        let mut snapshot = serde_json::json!({"user":{"login":"historical"}, "repositories":[], "fetched_at":"2020-01-01", "packages":[]});
        let before = snapshot.clone();

        let mappings = convert_snapshot(&mut snapshot).unwrap();

        assert_eq!(snapshot["schema_version"], 1);
        assert_eq!(snapshot["user"], before["user"]);
        assert_eq!(snapshot["fetched_at"], before["fetched_at"]);
        assert_eq!(mappings.len(), 1);
    }

    #[test]
    fn rejects_unknown_snapshot_version() {
        let mut snapshot = serde_json::json!({"schema_version":99,"user":{}, "repositories":[]});

        let result = convert_snapshot(&mut snapshot);

        assert!(result.is_err());
        assert_eq!(snapshot["schema_version"], 99);
    }

    fn legacy_readme() -> &'static str {
        "Intro\n<details>\n\n### Projects\n\n| Project | What it does | Stack |\n| --- | --- | --- |\n| Sample | Preserved description | Rust |\n\n<!-- sourcefield:packages:start -->\n### NuGet\n\n- Preserved package\n<!-- sourcefield:packages:end -->\n\n</details>\nFooter\n"
    }

    #[test]
    fn inserts_project_markers_without_changing_any_original_readme_bytes() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let source = legacy_readme();
        fs::write(root.0.join("README.md"), source).unwrap();

        let report = migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        )
        .unwrap();

        let output = fs::read_to_string(destination.0.join("converted/output/README.md")).unwrap();
        assert!(output.contains("<!-- sourcefield:projects:start -->\n### Projects"));
        assert!(
            output
                .contains("<!-- sourcefield:projects:end -->\n<!-- sourcefield:packages:start -->")
        );
        assert_eq!(
            output
                .replace("<!-- sourcefield:projects:start -->\n", "")
                .replace("<!-- sourcefield:projects:end -->\n", ""),
            source
        );
        assert_eq!(
            fs::read_to_string(root.0.join("README.md")).unwrap(),
            source
        );
        assert_eq!(
            fs::read_to_string(destination.0.join("converted/recovery/README.md")).unwrap(),
            source
        );
        assert_eq!(report.files[0].mappings.len(), 1);
    }

    #[test]
    fn readme_project_marker_migration_is_idempotent() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let source = legacy_readme()
            .replace(
                "### Projects",
                "<!-- sourcefield:projects:start -->\n### Projects",
            )
            .replace(
                "<!-- sourcefield:packages:start -->",
                "<!-- sourcefield:projects:end -->\n<!-- sourcefield:packages:start -->",
            );

        fs::write(root.0.join("README.md"), &source).unwrap();

        let report = migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("README.md")],
            ProfileKind::Organization,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.0.join("converted/output/README.md")).unwrap(),
            source
        );
        assert!(report.files[0].mappings.is_empty());
    }

    #[test]
    fn intervening_readme_prose_blocks_legacy_marker_migration() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let source = legacy_readme().replace("<!-- sourcefield:packages:start -->", "Authored prose must not become generated content.\n<!-- sourcefield:packages:start -->");
        fs::write(root.0.join("README.md"), &source).unwrap();

        let result = migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(root.0.join("README.md")).unwrap(),
            source
        );
        assert!(
            !destination
                .0
                .join("converted/migration-report.json")
                .exists()
        );
    }

    #[test]
    fn duplicate_project_headings_block_legacy_marker_migration() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let source = format!("### Projects\n{}", legacy_readme());
        fs::write(root.0.join("README.md"), source).unwrap();

        let result = migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert!(
            !destination
                .0
                .join("converted/migration-report.json")
                .exists()
        );
    }

    #[test]
    fn partial_project_markers_block_legacy_marker_migration() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let source = legacy_readme().replace(
            "### Projects",
            "<!-- sourcefield:projects:start -->\n### Projects",
        );

        fs::write(root.0.join("README.md"), source).unwrap();

        let result = migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert!(
            !destination
                .0
                .join("converted/migration-report.json")
                .exists()
        );
    }

    #[test]
    fn readme_marker_migration_preserves_crlf_line_endings() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let source = legacy_readme().replace('\n', "\r\n");
        fs::write(root.0.join("README.md"), &source).unwrap();

        migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        )
        .unwrap();

        let output = fs::read_to_string(destination.0.join("converted/output/README.md")).unwrap();
        assert!(output.contains("<!-- sourcefield:projects:start -->\r\n### Projects"));
        assert_eq!(
            output
                .replace("<!-- sourcefield:projects:start -->\r\n", "")
                .replace("<!-- sourcefield:projects:end -->\r\n", ""),
            source
        );
    }

    #[test]
    fn relative_migration_destination_is_resolved_from_current_directory() {
        let root = TestRoot::new();
        let relative = PathBuf::from(root.0.file_name().unwrap());
        let destination = TestRoot(std::env::current_dir().unwrap().join(&relative));
        fs::write(root.0.join("README.md"), b"unrelated prose").unwrap();

        let report = migrate_files(
            &root.0,
            &relative,
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        )
        .unwrap();

        assert_eq!(report.files.len(), 1);
        assert_eq!(
            fs::read(destination.0.join("output/README.md")).unwrap(),
            b"unrelated prose"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn migration_accepts_trusted_tmp_destination_alias() {
        let root = TestRoot::new();
        let name = TestRoot::new();
        let destination = TestRoot(Path::new("/private/tmp").join(name.0.file_name().unwrap()));
        fs::create_dir(&destination.0).unwrap();
        let alias = Path::new("/tmp")
            .join(destination.0.file_name().unwrap())
            .join("converted");
        fs::write(root.0.join("README.md"), b"unrelated prose").unwrap();

        migrate_files(
            &root.0,
            &alias,
            &[PathBuf::from("README.md")],
            ProfileKind::Personal,
        )
        .unwrap();

        assert_eq!(
            fs::read(destination.0.join("converted/output/README.md")).unwrap(),
            b"unrelated prose"
        );
    }

    /// Both historic consumers used these exact package groups; migration alone knows their legacy labels.
    fn legacy_package_config(organization: bool) -> String {
        let maintainer = if organization {
            "maintainer_username='example'\nmaintainer_role='Maintainer'\n"
        } else {
            ""
        };

        format!(
            "version=1\n[profile]\nusername='example'\n{maintainer}\n[[publications]]\nid='mysql'\nlabel='Doka.EntityFrameworkCore.MySql'\n[[publications]]\nid='safe-migrations'\nlabel='Doka.EntityFrameworkCore.SafeMigrations'\n[[publications]]\nid='nested-set'\nlabel='Doka.EntityFrameworkCore.NestedSet'\n"
        )
    }

    #[test]
    fn personal_legacy_doka_labels_become_explicit_configuration() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        fs::write(root.0.join("profile.toml"), legacy_package_config(false)).unwrap();

        migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("profile.toml")],
            ProfileKind::Personal,
        )
        .unwrap();

        let output: toml::Value = toml::from_str(
            &fs::read_to_string(destination.0.join("converted/output/profile.toml")).unwrap(),
        )
        .unwrap();
        let groups = output["publications"].as_array().unwrap();
        assert_eq!(
            groups[0]["label_prefix"].as_str(),
            Some("Doka.EntityFrameworkCore.")
        );
        assert!(groups[0].get("core_label").is_none());
        assert_eq!(
            groups[1]["label_prefix"].as_str(),
            Some("Doka.EntityFrameworkCore.SafeMigrations.")
        );
        assert_eq!(groups[1]["core_label"].as_str(), Some("Core"));
        assert_eq!(
            groups[2]["label_overrides"]["Doka.NestedSet"].as_str(),
            Some("Doka.NestedSet")
        );
    }

    #[test]
    fn organization_legacy_labels_and_authored_override_are_preserved() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        let input = format!(
            "{}label_prefix='custom.'\n[publications.label_overrides]\n'Doka.NestedSet'='Authored label'\n",
            legacy_package_config(true)
        );
        fs::write(root.0.join("profile.toml"), input).unwrap();

        migrate_files(
            &root.0,
            destination.0.join("converted"),
            &[PathBuf::from("profile.toml")],
            ProfileKind::Organization,
        )
        .unwrap();

        let output: toml::Value = toml::from_str(
            &fs::read_to_string(destination.0.join("converted/output/profile.toml")).unwrap(),
        )
        .unwrap();
        let groups = output["publications"].as_array().unwrap();
        assert_eq!(
            output["profile"]["maintainer"]["username"].as_str(),
            Some("example")
        );
        assert_eq!(groups[1]["core_label"].as_str(), Some("Core"));
        assert_eq!(groups[2]["label_prefix"].as_str(), Some("custom."));
        assert_eq!(
            groups[2]["label_overrides"]["Doka.NestedSet"].as_str(),
            Some("Authored label")
        );
    }

    #[test]
    fn unrelated_package_labels_are_not_reinterpreted_as_doka() {
        let mut table: toml::Value =
            toml::from_str("[[publications]]\nid='custom'\nlabel='Example.Core'\n").unwrap();
        let before = table.clone();
        let mut mappings = Vec::new();

        migrate_publication_labels(table.as_table_mut().unwrap(), &mut mappings).unwrap();

        assert_eq!(table, before);
        assert!(mappings.is_empty());
    }

    #[test]
    fn refuses_destination_inside_source() {
        let root = TestRoot::new();
        fs::write(root.0.join("state.json"), b"{}").unwrap();

        let result = migrate_files(
            &root.0,
            root.0.join("converted"),
            &[PathBuf::from("state.json")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert!(!root.0.join("converted").exists());
    }

    #[test]
    fn refuses_existing_destination_without_modifying_it() {
        let root = TestRoot::new();
        let destination = TestRoot::new();
        fs::write(root.0.join("state.json"), b"{}").unwrap();
        fs::write(destination.0.join("keep"), b"original").unwrap();

        let result = migrate_files(
            &root.0,
            &destination.0,
            &[PathBuf::from("state.json")],
            ProfileKind::Personal,
        );

        assert!(result.is_err());
        assert_eq!(fs::read(destination.0.join("keep")).unwrap(), b"original");
    }
}
