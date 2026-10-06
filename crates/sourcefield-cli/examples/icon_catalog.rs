//! Render built-in and consumer-authored icons from the documented TOML contract.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use serde::Deserialize;
use sourcefield_core::{Config, IconCatalog, Snapshot, build_state, validate_config};
use sourcefield_render::{PreparedPresentation, Theme};

/// A new destination keeps example runs from replacing previously reviewed artifacts.
#[derive(Parser)]
struct Arguments {
    /// New directory for the validated inputs, state and three SVG variants.
    #[arg(long)]
    output: PathBuf,
}

/// Parse the standalone documentation snippet without introducing another asset loader.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogExample {
    icons: IconCatalog,
}

/// Exercise the documented catalog in an otherwise unchanged synthetic profile.
fn example_config() -> Result<Config> {
    let mut config: Config = toml::from_str(include_str!("../../../config/profile.toml"))?;
    let catalog: CatalogExample = toml::from_str(include_str!("../../../examples/icons.toml"))?;
    let selections = [
        (
            "builtin:sourcefield",
            "Sourcefield",
            "Sourcefield",
            "core",
            47.0,
        ),
        (
            "builtin:database-safe",
            "Database safe",
            "Safe",
            "migrations",
            51.0,
        ),
        ("orbit", "Custom icon", "Custom icon", "lab", 44.0),
    ];

    config.icons = catalog.icons;

    for (project, (icon, label, surface_label, visual, radius)) in
        config.projects.iter_mut().zip(selections)
    {
        project.icon = Some(icon.into());
        project.label = label.into();
        project.surface_label = surface_label.into();
        project.visual = visual.into();
        project.radius = Some(radius);
    }

    validate_config(&config)?;

    Ok(config)
}

/// Produce inspectable artifacts without touching any authored consumer inputs.
fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let config = example_config()?;
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../../../config/offline-snapshot.json"))?;

    let state = build_state(&config, &snapshot, "1970-01-01T00:00:00Z")?;
    let presentation = PreparedPresentation::new(&state);
    let variants = [
        ("icons.dark.svg", Theme::Dark, true),
        ("icons.light.svg", Theme::Light, true),
        ("icons.static.svg", Theme::Dark, false),
    ];

    fs::create_dir(&arguments.output).context("icon preview output must be a new directory")?;
    fs::write(
        arguments.output.join("profile.toml"),
        toml::to_string_pretty(&config)?,
    )?;
    fs::write(
        arguments.output.join("profile-state.json"),
        serde_json::to_vec_pretty(&state)?,
    )?;

    for (name, theme, motion) in variants {
        fs::write(
            arguments.output.join(name),
            presentation.render(theme, motion),
        )?;
    }

    println!("Icon previews: {}", arguments.output.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_catalog_builds_a_valid_shared_state() {
        let config = example_config().unwrap();

        let state = build_state(&config, &Snapshot::default(), "fixed").unwrap();

        assert_eq!(state.icons.len(), 1);
        assert_eq!(
            state
                .nodes
                .iter()
                .filter(|node| node.icon.is_some())
                .count(),
            3
        );
        assert!(sourcefield_core::validate_state(&state).is_ok());
    }

    #[test]
    fn documented_catalog_does_not_allow_arbitrary_svg_attributes() {
        let source = include_str!("../../../examples/icons.toml").replacen(
            "stroke = \"accent\"",
            "onclick = \"alert(1)\"",
            1,
        );

        let result = toml::from_str::<CatalogExample>(&source);

        assert!(result.is_err());
    }
}
