//! Where the client gets its Bearer token. The OIDC implementation is in `medvita_auth`.

use async_trait::async_trait;
use secrecy::SecretString;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthFailure {
    /// Nobody is signed in on this server.
    #[error("not signed in")]
    NotSignedIn,
    /// The session can no longer be renewed (refresh token expired or revoked): sign in again.
    #[error("session expired")]
    SessionExpired,
    /// Renewal failed for another reason (network, server error); the session may still be valid.
    #[error("token renewal failed: {0}")]
    Renewal(String),
}

#[async_trait]
pub trait AccessTokenProvider: Send + Sync {
    /// A valid access token, renewing it first when it is about to expire.
    async fn access_token(&self) -> Result<SecretString, AuthFailure>;

    /// Called once after the server answered `401 invalid_token` to `rejected`. Implementations renew
    /// at most once at a time: when another task already renewed, they return the newer token.
    async fn refresh_after_unauthorized(
        &self,
        rejected: &SecretString,
    ) -> Result<SecretString, AuthFailure>;
}

/// A fixed token that cannot be renewed: the personal `skt_` token of the developer mode, and tests.
pub struct StaticTokenProvider {
    token: SecretString,
}

impl StaticTokenProvider {
    pub fn new(token: SecretString) -> Self {
        Self { token }
    }
}

#[async_trait]
impl AccessTokenProvider for StaticTokenProvider {
    async fn access_token(&self) -> Result<SecretString, AuthFailure> {
        Ok(self.token.clone())
    }

    async fn refresh_after_unauthorized(
        &self,
        _rejected: &SecretString,
    ) -> Result<SecretString, AuthFailure> {
        Err(AuthFailure::SessionExpired)
    }
}
