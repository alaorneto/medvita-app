//! The client against a simulated server (wiremock).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use medvita_api::{
    AccessTokenProvider, ApiClient, ApiError, AuthFailure, CursorPage, ETag, IdempotencyKey, Me,
    Versioned, WriteOptions,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Hands out `t1`, then `t2` after a renewal; counts renewals.
struct CountingProvider {
    renewals: AtomicUsize,
    fail_renewal: bool,
}

impl CountingProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            renewals: AtomicUsize::new(0),
            fail_renewal: false,
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Self {
            renewals: AtomicUsize::new(0),
            fail_renewal: true,
        })
    }

    fn renewals(&self) -> usize {
        self.renewals.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AccessTokenProvider for CountingProvider {
    async fn access_token(&self) -> Result<SecretString, AuthFailure> {
        let token = if self.renewals() == 0 { "t1" } else { "t2" };
        Ok(SecretString::from(token))
    }

    async fn refresh_after_unauthorized(
        &self,
        rejected: &SecretString,
    ) -> Result<SecretString, AuthFailure> {
        assert_eq!(rejected.expose_secret(), "t1");
        self.renewals.fetch_add(1, Ordering::SeqCst);
        if self.fail_renewal {
            return Err(AuthFailure::SessionExpired);
        }
        Ok(SecretString::from("t2"))
    }
}

async fn client(provider: Arc<CountingProvider>) -> (MockServer, ApiClient) {
    let server = MockServer::start().await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    let client = ApiClient::new(base, provider).unwrap();
    (server, client)
}

fn me_body() -> serde_json::Value {
    json!({
        "sysid": Uuid::nil(),
        "email": "ana@example.com",
        "first_name": "Ana",
        "last_name": "Silva",
        "revision": 3,
        "profile": {"sysid": null, "revision": 5, "display_name": "", "timezone": "America/Sao_Paulo", "language": "pt-br"}
    })
}

#[tokio::test]
async fn me_sends_the_bearer_and_keeps_etag_and_request_id() {
    let (server, client) = client(CountingProvider::new()).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/me/"))
        .and(header("authorization", "Bearer t1"))
        .and(header("accept", "application/json"))
        .and(header_exists("user-agent"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"3-5\"")
                .insert_header("X-Request-ID", "req-1")
                .set_body_json(me_body()),
        )
        .expect(1)
        .mount(&server)
        .await;

    let me: Versioned<Me> = client.me().await.unwrap();
    assert_eq!(me.value.display_name(), "Ana Silva");
    assert_eq!(me.etag, Some(ETag("\"3-5\"".into())));
    assert_eq!(me.request_id.as_deref(), Some("req-1"));
}

#[tokio::test]
async fn invalid_token_renews_once_and_repeats() {
    let provider = CountingProvider::new();
    let (server, client) = client(provider.clone()).await;
    Mock::given(path("/api/v1/me/"))
        .and(header("authorization", "Bearer t1"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"code": "invalid_token", "detail": "x"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/me/"))
        .and(header("authorization", "Bearer t2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me_body()))
        .expect(1)
        .mount(&server)
        .await;

    client.me().await.unwrap();
    assert_eq!(provider.renewals(), 1);
}

#[tokio::test]
async fn second_unauthorized_is_returned_without_a_loop() {
    let provider = CountingProvider::new();
    let (server, client) = client(provider.clone()).await;
    Mock::given(path("/api/v1/me/"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"code": "invalid_token"})))
        .expect(2)
        .mount(&server)
        .await;

    let error = client.me().await.unwrap_err();
    assert!(matches!(error, ApiError::Unauthorized { .. }), "{error:?}");
    assert!(error.needs_sign_in());
    assert_eq!(provider.renewals(), 1);
}

#[tokio::test]
async fn failed_renewal_asks_for_sign_in() {
    let provider = CountingProvider::failing();
    let (server, client) = client(provider.clone()).await;
    Mock::given(path("/api/v1/me/"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let error = client.me().await.unwrap_err();
    assert!(
        matches!(error, ApiError::Auth(AuthFailure::SessionExpired)),
        "{error:?}"
    );
    assert!(error.needs_sign_in());
}

#[tokio::test]
async fn token_not_allowed_does_not_renew() {
    let provider = CountingProvider::new();
    let (server, client) = client(provider.clone()).await;
    Mock::given(path("/api/v1/me/"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("X-Request-ID", "req-9")
                .set_body_json(json!({"code": "token_not_allowed", "detail": "Use a sessão."})),
        )
        .mount(&server)
        .await;

    let error = client.me().await.unwrap_err();
    assert_eq!(error.code(), Some("token_not_allowed"));
    assert_eq!(error.request_id(), Some("req-9"));
    assert_eq!(provider.renewals(), 0);
}

#[tokio::test]
async fn every_documented_status_maps_to_its_variant() {
    let (server, client) = client(CountingProvider::new()).await;
    let cases: Vec<(u16, serde_json::Value)> = vec![
        (
            400,
            json!({"code": "validation_error", "detail": "Verifique.", "fields": {"email": ["Inválido."]}}),
        ),
        (404, json!({"code": "not_found", "detail": "Not found."})),
        (406, json!({"code": "not_acceptable"})),
        (409, json!({"code": "record_sealed", "detail": "Selado."})),
        (412, json!({"code": "revision_conflict"})),
        (413, json!({"code": "payload_too_large"})),
        (428, json!({"code": "revision_required"})),
        (500, json!({})),
    ];
    for (status, body) in &cases {
        Mock::given(path(format!("/api/v1/status/{status}/")))
            .respond_with(ResponseTemplate::new(*status).set_body_json(body.clone()))
            .mount(&server)
            .await;
    }
    let result = |status: u16| {
        let client = &client;
        async move {
            client
                .get::<serde_json::Value>(&format!("status/{status}/"), &[])
                .await
                .unwrap_err()
        }
    };
    match result(400).await {
        ApiError::Validation { fields, .. } => {
            assert_eq!(fields["email"], vec!["Inválido.".to_owned()])
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(result(404).await, ApiError::NotFound { .. }));
    assert!(matches!(result(406).await, ApiError::NotAcceptable { .. }));
    assert!(
        matches!(result(409).await, ApiError::Conflict { code, .. } if code == "record_sealed")
    );
    assert!(matches!(
        result(412).await,
        ApiError::PreconditionFailed { .. }
    ));
    assert!(matches!(
        result(413).await,
        ApiError::PayloadTooLarge { .. }
    ));
    assert!(matches!(
        result(428).await,
        ApiError::PreconditionRequired { .. }
    ));
    assert!(matches!(
        result(500).await,
        ApiError::Server { status: 500, .. }
    ));
}

#[tokio::test]
async fn rate_limit_and_maintenance_expose_when_to_retry() {
    let (server, client) = client(CountingProvider::new()).await;
    Mock::given(path("/api/v1/limited/"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "30"))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/down/"))
        .respond_with(
            ResponseTemplate::new(503)
                .insert_header("Retry-After", "600")
                .set_body_json(json!({"code": "maintenance", "detail": "Em manutenção.", "ends_at": "2026-11-21T03:00:00+00:00"})),
        )
        .mount(&server)
        .await;

    match client
        .get::<serde_json::Value>("limited/", &[])
        .await
        .unwrap_err()
    {
        ApiError::RateLimited { retry_after, .. } => {
            assert_eq!(retry_after, Some(Duration::from_secs(30)))
        }
        other => panic!("{other:?}"),
    }
    let error = client
        .get::<serde_json::Value>("down/", &[])
        .await
        .unwrap_err();
    assert!(error.is_transient());
    match error {
        ApiError::Maintenance {
            retry_after,
            ends_at,
            ..
        } => {
            assert_eq!(retry_after, Some(Duration::from_secs(600)));
            assert_eq!(ends_at.unwrap().to_rfc3339(), "2026-11-21T03:00:00+00:00");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn organizations_and_cursor_pages_are_read() {
    let (server, client) = client(CountingProvider::new()).await;
    let org = Uuid::new_v4();
    Mock::given(path("/api/v1/organizations/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": [
            {"sysid": Uuid::new_v4(), "organization_id": org, "is_active": true, "organization_name": "Clínica B", "organization_slug": "b"},
            {"sysid": Uuid::new_v4(), "organization_id": Uuid::new_v4(), "is_active": false, "organization_name": "Antiga", "organization_slug": "a"}
        ]})))
        .mount(&server)
        .await;
    Mock::given(path(format!("/api/v1/o/{org}/apps/med_patients/patients/")))
        .and(query_param("cursor", "abc"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"results": [1, 2], "next_cursor": "def"})),
        )
        .mount(&server)
        .await;

    let active = client.active_organizations().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].organization_id, org);

    let page: Versioned<CursorPage<u32>> = client
        .org(org)
        .app("med_patients")
        .get("patients/", &[("cursor", "abc".to_owned())])
        .await
        .unwrap();
    assert_eq!(page.value.results, vec![1, 2]);
    assert_eq!(page.value.next_cursor.as_deref(), Some("def"));
}

#[tokio::test]
async fn writes_send_if_match_and_idempotency_key() {
    let (server, client) = client(CountingProvider::new()).await;
    let org = Uuid::new_v4();
    let key = IdempotencyKey::new();
    Mock::given(method("POST"))
        .and(path(format!(
            "/api/v1/o/{org}/apps/med_scheduling/appointments/"
        )))
        .and(header(
            "idempotency-key",
            key.as_uuid().to_string().as_str(),
        ))
        .and(header("content-type", "application/json"))
        .respond_with(
            ResponseTemplate::new(201)
                .insert_header("ETag", "\"1\"")
                .set_body_json(json!({"ok": true})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!(
            "/api/v1/o/{org}/apps/med_patients/patients/1/"
        )))
        .and(header("if-match", "\"1\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "\"2\"")
                .set_body_json(json!({"ok": true})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let created: Versioned<serde_json::Value> = client
        .org(org)
        .app("med_scheduling")
        .post(
            "appointments/",
            &json!({"x": 1}),
            WriteOptions::idempotent(key),
        )
        .await
        .unwrap();
    let etag = created.etag.unwrap();
    let updated: Versioned<serde_json::Value> = client
        .org(org)
        .app("med_patients")
        .patch(
            "patients/1/",
            &json!({"y": 2}),
            WriteOptions::if_match(etag),
        )
        .await
        .unwrap();
    assert_eq!(updated.etag, Some(ETag("\"2\"".into())));
}

#[tokio::test]
async fn too_large_body_is_refused_before_sending() {
    let (server, client) = client(CountingProvider::new()).await;
    let big = "x".repeat(medvita_api::client::MAX_REQUEST_BODY + 1);
    let error = client
        .post::<_, serde_json::Value>("anything/", &big, WriteOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(error, ApiError::PayloadTooLarge { .. }));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn non_json_body_is_a_decode_error_without_its_content() {
    let (server, client) = client(CountingProvider::new()).await;
    Mock::given(path("/api/v1/me/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"email\": \"Maria Secreta\"}"))
        .mount(&server)
        .await;

    let error = client.me().await.unwrap_err();
    match &error {
        ApiError::Decode { message, .. } => assert!(!message.contains("Maria"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn missing_optional_endpoints_are_none() {
    let (server, client) = client(CountingProvider::new()).await;
    Mock::given(path("/api/v1/server/"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    assert!(client.server_info().await.unwrap().is_none());
    assert!(client.active_apps(Uuid::new_v4()).await.unwrap().is_none());
    // server/ is anonymous: no Authorization header is sent.
    let requests = server.received_requests().await.unwrap();
    let server_request = requests
        .iter()
        .find(|r| r.url.path() == "/api/v1/server/")
        .unwrap();
    assert!(!server_request.headers.contains_key("authorization"));
}

#[tokio::test]
async fn paths_cannot_leave_the_api() {
    let (_server, client) = client(CountingProvider::new()).await;
    for bad in [
        "https://evil.example/x",
        "../../admin/",
        "me/?x=1",
        "me/#frag",
    ] {
        let error = client.get::<serde_json::Value>(bad, &[]).await.unwrap_err();
        assert!(
            matches!(error, ApiError::InvalidRequest { .. }),
            "{bad}: {error:?}"
        );
    }
}
