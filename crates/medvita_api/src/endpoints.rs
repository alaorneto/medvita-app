//! Core endpoints the shell uses. Module endpoints live with their modules.

use uuid::Uuid;

use crate::client::{ApiClient, Versioned};
use crate::error::ApiError;
use crate::models::{ActiveApp, Me, Organization, ServerInfo};
use crate::pagination::Results;

impl ApiClient {
    /// `GET me/`: identity and profile of the signed-in person (composite ETag `"<user>-<profile>"`).
    pub async fn me(&self) -> Result<Versioned<Me>, ApiError> {
        self.get("me/", &[]).await
    }

    /// `GET organizations/`: memberships of the person, unpaginated.
    pub async fn organizations(&self) -> Result<Vec<Organization>, ApiError> {
        let page: Versioned<Results<Organization>> = self.get("organizations/", &[]).await?;
        Ok(page.value.results)
    }

    /// Active organizations only, sorted by name.
    pub async fn active_organizations(&self) -> Result<Vec<Organization>, ApiError> {
        let mut organizations: Vec<_> = self
            .organizations()
            .await?
            .into_iter()
            .filter(|item| item.is_active)
            .collect();
        organizations.sort_by_key(|item| item.organization_name.to_lowercase());
        Ok(organizations)
    }

    /// `GET server/` (anonymous). `None` on servers that do not have it yet.
    pub async fn server_info(&self) -> Result<Option<ServerInfo>, ApiError> {
        Ok(self
            .get_public::<ServerInfo>("server/", &[])
            .await?
            .map(|value| value.value))
    }

    /// `GET o/<org>/apps/active/`. `None` on servers that do not have it yet.
    pub async fn active_apps(
        &self,
        organization: Uuid,
    ) -> Result<Option<Vec<ActiveApp>>, ApiError> {
        let path = self.org(organization).path("apps/active/");
        Ok(self
            .get_optional::<Results<ActiveApp>>(&path, &[])
            .await?
            .map(|page| page.value.results))
    }
}
