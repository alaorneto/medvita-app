//! Where the app keeps its files.
//!
//! - `%APPDATA%\Medvita` (roaming): `settings.json`.
//! - `%LOCALAPPDATA%\Medvita` (local, the Velopack install root): `logs\` and `state\`.
//!   Never under `current\`, which Velopack replaces on every update.
//!
//! No file here ever holds patient data (AGENTS.md).

use std::io;
use std::path::{Path, PathBuf};

use crate::build_info::DIR_NAME;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppPaths {
    /// The real locations of the current Windows user. `None` when the profile has no home.
    pub fn discover() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        Some(Self {
            config_dir: base.config_dir().join(DIR_NAME),
            data_dir: base.data_local_dir().join(DIR_NAME),
        })
    }

    /// Everything under one root, for tests and portable runs.
    pub fn under(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Small machine state that is not a setting (e.g. the last accepted update envelope).
    pub fn state_dir(&self) -> PathBuf {
        self.data_dir.join("state")
    }

    /// Creates the directories the app writes to.
    pub fn ensure(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(self.logs_dir())?;
        std::fs::create_dir_all(self.state_dir())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_a_root_keeps_every_path_inside_it() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        paths.ensure().unwrap();
        for path in [paths.settings_file(), paths.logs_dir(), paths.state_dir()] {
            assert!(path.starts_with(root.path()), "{path:?}");
        }
        assert!(paths.logs_dir().is_dir());
        assert!(paths.state_dir().is_dir());
    }

    #[test]
    fn discovered_paths_end_in_the_product_directory() {
        if let Some(paths) = AppPaths::discover() {
            assert!(paths.config_dir.ends_with(DIR_NAME));
            assert!(paths.data_dir.ends_with(DIR_NAME));
        }
    }
}
