//! The OIDC flow against a simulated server (wiremock) and the real loopback listener.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use medvita_api::{AccessTokenProvider, AuthFailure};
use medvita_auth::{AuthError, OidcClient, vault_account};
use medvita_platform::{MemoryStore, SecretStore};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use url::Url;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT: &str = "medvita-desktop";

struct Fixture {
    server: MockServer,
    base: Url,
    store: Arc<dyn SecretStore>,
    client: OidcClient,
}

async fn fixture() -> Fixture {
    let server = MockServer::start().await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    let issuer = server.uri();
    Mock::given(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/identity/o/authorize"),
            "token_endpoint": format!("{issuer}/identity/o/api/token"),
            "revocation_endpoint": format!("{issuer}/identity/o/api/revoke"),
            "code_challenge_methods_supported": ["S256"]
        })))
        .mount(&server)
        .await;
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let client = OidcClient::new(http, base.clone(), CLIENT, store.clone());
    Fixture {
        server,
        base,
        store,
        client,
    }
}

fn id_token(issuer: &str, nonce: &str) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "iss": issuer,
            "aud": CLIENT,
            "exp": chrono::Utc::now().timestamp() + 300,
            "nonce": nonce,
            "sub": "1"
        }))
        .unwrap(),
    );
    format!("{header}.{payload}.c2ln")
}

fn query(url: &Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Plays the browser: requests the redirect URI with the given query.
fn browser(redirect: &Url, query: &str) -> tokio::task::JoinHandle<u16> {
    let url = format!("{redirect}?{query}");
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        reqwest::get(url)
            .await
            .map(|r| r.status().as_u16())
            .unwrap_or(0)
    })
}

async fn mount_token(
    server: &MockServer,
    form_fragment: &str,
    body: serde_json::Value,
    expect: u64,
) {
    Mock::given(method("POST"))
        .and(path("/identity/o/api/token"))
        .and(body_string_contains(form_fragment))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(expect)
        .mount(server)
        .await;
}

#[tokio::test]
async fn full_sign_in_keeps_the_refresh_token_in_the_vault() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let attempt = f.client.begin_login(&endpoints).await.unwrap();

    let params = query(attempt.authorize_url());
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["client_id"], CLIENT);
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["scope"], "openid profile email");
    assert_eq!(params["redirect_uri"], attempt.redirect_uri().as_str());
    assert!(
        attempt
            .redirect_uri()
            .as_str()
            .starts_with("http://127.0.0.1:")
    );

    mount_token(
        &f.server,
        "grant_type=authorization_code",
        json!({
            "access_token": "a1", "token_type": "Bearer", "expires_in": 3600,
            "refresh_token": "r1", "id_token": id_token(&f.server.uri(), &params["nonce"])
        }),
        1,
    )
    .await;

    let redirect = attempt.redirect_uri().clone();
    let callback = browser(&redirect, &format!("code=c1&state={}", params["state"]));
    let session = attempt.complete(Duration::from_secs(5)).await.unwrap();
    assert_eq!(callback.await.unwrap(), 200);

    assert_eq!(session.access_token().await.unwrap().expose_secret(), "a1");
    let stored = f.store.get(&vault_account(&f.base)).unwrap().unwrap();
    assert_eq!(stored.expose_secret(), "r1");

    // The code exchange carried the PKCE verifier and the exact redirect URI.
    let requests = f.server.received_requests().await.unwrap();
    let exchange = requests
        .iter()
        .find(|r| r.url.path() == "/identity/o/api/token")
        .unwrap();
    let form: HashMap<String, String> = url::form_urlencoded::parse(&exchange.body)
        .into_owned()
        .collect();
    assert_eq!(form["code"], "c1");
    assert_eq!(form["redirect_uri"], redirect.as_str());
    assert_eq!(
        medvita_auth::pkce::challenge_of(&form["code_verifier"]),
        params["code_challenge"]
    );
}

#[tokio::test]
async fn wrong_state_is_refused_and_the_real_callback_still_completes() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let attempt = f.client.begin_login(&endpoints).await.unwrap();
    let params = query(attempt.authorize_url());
    mount_token(
        &f.server,
        "code=good",
        json!({"access_token": "a1", "token_type": "Bearer", "refresh_token": "r1",
               "id_token": id_token(&f.server.uri(), &params["nonce"])}),
        1,
    )
    .await;

    let redirect = attempt.redirect_uri().clone();
    let forged = browser(&redirect, "code=evil&state=forged");
    let real = {
        let url = format!("{redirect}?code=good&state={}", params["state"]);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            reqwest::get(url).await.unwrap().status().as_u16()
        })
    };
    attempt.complete(Duration::from_secs(5)).await.unwrap();
    assert_eq!(forged.await.unwrap(), 400);
    assert_eq!(real.await.unwrap(), 200);
}

#[tokio::test]
async fn access_denied_is_reported() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let attempt = f.client.begin_login(&endpoints).await.unwrap();
    let state = query(attempt.authorize_url())["state"].clone();
    let redirect = attempt.redirect_uri().clone();
    let _browser = browser(&redirect, &format!("error=access_denied&state={state}"));
    let error = attempt
        .complete(Duration::from_secs(5))
        .await
        .err()
        .unwrap();
    assert!(
        matches!(error, AuthError::Denied { ref error, .. } if error == "access_denied"),
        "{error:?}"
    );
}

