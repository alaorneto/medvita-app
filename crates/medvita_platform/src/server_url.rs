//! Validation of the server URL the person types (or the build default).
//!
//! `https` is required. Plain `http` is accepted only on loopback, for development against a local
//! server. The URL is normalized to its origin plus an optional base path ending in `/`, so joining
//! `api/v1/...` onto it works.

use url::{Host, Url};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerUrlError {
    #[error("not a valid URL")]
    Invalid,
    #[error("only https is accepted outside loopback")]
    InsecureScheme,
    #[error("unsupported scheme")]
    UnsupportedScheme,
    #[error("the URL must not carry credentials, query or fragment")]
    UnexpectedParts,
}

/// `true` for `127.0.0.1` (and the rest of 127/8), `::1` and `localhost`.
pub fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
    }
}

pub fn parse_server_url(input: &str) -> Result<Url, ServerUrlError> {
    let trimmed = input.trim();
    let mut url = Url::parse(trimmed).map_err(|_| ServerUrlError::Invalid)?;
    let host = url.host().ok_or(ServerUrlError::Invalid)?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback_host(&host) => {}
        "http" => return Err(ServerUrlError::InsecureScheme),
        _ => return Err(ServerUrlError::UnsupportedScheme),
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ServerUrlError::UnexpectedParts);
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_accepted_and_normalized() {
        let url = parse_server_url(" https://medvita.example ").unwrap();
        assert_eq!(url.as_str(), "https://medvita.example/");
        let url = parse_server_url("https://medvita.example/base").unwrap();
        assert_eq!(url.as_str(), "https://medvita.example/base/");
    }

    #[test]
    fn http_only_on_loopback() {
        assert!(parse_server_url("http://127.0.0.1:8000").is_ok());
        assert!(parse_server_url("http://localhost:8000").is_ok());
        assert!(parse_server_url("http://[::1]:8000").is_ok());
        assert_eq!(
            parse_server_url("http://medvita.example"),
            Err(ServerUrlError::InsecureScheme)
        );
    }

    #[test]
    fn rejects_other_schemes_and_extra_parts() {
        assert_eq!(
            parse_server_url("ftp://medvita.example"),
            Err(ServerUrlError::UnsupportedScheme)
        );
        assert_eq!(
            parse_server_url("medvita.example"),
            Err(ServerUrlError::Invalid)
        );
        assert_eq!(
            parse_server_url("https://user:pw@medvita.example"),
            Err(ServerUrlError::UnexpectedParts)
        );
        assert_eq!(
            parse_server_url("https://medvita.example/?x=1"),
            Err(ServerUrlError::UnexpectedParts)
        );
    }
}
