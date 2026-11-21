//! Automatic updates of Medvita Desktop (docs/updates.md, docs/adr/0003-velopack-servidor.md).
//!
//! Velopack installs and applies the packages. The feed comes from the **Medvita server**, through
//! [`MedvitaServerSource`], and is trusted only after its signed envelope checks out:
//! Ed25519 over the canonical JSON of `{client_id, channel, feed_sha256, signed_at, expires_at}`,
//! with the public key committed in `keys/update-public.key`. Velopack then checks the SHA-256 of
//! every package against the verified feed. Downloads need no sign-in, so a client too old to sign in
//! can still update.

pub mod canonical;
pub mod envelope;
pub mod policy;
pub mod service;
pub mod source;
pub mod state;

pub use envelope::{Envelope, Expectation, Payload, verify_envelope};
pub use policy::{Policy, fetch_policy};
pub use service::{PendingUpdate, UpdateService, UpdaterConfig, UpdaterStatus};
pub use source::MedvitaServerSource;

/// The release public key compiled into the app. `UNSET` (before the owner generated the key pair)
/// disables the updater.
pub const PUBLIC_KEY_PEM: &str = include_str!("../keys/update-public.key");

/// Feed of the Windows channel produced by `vpk pack`.
pub const FEED_FILE: &str = "releases.win.json";
/// The signed envelope that accompanies the feed.
pub const ENVELOPE_FILE: &str = "releases.win.sig.json";
/// Operating system segment of the policy endpoint.
pub const OS: &str = "win";

/// The compiled public key, or `None` while it is `UNSET`.
pub fn release_public_key() -> Option<ed25519_dalek::VerifyingKey> {
    envelope::parse_public_key(PUBLIC_KEY_PEM).ok()
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),
    #[error("server answered HTTP {0}")]
    Status(u16),
    #[error("feed envelope rejected: {0}")]
    Envelope(String),
    #[error("invalid feed: {0}")]
    Feed(String),
    #[error("updater unavailable: {0}")]
    Unavailable(String),
    #[error("velopack: {0}")]
    Velopack(String),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

impl UpdateError {
    pub(crate) fn from_reqwest(error: reqwest::Error) -> Self {
        Self::Network(error.without_url().to_string())
    }
}
