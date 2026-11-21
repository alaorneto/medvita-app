//! Platform layer of Medvita Desktop: settings, paths, logging, build information and the secret
//! store interface. Nothing here depends on GPUI, so it is testable without a GPU.

pub mod build_info;
pub mod logging;
pub mod paths;
pub mod secret;
pub mod server_url;
pub mod settings;

pub use paths::AppPaths;
pub use secret::{MemoryStore, SecretStore, SecretStoreError};
pub use server_url::{ServerUrlError, is_loopback_host, parse_server_url};
pub use settings::{Settings, SettingsStore, ThemePreference, UpdateChannel};
