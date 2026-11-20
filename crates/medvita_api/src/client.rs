//! The HTTP client: one `reqwest::Client`, the server base URL and the token provider.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde::de::DeserializeOwned;
use url::Url;
use uuid::Uuid;

use crate::auth::AccessTokenProvider;
use crate::error::{ApiError, ErrorBody, from_status};
use crate::scope::OrgScope;

/// Prefix of every API path, relative to the server base URL.
pub const API_PREFIX: &str = "api/v1/";

/// The server refuses bodies over 1 MiB with 413; refuse them before sending.
pub const MAX_REQUEST_BODY: usize = 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The `ETag` of a response, exactly as sent (quotes included), to send back in `If-Match`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ETag(pub String);

/// Value of the `Idempotency-Key` header: a fresh UUID v4 per logical operation. Keep the same key
/// when retrying the same operation, so the server does not apply it twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdempotencyKey(Uuid);

impl IdempotencyKey {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for IdempotencyKey {
    fn default() -> Self {
        Self::new()
    }
}

/// Concurrency and idempotency headers of a write.
#[derive(Debug, Clone, Default)]
pub struct WriteOptions {
    pub if_match: Option<ETag>,
    pub idempotency_key: Option<IdempotencyKey>,
}

impl WriteOptions {
    pub fn if_match(etag: ETag) -> Self {
        Self {
            if_match: Some(etag),
            ..Self::default()
        }
    }

    pub fn idempotent(key: IdempotencyKey) -> Self {
        Self {
            idempotency_key: Some(key),
            ..Self::default()
        }
    }
}

/// A decoded response with its `ETag` and `X-Request-ID`.
#[derive(Debug, Clone)]
pub struct Versioned<T> {
    pub value: T,
    pub etag: Option<ETag>,
    pub request_id: Option<String>,
}

impl<T> Versioned<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Versioned<U> {
        Versioned {
            value: f(self.value),
            etag: self.etag,
            request_id: self.request_id,
        }
    }
}

struct RawResponse {
    status: StatusCode,
    body: Vec<u8>,
    etag: Option<ETag>,
    request_id: Option<String>,
    retry_after: Option<Duration>,
}

impl RawResponse {
    fn into_error(self) -> ApiError {
        from_status(
            self.status.as_u16(),
            &self.body,
            self.retry_after,
            self.request_id,
        )
    }

    /// A 401 the client should renew the token for (any 401 without another explicit code).
    fn is_invalid_token(&self) -> bool {
        if self.status != StatusCode::UNAUTHORIZED {
            return false;
        }
        matches!(
            ErrorBody::parse(&self.body).code.as_deref(),
            None | Some("invalid_token")
        )
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    base: Url,
    auth: Arc<dyn AccessTokenProvider>,
}

impl ApiClient {
    /// A client with the default HTTP settings (see [`ApiClient::http_client_builder`]).
    pub fn new(base: Url, auth: Arc<dyn AccessTokenProvider>) -> Result<Self, ApiError> {
        let http =
            Self::http_client_builder()
                .build()
                .map_err(|error| ApiError::InvalidRequest {
                    message: format!("HTTP client: {error}"),
                })?;
        Ok(Self::with_http(http, base, auth))
    }

    pub fn with_http(http: reqwest::Client, base: Url, auth: Arc<dyn AccessTokenProvider>) -> Self {
        Self { http, base, auth }
    }

