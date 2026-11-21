//! Token endpoint (RFC 6749 §4.1.3, §6) and revocation (RFC 7009) as plain form posts.
//!
//! The client is public (`client_id` in the body, no secret). Revocation is done here instead of a
//! library because plain `http` must work on loopback during development.

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use url::Url;

use crate::discovery::OidcEndpoints;
use crate::error::AuthError;

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

#[derive(Deserialize, Default)]
struct TokenErrorBody {
    #[serde(default)]
    error: String,
}

pub struct TokenResponse {
    pub access_token: SecretString,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<SecretString>,
    pub id_token: Option<String>,
}

pub async fn exchange_code(
    http: &reqwest::Client,
    endpoints: &OidcEndpoints,
    client_id: &str,
    code: &str,
    redirect_uri: &Url,
    verifier: &str,
) -> Result<TokenResponse, AuthError> {
    post_token(
        http,
        &endpoints.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", client_id),
            ("code_verifier", verifier),
        ],
    )
    .await
}

pub async fn refresh(
    http: &reqwest::Client,
    endpoints: &OidcEndpoints,
    client_id: &str,
    refresh_token: &SecretString,
) -> Result<TokenResponse, AuthError> {
    post_token(
        http,
        &endpoints.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose_secret()),
            ("client_id", client_id),
        ],
    )
    .await
}

async fn post_token(
    http: &reqwest::Client,
    endpoint: &Url,
    form: &[(&str, &str)],
) -> Result<TokenResponse, AuthError> {
    let response = http
        .post(endpoint.clone())
        .form(form)
        .send()
        .await
        .map_err(AuthError::from_reqwest)?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(AuthError::from_reqwest)?;
    if !status.is_success() {
        let body: TokenErrorBody = serde_json::from_slice(&bytes).unwrap_or_default();
        if body.error == "invalid_grant" {
            return Err(AuthError::InvalidGrant);
        }
        return Err(AuthError::TokenEndpoint {
            status: status.as_u16(),
            error: body.error,
        });
    }
    let raw: RawTokenResponse = serde_json::from_slice(&bytes)
        .map_err(|_| AuthError::InvalidResponse("token response is not valid JSON".to_owned()))?;
    if !raw.token_type.is_empty() && !raw.token_type.eq_ignore_ascii_case("bearer") {
        return Err(AuthError::InvalidResponse(format!(
            "token type {}",
            raw.token_type
        )));
    }
    Ok(TokenResponse {
        access_token: SecretString::from(raw.access_token),
        expires_in: raw.expires_in,
        refresh_token: raw.refresh_token.map(SecretString::from),
        id_token: raw.id_token,
    })
}

/// RFC 7009 revocation. The server answers 200 even for unknown tokens; errors are reported to the
/// caller, who treats revocation as best effort.
pub async fn revoke(
    http: &reqwest::Client,
    endpoints: &OidcEndpoints,
    client_id: &str,
    token: &SecretString,
    hint: &str,
) -> Result<(), AuthError> {
    let Some(endpoint) = &endpoints.revocation_endpoint else {
        return Ok(());
    };
    let response = http
        .post(endpoint.clone())
        .form(&[
            ("token", token.expose_secret()),
            ("token_type_hint", hint),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(AuthError::from_reqwest)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(AuthError::TokenEndpoint {
            status: response.status().as_u16(),
            error: "revocation failed".to_owned(),
        })
    }
}
