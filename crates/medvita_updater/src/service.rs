//! The update service the UI drives: check, download in the background, restart into the new
//! version. Every method that talks to Velopack blocks: call it from a blocking thread
//! (`tokio::task::spawn_blocking`), never from the GPUI main thread or an async task.

use std::path::PathBuf;
use std::time::Duration;

use medvita_platform::UpdateChannel;
use medvita_platform::build_info::{self, CLIENT_ID};
use url::Url;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions};

use crate::policy::{self, Policy};
use crate::source::MedvitaServerSource;
use crate::state::EnvelopeState;
use crate::{UpdateError, release_public_key};

/// First check after the app opens, then every six hours.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub struct UpdaterConfig {
    pub server: Url,
    pub channel: UpdateChannel,
    pub state_dir: PathBuf,
    pub runtime: tokio::runtime::Handle,
    /// Needed when the person moves from beta to stable, whose latest version may be lower.
    pub allow_downgrade: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdaterStatus {
    /// Not available in this run (development run, key not configured…), with the reason.
    Disabled(String),
    Idle,
    Checking,
    UpToDate,
    Downloading {
        version: String,
        percent: u8,
    },
    /// Downloaded; applied on restart (or on the next start, automatically).
    Ready {
        version: String,
    },
    Failed(String),
}

/// A downloaded update, ready to apply.
pub struct PendingUpdate {
    pub version: String,
    info: UpdateInfo,
}

pub struct UpdateService {
    manager: Option<UpdateManager>,
    disabled_reason: Option<String>,
    http: reqwest::Client,
    server: Url,
    channel: UpdateChannel,
}

/// HTTP for updates: no total timeout (packages are large), a read timeout instead; redirects
/// allowed because every byte is checked against the signed feed.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(build_info::user_agent())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .expect("HTTP client for updates")
}

impl UpdateService {
    pub fn new(config: UpdaterConfig) -> Self {
        let http = http_client();
        let disabled = |reason: &str, http: reqwest::Client| Self {
            manager: None,
            disabled_reason: Some(reason.to_owned()),
            http,
            server: config.server.clone(),
            channel: config.channel,
        };
        let Some(key) = release_public_key() else {
            return disabled("chave de atualização não configurada", http);
        };
        let source = match MedvitaServerSource::new(
            http.clone(),
            config.runtime.clone(),
            &config.server,
            CLIENT_ID,
            config.channel.as_str(),
            key,
            EnvelopeState::new(&config.state_dir),
        ) {
            Ok(source) => source,
            Err(error) => return disabled(&error.to_string(), http),
        };
        let options = UpdateOptions {
            AllowVersionDowngrade: config.allow_downgrade,
            ExplicitChannel: None,
            MaximumDeltasBeforeFallback: 10,
        };
        match UpdateManager::new(source, Some(options), None) {
            Ok(manager) => Self {
                manager: Some(manager),
                disabled_reason: None,
                http,
                server: config.server.clone(),
                channel: config.channel,
            },
            Err(velopack::Error::NotInstalled(_)) => disabled("execução fora do instalador", http),
            Err(error) => disabled(&error.to_string(), http),
        }
    }

    pub fn initial_status(&self) -> UpdaterStatus {
        match &self.disabled_reason {
            Some(reason) => UpdaterStatus::Disabled(reason.clone()),
            None => match self.pending_restart() {
                Some(version) => UpdaterStatus::Ready { version },
                None => UpdaterStatus::Idle,
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.manager.is_some()
    }

    pub fn current_version(&self) -> String {
        self.manager
            .as_ref()
            .map(UpdateManager::get_current_version_as_string)
            .unwrap_or_else(|| build_info::version().to_owned())
    }

    /// A version already downloaded and waiting for a restart.
    pub fn pending_restart(&self) -> Option<String> {
        self.manager
            .as_ref()?
            .get_update_pending_restart()
            .map(|asset| asset.Version)
    }

    /// Checks the server and downloads the update, if any. Blocking.
    pub fn check_and_download(
        &self,
        on_progress: impl Fn(&str, u8) + Sync,
    ) -> Result<Option<PendingUpdate>, UpdateError> {
        let manager = self.manager()?;
        let info = match manager.check_for_updates().map_err(velopack_error)? {
            UpdateCheck::UpdateAvailable(info) => *info,
            UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => return Ok(None),
        };
        let version = info.TargetFullRelease.Version.clone();
        let (sender, receiver) = std::sync::mpsc::channel::<i16>();
        let on_progress = &on_progress;
        let reported = version.clone();
        std::thread::scope(|scope| {
            // Ends when `download_updates` drops the sender.
            scope.spawn(move || {
                for percent in receiver {
                    on_progress(&reported, percent.clamp(0, 100) as u8);
                }
            });
            let result = manager.download_updates(&info, Some(sender));
            result.map_err(velopack_error)
        })?;
        Ok(Some(PendingUpdate { version, info }))
    }

    /// Exits the app, applies the update and starts the new version. Does not return on success.
    pub fn apply_and_restart(&self, pending: &PendingUpdate) -> Result<(), UpdateError> {
        self.manager()?
            .apply_updates_and_restart(&pending.info)
            .map_err(velopack_error)
    }

    /// The version policy of the server for this client (async; no blocking).
    pub async fn policy(&self) -> Result<Option<Policy>, UpdateError> {
        if !self.is_enabled() {
            return Ok(None);
        }
        policy::fetch_policy(
            &self.http,
            &self.server,
            CLIENT_ID,
            self.channel.as_str(),
            &self.current_version(),
        )
        .await
    }

    fn manager(&self) -> Result<&UpdateManager, UpdateError> {
        self.manager.as_ref().ok_or_else(|| {
            UpdateError::Unavailable(self.disabled_reason.clone().unwrap_or_default())
        })
    }
}

fn velopack_error(error: velopack::Error) -> UpdateError {
    UpdateError::Velopack(error.to_string())
}
