//! The sign-in flow: discovery → authorization URL (opened in the system browser by the UI) →
//! loopback callback → code exchange → session.

use std::sync::Arc;
use std::time::Duration;

use medvita_platform::SecretStore;
use url::Url;

use crate::claims::{self, Expected};
use crate::discovery::{self, OidcEndpoints};
use crate::error::AuthError;
use crate::loopback::Loopback;
use crate::pkce::{self, Pkce};
use crate::session::OidcSession;
use crate::{SCOPES, tokens, vault_account};

pub struct OidcClient {
    http: reqwest::Client,
    server: Url,
    client_id: String,
    store: Arc<dyn SecretStore>,
}

impl OidcClient {
    /// `http` must not follow redirects (the API client builder already does that).
    pub fn new(
        http: reqwest::Client,
        server: Url,
        client_id: impl Into<String>,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            http,
            server,
            client_id: client_id.into(),
            store,
        }
    }

    pub fn server(&self) -> &Url {
        &self.server
    }

    pub async fn discover(&self) -> Result<OidcEndpoints, AuthError> {
        discovery::discover(&self.http, &self.server).await
    }

    /// The session kept in the vault for this server, if any.
    pub fn restore(&self, endpoints: &OidcEndpoints) -> Result<Option<OidcSession>, AuthError> {
        OidcSession::restore(
            self.http.clone(),
            endpoints.clone(),
            self.client_id.clone(),
            vault_account(&self.server),
            self.store.clone(),
        )
    }

    /// Prepares a sign-in: binds the loopback listener and builds the authorization URL.
    pub async fn begin_login(&self, endpoints: &OidcEndpoints) -> Result<LoginAttempt, AuthError> {
        let loopback = Loopback::bind().await?;
        let pkce = Pkce::new();
        let state = pkce::random_token();
        let nonce = pkce::random_token();
        let mut authorize_url = endpoints.authorization_endpoint.clone();
        authorize_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("redirect_uri", loopback.redirect_uri().as_str())
            .append_pair("scope", SCOPES)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", pkce::METHOD);
        Ok(LoginAttempt {
            client: OidcClient {
                http: self.http.clone(),
                server: self.server.clone(),
                client_id: self.client_id.clone(),
                store: self.store.clone(),
            },
            endpoints: endpoints.clone(),
            loopback,
            authorize_url,
            pkce,
            state,
            nonce,
        })
    }
}

/// A sign-in in progress. Dropping it (e.g. the person clicked Cancelar) closes the listener.
pub struct LoginAttempt {
    client: OidcClient,
    endpoints: OidcEndpoints,
    loopback: Loopback,
    authorize_url: Url,
    pkce: Pkce,
    state: String,
    nonce: String,
}

impl LoginAttempt {
    /// Open this in the system browser (never in a webview).
    pub fn authorize_url(&self) -> &Url {
        &self.authorize_url
    }

    pub fn redirect_uri(&self) -> &Url {
        self.loopback.redirect_uri()
    }

    /// Waits for the browser and exchanges the code; the refresh token is in the vault on success.
    pub async fn complete(self, timeout: Duration) -> Result<OidcSession, AuthError> {
        let redirect_uri = self.loopback.redirect_uri().clone();
        let code = self.loopback.wait_for_code(&self.state, timeout).await?;
        let response = tokens::exchange_code(
            &self.client.http,
            &self.endpoints,
            &self.client.client_id,
            &code,
            &redirect_uri,
            self.pkce.verifier(),
        )
        .await?;
        let id_token = response
            .id_token
            .as_deref()
            .ok_or_else(|| AuthError::InvalidIdToken("missing".to_owned()))?;
        claims::verify(
            id_token,
            &Expected {
                issuer: &self.endpoints.issuer,
                client_id: &self.client.client_id,
                nonce: &self.nonce,
                now: chrono::Utc::now().timestamp(),
            },
        )?;
        OidcSession::from_login(
            self.client.http.clone(),
            self.endpoints.clone(),
            self.client.client_id.clone(),
            vault_account(&self.client.server),
            self.client.store.clone(),
            response,
        )
    }
}
