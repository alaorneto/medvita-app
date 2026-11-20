//! [`MedvitaServerSource`]: Velopack's update source backed by the Medvita server.
//!
//! Layout on the server (docs/updates.md):
//! `<server>/clients/medvita-desktop/<channel>/releases.win.json`, `.../releases.win.sig.json` and
//! `.../<package file>`. The Velopack channel argument (`win`) is ignored: the Medvita channel
//! (`stable`/`beta`) is part of the path.
//!
//! The trait is synchronous; Velopack calls it from the updater thread, which must not be an async
//! task (the methods block on the tokio runtime).

use std::io::Write;
use std::path::Path;
use std::sync::mpsc::Sender;

use chrono::Utc;
use ed25519_dalek::VerifyingKey;
use futures::StreamExt;
use url::Url;
use velopack::sources::UpdateSource;
use velopack::{VelopackAsset, VelopackAssetFeed};

use crate::envelope::{Expectation, Payload, verify_envelope};
use crate::state::EnvelopeState;
use crate::{ENVELOPE_FILE, FEED_FILE, UpdateError};

pub struct MedvitaServerSource {
    http: reqwest::Client,
    runtime: tokio::runtime::Handle,
    channel_url: Url,
    client_id: String,
    channel: String,
    key: VerifyingKey,
    state: EnvelopeState,
}

impl MedvitaServerSource {
    pub fn new(
        http: reqwest::Client,
        runtime: tokio::runtime::Handle,
        server: &Url,
        client_id: &str,
        channel: &str,
        key: VerifyingKey,
        state: EnvelopeState,
    ) -> Result<Self, UpdateError> {
        Ok(Self {
            http,
            runtime,
            channel_url: channel_url(server, client_id, channel)?,
            client_id: client_id.to_owned(),
            channel: channel.to_owned(),
            key,
            state,
        })
    }

    async fn fetch(&self, name: &str) -> Result<Vec<u8>, UpdateError> {
        let url = self.file_url(name)?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(UpdateError::from_reqwest)?;
        if !response.status().is_success() {
            return Err(UpdateError::Status(response.status().as_u16()));
        }
        Ok(response
            .bytes()
            .await
            .map_err(UpdateError::from_reqwest)?
            .to_vec())
    }

    fn file_url(&self, name: &str) -> Result<Url, UpdateError> {
        if name.is_empty()
            || name.contains(['/', '\\', '?', '#', '%'])
            || name.contains("..")
            || name.starts_with('.')
        {
            return Err(UpdateError::Feed(format!("unexpected file name {name:?}")));
        }
        self.channel_url
            .join(name)
            .map_err(|error| UpdateError::Feed(error.to_string()))
    }

    /// Downloads the feed and its envelope and returns the feed only if the envelope verifies.
    pub async fn fetch_verified_feed(&self) -> Result<(VelopackAssetFeed, Payload), UpdateError> {
        let feed_bytes = self.fetch(FEED_FILE).await?;
        let envelope_bytes = match self.fetch(ENVELOPE_FILE).await {
            Ok(bytes) => bytes,
            Err(UpdateError::Status(404)) => {
                return Err(UpdateError::Envelope(
                    "feed published without an envelope".to_owned(),
                ));
            }
            Err(error) => return Err(error),
        };
        let payload = verify_envelope(
            &envelope_bytes,
            &feed_bytes,
            &self.key,
            &Expectation {
                client_id: &self.client_id,
                channel: &self.channel,
                now: Utc::now(),
                last_signed_at: self.state.last_signed_at(&self.channel),
            },
        )?;
        let feed: VelopackAssetFeed = serde_json::from_slice(&feed_bytes)
            .map_err(|_| UpdateError::Feed("feed is not a Velopack feed".to_owned()))?;
        self.state.accept(&self.channel, payload.signed_at()?)?;
        Ok((feed, payload))
    }

    /// Streams one package to `local_file`, reporting 0–100 to `progress`.
    pub async fn download(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress: Option<Sender<i16>>,
    ) -> Result<(), UpdateError> {
        let url = self.file_url(&asset.FileName)?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(UpdateError::from_reqwest)?;
        if !response.status().is_success() {
            return Err(UpdateError::Status(response.status().as_u16()));
        }
        let total = asset.Size.max(1);
        let mut file = std::fs::File::create(local_file)?;
        let mut stream = response.bytes_stream();
        let mut received: u64 = 0;
        let mut last_percent = -1i16;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(UpdateError::from_reqwest)?;
            file.write_all(&chunk)?;
            received += chunk.len() as u64;
            let percent = ((received.min(total) * 100) / total) as i16;
            if percent != last_percent {
                last_percent = percent;
                if let Some(sender) = &progress {
                    let _ = sender.send(percent);
                }
            }
        }
        file.sync_all()?;
        Ok(())
    }
}

/// `<server>/clients/<client_id>/<channel>/`.
pub fn channel_url(server: &Url, client_id: &str, channel: &str) -> Result<Url, UpdateError> {
    server
        .join(&format!("clients/{client_id}/{channel}/"))
        .map_err(|error| UpdateError::Feed(error.to_string()))
}

fn to_velopack(error: UpdateError) -> velopack::Error {
    match error {
        UpdateError::Io(error) => velopack::Error::Io(error),
        other => velopack::Error::Other(other.to_string()),
    }
}

impl UpdateSource for MedvitaServerSource {
    fn get_release_feed(
        &self,
        _velopack_channel: &str,
        _app: &velopack::bundle::Manifest,
        _staged_user_id: &str,
    ) -> Result<VelopackAssetFeed, velopack::Error> {
        let result = self.runtime.block_on(self.fetch_verified_feed());
        if let Err(error) = &result {
            tracing::warn!(%error, "update feed refused");
        }
        result.map(|(feed, _)| feed).map_err(to_velopack)
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress_sender: Option<Sender<i16>>,
    ) -> Result<(), velopack::Error> {
        self.runtime
            .block_on(self.download(asset, local_file, progress_sender))
            .map_err(to_velopack)
    }
}
