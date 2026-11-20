//! Version policy of the server: latest and minimum version per channel.
//! `GET <server>/api/v1/clients/medvita-desktop/policy?version=<v>&os=win&channel=<c>` (anonymous).

use serde::Deserialize;
use url::Url;

use crate::UpdateError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Policy {
    pub latest_version: Option<String>,
    pub minimum_version: Option<String>,
    #[serde(default)]
    pub update_required: bool,
}

impl Policy {
    /// `true` when this version may no longer be used: the server says so, or it is below the minimum.
    pub fn requires_update(&self, current: &str) -> bool {
        if self.update_required {
            return true;
        }
        match (
            self.minimum_version
                .as_deref()
                .and_then(|v| semver::Version::parse(v).ok()),
            semver::Version::parse(current).ok(),
        ) {
            (Some(minimum), Some(current)) => current < minimum,
            _ => false,
        }
    }
}

pub fn policy_url(server: &Url, client_id: &str) -> Result<Url, UpdateError> {
    server
        .join(&format!("api/v1/clients/{client_id}/policy"))
        .map_err(|error| UpdateError::Feed(error.to_string()))
}

/// The policy, or `None` on servers that do not publish one yet (404).
pub async fn fetch_policy(
    http: &reqwest::Client,
    server: &Url,
    client_id: &str,
    channel: &str,
    version: &str,
) -> Result<Option<Policy>, UpdateError> {
    let response = http
        .get(policy_url(server, client_id)?)
        .header("Accept", "application/json")
        .query(&[
            ("version", version),
            ("os", crate::OS),
            ("channel", channel),
        ])
        .send()
        .await
        .map_err(UpdateError::from_reqwest)?;
    match response.status().as_u16() {
        404 => Ok(None),
        200 => response
            .json::<Policy>()
            .await
            .map(Some)
            .map_err(|_| UpdateError::Feed("invalid policy document".to_owned())),
        status => Err(UpdateError::Status(status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(minimum: Option<&str>, required: bool) -> Policy {
        Policy {
            latest_version: Some("0.3.0".into()),
            minimum_version: minimum.map(Into::into),
            update_required: required,
        }
    }

    #[test]
    fn below_the_minimum_requires_an_update() {
        assert!(policy(Some("0.2.0"), false).requires_update("0.1.9"));
        assert!(!policy(Some("0.2.0"), false).requires_update("0.2.0"));
        assert!(policy(None, true).requires_update("0.2.0"));
        assert!(!policy(None, false).requires_update("0.2.0"));
        assert!(!policy(Some("garbage"), false).requires_update("0.2.0"));
    }
}
