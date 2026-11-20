//! A signed-in session: the access token in memory, the refresh token in the vault, renewal that
//! happens one at a time.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use medvita_api::{AccessTokenProvider, AuthFailure};
use medvita_platform::SecretStore;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::Mutex;

use crate::discovery::OidcEndpoints;
use crate::error::AuthError;
use crate::tokens::{self, TokenResponse};

/// Renew when the access token has less than this left.
const RENEW_MARGIN: Duration = Duration::from_secs(60);
/// Lifetime assumed when the server does not say (`expires_in` missing).
const DEFAULT_LIFETIME: Duration = Duration::from_secs(300);

struct TokenState {
    access: Option<SecretString>,
    expires_at: Option<Instant>,
}

pub struct OidcSession {
    http: reqwest::Client,
    endpoints: OidcEndpoints,
    client_id: String,
    account: String,
    store: Arc<dyn SecretStore>,
    /// Held during renewal: this is what makes renewal single-flight.
    state: Mutex<TokenState>,
}

impl OidcSession {
    /// A session right after the code exchange. The refresh token is saved before returning.
    pub(crate) fn from_login(
        http: reqwest::Client,
        endpoints: OidcEndpoints,
        client_id: String,
        account: String,
        store: Arc<dyn SecretStore>,
        tokens: TokenResponse,
    ) -> Result<Self, AuthError> {
        if let Some(refresh) = &tokens.refresh_token {
            store.set(&account, refresh)?;
        } else {
            tracing::warn!(
                "server issued no refresh token; the session ends with the access token"
            );
        }
        Ok(Self {
            http,
            endpoints,
            client_id,
            account,
            store,
            state: Mutex::new(TokenState {
                expires_at: Some(expiry(tokens.expires_in)),
                access: Some(tokens.access_token),
            }),
        })
    }

    /// The session kept in the vault from a previous run, if any. No request is made: the first
    /// `access_token()` renews it (and reports `SessionExpired` if the server refuses).
    pub fn restore(
        http: reqwest::Client,
        endpoints: OidcEndpoints,
        client_id: String,
        account: String,
        store: Arc<dyn SecretStore>,
    ) -> Result<Option<Self>, AuthError> {
        if store.get(&account)?.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            http,
            endpoints,
            client_id,
            account,
            store,
            state: Mutex::new(TokenState {
                access: None,
                expires_at: None,
            }),
        }))
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    /// Ends the session: revokes both tokens (best effort) and always clears the vault and memory.
    pub async fn logout(&self) {
        let mut state = self.state.lock().await;
        let refresh = self.store.get(&self.account).ok().flatten();
        if let Some(refresh) = &refresh
            && let Err(error) = tokens::revoke(
                &self.http,
                &self.endpoints,
                &self.client_id,
                refresh,
                "refresh_token",
            )
            .await
        {
            tracing::warn!(%error, "refresh token revocation failed");
        }
        if let Some(access) = &state.access
            && let Err(error) = tokens::revoke(
                &self.http,
                &self.endpoints,
                &self.client_id,
                access,
                "access_token",
            )
            .await
        {
            tracing::warn!(%error, "access token revocation failed");
        }
        if let Err(error) = self.store.delete(&self.account) {
            tracing::error!(%error, "could not delete the refresh token from the vault");
        }
        state.access = None;
        state.expires_at = None;
    }

    fn is_fresh(state: &TokenState) -> bool {
        match (&state.access, state.expires_at) {
            (Some(_), Some(expires_at)) => Instant::now() + RENEW_MARGIN < expires_at,
            _ => false,
        }
    }

    /// Renews with the refresh token. Called with the state lock held.
    async fn renew(&self, state: &mut TokenState) -> Result<SecretString, AuthFailure> {
        let refresh = self
            .store
            .get(&self.account)
            .map_err(|error| AuthFailure::Renewal(error.to_string()))?
            .ok_or(AuthFailure::SessionExpired)?;
        match tokens::refresh(&self.http, &self.endpoints, &self.client_id, &refresh).await {
            Ok(tokens) => {
                // The server rotates the refresh token: keep the new one before anything else.
                if let Some(new_refresh) = &tokens.refresh_token {
                    self.store
                        .set(&self.account, new_refresh)
                        .map_err(|error| AuthFailure::Renewal(error.to_string()))?;
                }
                state.expires_at = Some(expiry(tokens.expires_in));
                state.access = Some(tokens.access_token.clone());
                Ok(tokens.access_token)
            }
            Err(AuthError::InvalidGrant) => {
                tracing::info!("refresh token refused; signing out");
                let _ = self.store.delete(&self.account);
                state.access = None;
                state.expires_at = None;
                Err(AuthFailure::SessionExpired)
            }
            Err(error) => Err(AuthFailure::Renewal(error.to_string())),
        }
    }
}

fn expiry(expires_in: Option<u64>) -> Instant {
    Instant::now()
        + expires_in
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_LIFETIME)
}

#[async_trait]
impl AccessTokenProvider for OidcSession {
    async fn access_token(&self) -> Result<SecretString, AuthFailure> {
        let mut state = self.state.lock().await;
        if Self::is_fresh(&state) {
            return Ok(state.access.clone().expect("fresh state has a token"));
        }
        self.renew(&mut state).await
    }

    async fn refresh_after_unauthorized(
        &self,
        rejected: &SecretString,
    ) -> Result<SecretString, AuthFailure> {
        let mut state = self.state.lock().await;
        // Another task renewed while this request was in flight: use its token.
        if let Some(current) = &state.access
            && current.expose_secret() != rejected.expose_secret()
            && Self::is_fresh(&state)
        {
            return Ok(current.clone());
        }
        state.access = None;
        self.renew(&mut state).await
    }
}
