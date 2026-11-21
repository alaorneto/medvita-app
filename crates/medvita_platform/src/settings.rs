//! Person-level preferences, saved as JSON in `%APPDATA%\Medvita\settings.json`.
//!
//! Loading never fails: a missing, unreadable, corrupt or newer-schema file gives the defaults and a
//! warning in the log, so a broken file never keeps the app from opening. Saving is atomic.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

/// Schema of `settings.json`. Bump it when a field changes meaning, and migrate in `load`.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    /// Follows the Windows light/dark setting.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
}

impl UpdateChannel {
    /// The path segment of the channel on the update server.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema: u32,
    /// Server chosen by the person; `None` falls back to the build default.
    pub server_url: Option<Url>,
    pub theme: ThemePreference,
    pub update_channel: UpdateChannel,
    /// Last organization chosen, per server host. Only the UUID: never names or other data.
    pub last_organization: BTreeMap<String, Uuid>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            server_url: None,
            theme: ThemePreference::default(),
            update_channel: UpdateChannel::default(),
            last_organization: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the settings, falling back to the defaults on any problem (logged as a warning).
    pub fn load(&self) -> Settings {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Settings::default(),
            Err(error) => {
                tracing::warn!(%error, "settings unreadable; using defaults");
                return Settings::default();
            }
        };
        match serde_json::from_slice::<Settings>(&bytes) {
            Ok(settings) if settings.schema == SCHEMA => settings,
            Ok(settings) => {
                tracing::warn!(
                    schema = settings.schema,
                    "settings schema not supported; using defaults"
                );
                Settings::default()
            }
            Err(error) => {
                tracing::warn!(%error, "settings corrupt; using defaults");
                Settings::default()
            }
        }
    }

    /// Writes the settings atomically: a temporary file in the same directory, flushed, then renamed
    /// over the old one, so a crash never leaves a half-written file.
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        let dir = self.path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "settings path has no parent")
        })?;
        std::fs::create_dir_all(dir)?;
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp)?;
            serde_json::to_writer_pretty(&mut file, settings).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("config").join("settings.json"));
        (dir, store)
    }

    #[test]
    fn missing_file_gives_defaults() {
        let (_dir, store) = store();
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let (_dir, store) = store();
        let mut settings = Settings {
            server_url: Some(Url::parse("https://medvita.example").unwrap()),
            theme: ThemePreference::Dark,
            update_channel: UpdateChannel::Beta,
            ..Settings::default()
        };
        settings
            .last_organization
            .insert("medvita.example".into(), Uuid::new_v4());
        store.save(&settings).unwrap();
        assert_eq!(store.load(), settings);
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let (_dir, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), b"{ not json").unwrap();
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn newer_schema_gives_defaults() {
        let (_dir, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), br#"{"schema": 99, "theme": "dark"}"#).unwrap();
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn unknown_fields_and_missing_fields_are_tolerated() {
        let (_dir, store) = store();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(
            store.path(),
            br#"{"schema": 1, "theme": "light", "future": true}"#,
        )
        .unwrap();
        let loaded = store.load();
        assert_eq!(loaded.theme, ThemePreference::Light);
        assert_eq!(loaded.update_channel, UpdateChannel::Stable);
    }

    #[test]
    fn save_leaves_no_temporary_file() {
        let (_dir, store) = store();
        store.save(&Settings::default()).unwrap();
        store.save(&Settings::default()).unwrap();
        let dir = store.path().parent().unwrap();
        let names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("settings.json")]);
    }
}