    /// The HTTP settings every Medvita request uses: product `User-Agent`, JSON `Accept`, timeouts,
    /// and no redirects (a redirect would carry the Bearer token to another place).
    pub fn http_client_builder() -> reqwest::ClientBuilder {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        reqwest::Client::builder()
            .user_agent(medvita_platform::build_info::user_agent())
            .default_headers(headers)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    /// Paths under `o/<organization>/`.
    pub fn org(&self, organization: Uuid) -> OrgScope<'_> {
        OrgScope::new(self, organization)
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Versioned<T>, ApiError> {
        let raw = self
            .execute(
                Method::GET,
                path,
                query,
                None,
                &WriteOptions::default(),
                true,
            )
            .await?;
        decode(raw)
    }

    /// Like [`ApiClient::get`], with `404` as `None`: for endpoints older servers do not have.
    pub async fn get_optional<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Option<Versioned<T>>, ApiError> {
        match self.get(path, query).await {
            Ok(value) => Ok(Some(value)),
            Err(ApiError::NotFound { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// A GET without credentials, for the anonymous endpoints (server information, update policy).
    pub async fn get_public<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<Option<Versioned<T>>, ApiError> {
        let raw = self
            .execute(
                Method::GET,
                path,
                query,
                None,
                &WriteOptions::default(),
                false,
            )
            .await?;
        if raw.status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        decode(raw).map(Some)
    }

    pub async fn post<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        options: WriteOptions,
    ) -> Result<Versioned<T>, ApiError> {
        let body = encode(body)?;
        let raw = self
            .execute(Method::POST, path, &[], Some(body), &options, true)
            .await?;
        decode(raw)
    }

    pub async fn patch<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        options: WriteOptions,
    ) -> Result<Versioned<T>, ApiError> {
        let body = encode(body)?;
        let raw = self
            .execute(Method::PATCH, path, &[], Some(body), &options, true)
            .await?;
        decode(raw)
    }

    pub async fn delete(&self, path: &str, options: WriteOptions) -> Result<(), ApiError> {
        let raw = self
            .execute(Method::DELETE, path, &[], None, &options, true)
            .await?;
        if raw.status.is_success() {
            Ok(())
        } else {
            Err(raw.into_error())
        }
    }

    /// `<base>api/v1/<path>`, refusing anything that would leave the API prefix of this server.
    pub(crate) fn endpoint(&self, path: &str) -> Result<Url, ApiError> {
        let relative = path.trim_start_matches('/');
        let invalid = || ApiError::InvalidRequest {
            message: "path outside the API".to_owned(),
        };
        if relative.contains("://") || relative.contains('?') || relative.contains('#') {
            return Err(invalid());
        }
        let url = self
            .base
            .join(&format!("{API_PREFIX}{relative}"))
            .map_err(|_| invalid())?;
        let prefix = format!("{}{API_PREFIX}", self.base.path());
        if url.origin() != self.base.origin() || !url.path().starts_with(&prefix) {
            return Err(invalid());
        }
        Ok(url)
    }

    /// Sends the request, renewing the token and repeating once on `401 invalid_token`.
    async fn execute(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Vec<u8>>,
        options: &WriteOptions,
        authenticated: bool,
    ) -> Result<RawResponse, ApiError> {
        let url = self.endpoint(path)?;
        if !authenticated {
            return self
                .send_once(&method, &url, query, body.as_deref(), options, None)
                .await;
        }
        let token = self.auth.access_token().await.map_err(ApiError::Auth)?;
        let raw = self
            .send_once(&method, &url, query, body.as_deref(), options, Some(&token))
            .await?;
        if !raw.is_invalid_token() {
            return Ok(raw);
        }
        let renewed = self
            .auth
            .refresh_after_unauthorized(&token)
            .await
            .map_err(ApiError::Auth)?;
        self.send_once(
            &method,
            &url,
            query,
            body.as_deref(),
            options,
            Some(&renewed),
        )
        .await
    }

    async fn send_once(
        &self,
        method: &Method,
        url: &Url,
        query: &[(&str, String)],
        body: Option<&[u8]>,
        options: &WriteOptions,
        token: Option<&SecretString>,
    ) -> Result<RawResponse, ApiError> {
        let mut request = self.http.request(method.clone(), url.clone());
        if !query.is_empty() {
            request = request.query(query);
        }
        if let Some(token) = token {
            request = request.bearer_auth(token.expose_secret());
        }
        if let Some(etag) = &options.if_match {
            request = request.header("If-Match", etag.0.as_str());
        }
        if let Some(key) = &options.idempotency_key {
            request = request.header("Idempotency-Key", key.as_uuid().to_string());
        }
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }

        let started = Instant::now();
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(transport_error)?.to_vec();
        let request_id = header_text(&headers, "x-request-id");

        // Only the method, the path (never the query), the status and timings: AGENTS.md.
        tracing::info!(
            target: "medvita_api",
            method = %method,
            route = url.path(),
            status = status.as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            request_id = request_id.as_deref().unwrap_or("-"),
            "api request"
        );

        Ok(RawResponse {
            status,
            body,
            etag: header_text(&headers, "etag").map(ETag),
            retry_after: header_text(&headers, "retry-after")
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(Duration::from_secs),
            request_id,
        })
    }
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn transport_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        return ApiError::Timeout;
    }
    // `without_url` keeps the query string (and any identifier in it) out of the message.
    ApiError::Network {
        message: error.without_url().to_string(),
    }
}

fn encode<B: Serialize>(body: &B) -> Result<Vec<u8>, ApiError> {
    let bytes = serde_json::to_vec(body).map_err(|error| ApiError::InvalidRequest {
        message: format!("body not serializable: {error}"),
    })?;
    if bytes.len() > MAX_REQUEST_BODY {
        return Err(ApiError::PayloadTooLarge { request_id: None });
    }
    Ok(bytes)
}

fn decode<T: DeserializeOwned>(raw: RawResponse) -> Result<Versioned<T>, ApiError> {
    if !raw.status.is_success() {
        return Err(raw.into_error());
    }
    match serde_json::from_slice::<T>(&raw.body) {
        Ok(value) => Ok(Versioned {
            value,
            etag: raw.etag,
            request_id: raw.request_id,
        }),
        // The serde message can quote values from the body (names, documents): keep only the
        // category and the position.
        Err(error) => Err(ApiError::Decode {
            message: format!(
                "{:?} at line {} column {}",
                error.classify(),
                error.line(),
                error.column()
            ),
            request_id: raw.request_id,
        }),
    }
}
