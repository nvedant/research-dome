//! Runtime configuration: where the database, uploaded PDFs, figures and LaTeX
//! workspaces live on the user's machine.

use std::path::{Path, PathBuf};

/// Environment variable that overrides the data directory (useful for portable
/// installs, backups and tests).
pub const DATA_DIR_ENV: &str = "RESEARCH_DOME_DATA_DIR";

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
}

impl Config {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into() }
    }

    /// Resolve the data directory: `RESEARCH_DOME_DATA_DIR` wins, otherwise the
    /// supplied platform default (the OS per-user application data folder).
    pub fn resolve(platform_default: impl AsRef<Path>) -> Self {
        match std::env::var_os(DATA_DIR_ENV) {
            Some(value) if !value.is_empty() => Self::new(PathBuf::from(value)),
            _ => Self::new(platform_default.as_ref()),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("research-dome.sqlite")
    }

    pub fn storage_root(&self) -> PathBuf {
        self.data_dir.join("storage")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }
}
