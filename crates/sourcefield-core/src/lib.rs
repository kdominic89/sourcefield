//! Typed public configuration, deterministic graph construction, and validation for SOURCEFIELD.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod config;
mod constants;
mod presentation_layout;
mod xml_text;
pub use constants::*;
pub use presentation_layout::*;
mod graph;
mod model;
mod organizations;
mod privacy;
mod registry;
mod surface_labels;
mod validate;

pub use config::{ConfigError, load_config};
pub use graph::{GraphError, PreparedProfile, build_prepared_state, build_state, prepare_profile};
pub use model::*;
pub use organizations::*;
pub use privacy::scope_snapshot;
pub use registry::{
    expanded_config, package_label, package_matches, publication_owner,
    render_package_readme_prepared, render_project_readme, restore_package_cache,
    usable_observation, valid_package_id,
};
pub use validate::{ValidationError, validate_config, validate_state};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_mode_defaults_to_preview() {
        assert_eq!(Snapshot::default().mode, SnapshotMode::Preview);
    }

    #[test]
    fn private_abstract_serialization_is_stable() {
        let value = serde_json::to_string(&Visibility::PrivateAbstract).unwrap();
        assert_eq!(value, r#""private-abstract""#);
    }
}

#[cfg(test)]
mod contract_tests;

#[cfg(test)]
mod organization_tests;

#[cfg(test)]
mod review_tests;

#[cfg(test)]
mod followup_tests;
