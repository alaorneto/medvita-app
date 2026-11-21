//! Sign-in of Medvita Desktop through the web system's own login (AGENTS.md, docs/authentication.md).
//!
//! OIDC Authorization Code with PKCE (S256), `state` and `nonce`, in the system browser, returning
//! to a loopback listener on `127.0.0.1` (RFC 8252). The refresh token lives only in the Windows
//! Credential Manager (local persistence); the access token only in memory. Renewal is single-flight
//! because the server rotates refresh tokens without reuse detection.
//!
//! Token requests are plain form posts (RFC 6749/7636/7009) on the shared `reqwest` client: see
//! docs/adr/0002-oidc-loopback.md for why the `oauth2` crate is not used.

pub mod claims;
pub mod discovery;
pub mod error;
pub mod login;
pub mod loopback;
#[cfg(debug_assertions)]
pub mod personal;
pub mod pkce;
pub mod session;
pub mod tokens;
pub mod vault;

pub use discovery::{OidcEndpoints, discover};
pub use error::AuthError;
pub use login::{LoginAttempt, OidcClient};
pub use session::OidcSession;
pub use vault::default_store;

/// Scopes requested from the server. `offline_access` is not requested: the server issues a
/// refresh token with every code grant and refuses scopes the client was not registered with.
pub const SCOPES: &str = "openid profile email";

/// How long the loopback listener waits for the browser to come back.
pub const LOGIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Vault account of a server: its origin (`https://host[:port]`).
pub fn vault_account(server: &url::Url) -> String {
    server.origin().ascii_serialization()
}
