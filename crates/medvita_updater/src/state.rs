//! The `signed_at` of the last envelope accepted, per channel, in `%LOCALAPPDATA%\Medvita\state`.
//! A feed signed before it is refused (rollback protection).

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct EnvelopeState {
    path: PathBuf,
}

impl EnvelopeState {
    pub fn new(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            path: state_dir.into().join("update-envelope.json"),
        }
    }

    fn read(&self) -> BTreeMap<String, DateTime<Utc>> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn last_signed_at(&self, channel: &str) -> Option<DateTime<Utc>> {
        self.read().get(channel).copied()
    }

    /// Records `signed_at` for `channel` if it is newer than what is stored.
    pub fn accept(&self, channel: &str, signed_at: DateTime<Utc>) -> io::Result<()> {
        let mut all = self.read();
        if all
            .get(channel)
            .is_some_and(|current| *current >= signed_at)
        {
            return Ok(());
        }
        all.insert(channel.to_owned(), signed_at);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(&all).map_err(io::Error::other)?)?;
            file.sync_all()?;
        }
        std::fs::rename(tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_per_channel() {
        let dir = tempfile::tempdir().unwrap();
        let state = EnvelopeState::new(dir.path());
        let older = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let newer = DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(state.last_signed_at("stable").is_none());
        state.accept("stable", newer).unwrap();
        state.accept("stable", older).unwrap();
        state.accept("beta", older).unwrap();
        assert_eq!(state.last_signed_at("stable"), Some(newer));
        assert_eq!(state.last_signed_at("beta"), Some(older));
    }
}