#[tokio::test]
async fn sign_in_times_out() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let attempt = f.client.begin_login(&endpoints).await.unwrap();
    let error = attempt
        .complete(Duration::from_millis(200))
        .await
        .err()
        .unwrap();
    assert!(matches!(error, AuthError::LoginTimedOut), "{error:?}");
}

#[tokio::test]
async fn wrong_nonce_is_refused() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let attempt = f.client.begin_login(&endpoints).await.unwrap();
    let state = query(attempt.authorize_url())["state"].clone();
    mount_token(
        &f.server,
        "grant_type=authorization_code",
        json!({"access_token": "a1", "token_type": "Bearer", "refresh_token": "r1",
               "id_token": id_token(&f.server.uri(), "another-nonce")}),
        1,
    )
    .await;
    let redirect = attempt.redirect_uri().clone();
    let _browser = browser(&redirect, &format!("code=c1&state={state}"));
    let error = attempt
        .complete(Duration::from_secs(5))
        .await
        .err()
        .unwrap();
    assert!(matches!(error, AuthError::InvalidIdToken(_)), "{error:?}");
    assert!(f.store.get(&vault_account(&f.base)).unwrap().is_none());
}

#[tokio::test]
async fn restored_session_renews_and_rotates_the_refresh_token() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let account = vault_account(&f.base);
    f.store.set(&account, &SecretString::from("r1")).unwrap();
    mount_token(
        &f.server,
        "refresh_token=r1",
        json!({"access_token": "a2", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "r2"}),
        1,
    )
    .await;

    let session = f.client.restore(&endpoints).unwrap().unwrap();
    assert_eq!(session.access_token().await.unwrap().expose_secret(), "a2");
    // Fresh now: no second request.
    assert_eq!(session.access_token().await.unwrap().expose_secret(), "a2");
    assert_eq!(
        f.store.get(&account).unwrap().unwrap().expose_secret(),
        "r2"
    );
}

#[tokio::test]
async fn concurrent_renewals_make_a_single_request() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    f.store
        .set(&vault_account(&f.base), &SecretString::from("r1"))
        .unwrap();
    Mock::given(method("POST"))
        .and(path("/identity/o/api/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_json(json!({"access_token": "a2", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "r2"})),
        )
        .expect(1)
        .mount(&f.server)
        .await;

    let session = Arc::new(f.client.restore(&endpoints).unwrap().unwrap());
    let (a, b) = tokio::join!(session.access_token(), session.access_token());
    assert_eq!(a.unwrap().expose_secret(), "a2");
    assert_eq!(b.unwrap().expose_secret(), "a2");
    // A 401 on the old token after someone else renewed reuses the new one.
    let renewed = session
        .refresh_after_unauthorized(&SecretString::from("a1"))
        .await
        .unwrap();
    assert_eq!(renewed.expose_secret(), "a2");
}

#[tokio::test]
async fn refused_refresh_token_ends_the_session() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let account = vault_account(&f.base);
    f.store.set(&account, &SecretString::from("r-old")).unwrap();
    Mock::given(method("POST"))
        .and(path("/identity/o/api/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .mount(&f.server)
        .await;

    let session = f.client.restore(&endpoints).unwrap().unwrap();
    assert_eq!(
        session.access_token().await.unwrap_err(),
        AuthFailure::SessionExpired
    );
    assert!(f.store.get(&account).unwrap().is_none());
}

#[tokio::test]
async fn logout_revokes_both_tokens_and_clears_the_vault_even_if_revocation_fails() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    let account = vault_account(&f.base);
    f.store.set(&account, &SecretString::from("r1")).unwrap();
    mount_token(
        &f.server,
        "refresh_token=r1",
        json!({"access_token": "a2", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "r2"}),
        1,
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/identity/o/api/revoke"))
        .respond_with(ResponseTemplate::new(500))
        .expect(2)
        .mount(&f.server)
        .await;

    let session = f.client.restore(&endpoints).unwrap().unwrap();
    session.access_token().await.unwrap();
    session.logout().await;
    assert!(f.store.get(&account).unwrap().is_none());

    let requests = f.server.received_requests().await.unwrap();
    let hints: Vec<String> = requests
        .iter()
        .filter(|r| r.url.path() == "/identity/o/api/revoke")
        .map(|r| String::from_utf8_lossy(&r.body).into_owned())
        .collect();
    assert!(
        hints
            .iter()
            .any(|b| b.contains("token=r2") && b.contains("token_type_hint=refresh_token"))
    );
    assert!(
        hints
            .iter()
            .any(|b| b.contains("token=a2") && b.contains("token_type_hint=access_token"))
    );
}

#[tokio::test]
async fn no_vault_entry_means_no_session() {
    let f = fixture().await;
    let endpoints = f.client.discover().await.unwrap();
    assert!(f.client.restore(&endpoints).unwrap().is_none());
}
