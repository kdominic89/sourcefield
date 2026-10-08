//! Opt-in repository captions resolved once at the public ownership boundary.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::{Config, DomainConfig, Snapshot, ValidationError, Visibility};

/// Inventory used for an ownership-domain repository caption.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepositoryCaptionSource {
    /// Count authored projects in this domain that participate in the README field.
    #[default]
    SelectedProjects,
    /// Use available repository counts observed for this domain's configured GitHub owner.
    OwnerRepositories,
}

/// Bounded plain-text labels and count source for an ownership-domain caption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryCaptionConfig {
    /// Count selected authored projects by default, or the configured owner's observed inventory.
    #[serde(default)]
    pub source: RepositoryCaptionSource,
    /// Label following an available public count; defaults to public.
    #[serde(default = "default_public_label")]
    pub public_label: String,
    /// Label following an available private count; defaults to private.
    #[serde(default = "default_private_label")]
    pub private_label: String,
    /// Optional final noun, defaulting to repos; an empty string omits the suffix.
    #[serde(default = "default_suffix")]
    pub suffix: String,
    /// Complete caption when neither count is available; defaults to repos unavailable.
    #[serde(default = "default_unavailable")]
    pub unavailable: String,
}

impl Default for RepositoryCaptionConfig {
    fn default() -> Self {
        Self {
            source: RepositoryCaptionSource::default(),
            public_label: default_public_label(),
            private_label: default_private_label(),
            suffix: default_suffix(),
            unavailable: default_unavailable(),
        }
    }
}

fn default_public_label() -> String {
    "public".into()
}

fn default_private_label() -> String {
    "private".into()
}

fn default_suffix() -> String {
    "repos".into()
}

fn default_unavailable() -> String {
    "repos unavailable".into()
}

/// Admit bounded single-line labels before any caption reaches a public projection.
pub(crate) fn validate_config(config: &RepositoryCaptionConfig) -> Result<(), ValidationError> {
    validate_text(&config.public_label, "public_label", 24, false)?;
    validate_text(&config.private_label, "private_label", 24, false)?;
    validate_text(&config.suffix, "suffix", 24, true)?;
    validate_text(&config.unavailable, "unavailable", 64, false)?;

    Ok(())
}

/// Admit standalone state captions independently of their originating configuration.
pub(crate) fn validate_materialized(caption: &str) -> Result<(), ValidationError> {
    validate_text(caption, "materialized", 128, false)
}

/// Check Unicode scalar limits without allocating trimmed or normalized copies.
fn validate_text(
    text: &str,
    field: &str,
    maximum: usize,
    allow_empty: bool,
) -> Result<(), ValidationError> {
    if (!allow_empty && text.is_empty())
        || text.trim() != text
        || text.chars().take(maximum + 1).count() > maximum
        || text.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}')
        })
    {
        let requirement = if allow_empty {
            "trimmed"
        } else {
            "nonempty, trimmed"
        };

        return Err(ValidationError::InvalidValue(format!(
            "repository caption {field} must be {requirement} single-line text \
             of at most {maximum} characters"
        )));
    }

    Ok(())
}

/// Resolve authored ownership and already publication-scoped observations without a template pass.
pub(crate) fn resolve(
    config: &Config,
    domain: &DomainConfig,
    snapshot: &Snapshot,
) -> Option<String> {
    let caption = domain.repository_caption.as_ref()?;
    let (public, private) = match caption.source {
        RepositoryCaptionSource::SelectedProjects => selected_counts(config, domain),
        RepositoryCaptionSource::OwnerRepositories => owner_counts(config, domain, snapshot),
    };

    if public.is_none() && private.is_none() {
        return Some(caption.unavailable.clone());
    }

    // Decimal u32 counts need at most ten bytes each. Reserve the bounded labels once so
    // composing both segments and their suffix does not grow or duplicate the output buffer.
    let mut text = String::with_capacity(
        26 + caption.public_label.len() + caption.private_label.len() + caption.suffix.len(),
    );

    if let Some(public) = public {
        let _ = write!(text, "{public} {}", caption.public_label);
    }

    if let Some(private) = private {
        if public.is_some() {
            text.push_str(" / ");
        }

        let _ = write!(text, "{private} {}", caption.private_label);
    }

    if !caption.suffix.is_empty() {
        text.push(' ');
        text.push_str(&caption.suffix);
    }

    Some(text)
}

/// Count only approved projects; optional discovery satellites are a different inventory.
fn selected_counts(config: &Config, domain: &DomainConfig) -> (Option<u32>, Option<u32>) {
    let (public, private) = config
        .projects
        .iter()
        .filter(|project| project.domain == domain.id && project.show_in_readme)
        .fold((0, 0), |(public, private), project| {
            match project.visibility {
                Visibility::Public => (public + 1, private),
                Visibility::PrivateAbstract => (public, private + 1),
            }
        });

    (Some(public), Some(private))
}

/// Admit a caption observation only when every matching source status is complete.
fn exact_source_available(snapshot: &Snapshot, key: &str) -> bool {
    // Conflicting duplicates cannot hide an explicit failure behind the first healthy record.
    // Legacy statistics retain their existing first-match policy outside this opt-in caption.
    snapshot
        .sources
        .iter()
        .filter(|status| status.source.eq_ignore_ascii_case(key))
        .all(|status| crate::graph::source_is_complete(status.status))
}

/// Borrow current and legacy owner-source keys already retained by snapshot scoping.
fn organization_source_available(snapshot: &Snapshot, owner: &str) -> bool {
    // Scoping admits both organization key spellings. Reading only the current spelling would
    // treat an explicit legacy failure as an absent status and expose its stale account count.
    snapshot
        .sources
        .iter()
        .filter(|status| {
            status
                .source
                .strip_prefix("github:org:")
                .or_else(|| status.source.strip_prefix("github:organization:"))
                .is_some_and(|source_owner| source_owner.eq_ignore_ascii_case(owner))
        })
        .all(|status| crate::graph::source_is_complete(status.status))
}

/// Keep each owner independent; a personal aggregate never describes another ownership domain.
fn owner_counts(
    config: &Config,
    domain: &DomainConfig,
    snapshot: &Snapshot,
) -> (Option<u32>, Option<u32>) {
    if domain.kind == "organization" {
        let public = snapshot
            .organizations
            .iter()
            .find(|account| account.login.eq_ignore_ascii_case(&domain.owner))
            .filter(|_| organization_source_available(snapshot, &domain.owner))
            .map(|account| account.public_repositories);

        return (public, None);
    }

    let primary_owner = config.collection.github_user.as_str();

    if !domain.owner.eq_ignore_ascii_case(primary_owner)
        || (!snapshot.user.login.is_empty()
            && !snapshot.user.login.eq_ignore_ascii_case(primary_owner))
    {
        return (None, None);
    }

    let public = (snapshot.user.login.eq_ignore_ascii_case(primary_owner)
        && exact_source_available(snapshot, "github:user"))
    .then_some(snapshot.user.public_repositories);

    // Publication authorization was resolved before scoping this snapshot. Keep its private
    // observation independent of public-account failure, but never expose a known partial count.
    let private = snapshot
        .private_repository_count
        .filter(|_| exact_source_available(snapshot, "github:private-count"));

    (public, private)
}

#[cfg(test)]
mod tests;
