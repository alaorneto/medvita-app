//! OIDC discovery (`/.well-known/openid-configuration`) with the checks a native client needs.

use serde::Deserialize;
use url::Url;

use crate::error::AuthError;
use medvita_platform::is_loopback_host;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcEndpoints {
    /// Normalized issuer (no trailing slash).
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub revocation_endpoint: Option<Url>,
}

#[derive(Deserialize)]
struct Document {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: Option<String>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

/// `https://host/` and `https://host` are the same issuer (the server strips the slash).
pub fn normalize_issuer(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

/// Reads the discovery document of `server` and checks that:
/// - the issuer is the server itself;
/// - every endpoint is on the same origin and uses https (http only on loopback);
/// - S256 is supported when the document lists PKCE methods.
pub async fn discover(http: &reqwest::Client, server: &Url) -> Result<OidcEndpoints, AuthError> {
    let url = server
        .join(".well-known/openid-configuration")
        .map_err(|error| AuthError::Discovery(error.to_string()))?;
    let response = http
        .get(url)
        .send()
        .await
        .map_err(AuthError::from_reqwest)?;
    if !response.status().is_success() {
        return Err(AuthError::Discovery(format!(
            "HTTP {}",
            response.status().as_u16()
        )));
    }
    let document: Document = response
        .json()
        .await
        .map_err(|_| AuthError::Discovery("invalid discovery document".to_owned()))?;
    check(server, document)
}

fn check(server: &Url, document: Document) -> Result<OidcEndpoints, AuthError> {
    let expected = normalize_issuer(server.as_str());
    let found = normalize_issuer(&document.issuer);
    if expected != found {
        return Err(AuthError::IssuerMismatch { expected, found });
    }
    if !document.code_challenge_methods_supported.is_empty()
        && !document
            .code_challenge_methods_supported
            .iter()
            .any(|method| method == crate::pkce::METHOD)
    {
        return Err(AuthError::Discovery(
            "server does not support PKCE S256".to_owned(),
        ));
    }
    let endpoint = |value: &str| trusted_endpoint(server, value);
    Ok(OidcEndpoints {
        issuer: found,
        authorization_endpoint: endpoint(&document.authorization_endpoint)?,
        token_endpoint: endpoint(&document.token_endpoint)?,
        revocation_endpoint: document
            .revocation_endpoint
            .as_deref()
            .map(endpoint)
            .transpose()?,
    })
}

/// An endpoint the client may send codes and tokens to: same origin as the server, and https
/// unless the server itself is on loopback.
pub fn trusted_endpoint(server: &Url, value: &str) -> Result<Url, AuthError> {
    let url = Url::parse(value).map_err(|_| AuthError::UntrustedEndpoint(value.to_owned()))?;
    let secure = url.scheme() == "https"
        || (url.scheme() == "http" && url.host().is_some_and(|host| is_loopback_host(&host)));
    if !secure || url.origin() != server.origin() {
        return Err(AuthError::UntrustedEndpoint(value.to_owned()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(issuer: &str, token: &str) -> Document {
        Document {
            issuer: issuer.into(),
            authorization_endpoint: "https://medvita.example/identity/o/authorize".into(),
            token_endpoint: token.into(),
            revocation_endpoint: Some("https://medvita.example/identity/o/api/revoke".into()),
            code_challenge_methods_supported: vec!["S256".into()],
        }
    }

    #[test]
    fn issuer_is_compared_without_trailing_slash() {
        let server = Url::parse("https://medvita.example/").unwrap();
        let endpoints = check(
            &server,
            document(
                "https://medvita.example",
                "https://medvita.example/identity/o/api/token",
            ),
        )
        .unwrap();
        assert_eq!(endpoints.issuer, "https://medvita.example");
    }

    #[test]
    fn other_issuer_is_refused() {
        let server = Url::parse("https://medvita.example/").unwrap();
        let error = check(
            &server,
            document("https://evil.example", "https://medvita.example/t"),
        )
        .unwrap_err();
        assert!(matches!(error, AuthError::IssuerMismatch { .. }));
    }

    #[test]
    fn endpoint_on_another_origin_or_plain_http_is_refused() {
        let server = Url::parse("https://medvita.example/").unwrap();
        for token in ["https://evil.example/token", "http://medvita.example/token"] {
            let error = check(&server, document("https://medvita.example", token)).unwrap_err();
            assert!(matches!(error, AuthError::UntrustedEndpoint(_)), "{token}");
        }
    }

    #[test]
    fn plain_http_is_fine_on_loopback() {
        let server = Url::parse("http://127.0.0.1:8000/").unwrap();
        assert!(trusted_endpoint(&server, "http://127.0.0.1:8000/identity/o/api/token").is_ok());
    }

    #[test]
    fn server_without_s256_is_refused() {
        let server = Url::parse("https://medvita.example/").unwrap();
        let mut doc = document("https://medvita.example", "https://medvita.example/t");
        doc.code_challenge_methods_supported = vec!["plain".into()];
        assert!(matches!(check(&server, doc), Err(AuthError::Discovery(_))));
    }
}
