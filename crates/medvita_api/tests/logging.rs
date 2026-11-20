//! What the client logs. Its own test binary: a thread-local `tracing` subscriber is only reliable
//! when no other test registers callsites concurrently.

use std::io::Write;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use medvita_api::{AccessTokenProvider, ApiClient, AuthFailure};
use secrecy::SecretString;
use serde_json::json;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fixed;

#[async_trait]
impl AccessTokenProvider for Fixed {
    async fn access_token(&self) -> Result<SecretString, AuthFailure> {
        Ok(SecretString::from("t1-secret-token"))
    }
    async fn refresh_after_unauthorized(
        &self,
        _: &SecretString,
    ) -> Result<SecretString, AuthFailure> {
        Err(AuthFailure::SessionExpired)
    }
}

fn me_body() -> serde_json::Value {
    json!({
        "sysid": Uuid::nil(), "email": "ana@example.com", "first_name": "Ana", "last_name": "Silva",
        "revision": 3,
        "profile": {"sysid": null, "revision": 5, "display_name": "", "timezone": "America/Sao_Paulo", "language": "pt-br"}
    })
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn logs_carry_no_query_token_or_body() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_max_level(tracing::Level::INFO)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let server = MockServer::start().await;
    let client = ApiClient::new(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        Arc::new(Fixed),
    )
    .unwrap();
    Mock::given(path("/api/v1/me/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Request-ID", "req-7")
                .set_body_json(me_body()),
        )
        .mount(&server)
        .await;
    client
        .get::<serde_json::Value>("me/", &[("q", "cpf-123".to_owned())])
        .await
        .unwrap();

    let logged = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(logged.contains("/api/v1/me/"), "{logged}");
    assert!(logged.contains("req-7"), "{logged}");
    for secret in ["cpf-123", "t1-secret-token", "ana@example.com", "Silva"] {
        assert!(!logged.contains(secret), "log leaked {secret}: {logged}");
    }
}
