use std::{fs, path::Path};

use thiserror::Error;

use crate::Config;

/// Failure to read or parse a profile configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The requested file could not be read.
    #[error("failed to read {path}: {source}")]
    Read {
        /// Path requested by the caller.
        path: String,
        /// Underlying filesystem error.
        source: std::io::Error,
    },
    /// The parsed configuration violates its semantic contract.
    #[error(transparent)]
    Validation(#[from] crate::ValidationError),
    /// The file is not a valid typed TOML configuration.
    #[error("failed to parse TOML configuration: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Read typed TOML configuration without mutating the source file.
///
/// Parsing rejects invalid public text and references, except icon keys in selected import
/// namespaces. Composition resolves those keys and applies full catalog usage limits.
pub fn load_config(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        source,
    })?;

    let config = toml::from_str(&text)?;
    crate::validate::validate_authored_config(&config)?;

    Ok(config)
}
