//! Paths scoped to an organization (`o/<uuid>/…`) and to an app (`o/<uuid>/apps/<label>/…`).
//! The organization only travels in the path, never in a header or body (core rule).

use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::client::{ApiClient, Versioned, WriteOptions};
use crate::error::ApiError;

#[derive(Clone, Copy)]
pub struct OrgScope<'a> {
    client: &'a ApiClient,
    organization: Uuid,
}

impl<'a> OrgScope<'a> {
    pub(crate) fn new(client: &'a ApiClient, organization: Uuid) -> Self {
        Self {
            client,
            organization,
        }
    }

    pub fn organization(&self) -> Uuid {
        self.organization
    }

    /// `o/<uuid>/<path>`.
    pub fn path(&self, path: &str) -> String {
        format!("o/{}/{}", self.organization, path.trim_start_matches('/'))
    }

    pub fn app(&self, label: &'a str) -> AppScope<'a> {
        AppScope { org: *self, label }
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Versioned<T>, ApiError> {
        self.client.get(&self.path(path), query).await
    }
}

#[derive(Clone, Copy)]
pub struct AppScope<'a> {
    org: OrgScope<'a>,
    label: &'a str,
}

impl AppScope<'_> {
    /// `o/<uuid>/apps/<label>/<path>`.
    pub fn path(&self, path: &str) -> String {
        self.org.path(&format!(
            "apps/{}/{}",
            self.label,
            path.trim_start_matches('/')
        ))
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Versioned<T>, ApiError> {
        self.org.client.get(&self.path(path), query).await
    }

    pub async fn post<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        options: WriteOptions,
    ) -> Result<Versioned<T>, ApiError> {
        self.org.client.post(&self.path(path), body, options).await
    }

    pub async fn patch<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        options: WriteOptions,
    ) -> Result<Versioned<T>, ApiError> {
        self.org.client.patch(&self.path(path), body, options).await
    }
}
