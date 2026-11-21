//! `MedvitaServerSource` against a simulated Medvita server (wiremock).

use std::sync::mpsc;

use chrono::Utc;
use ed25519_dalek::SigningKey;
use medvita_updater::envelope::sign_envelope;
use medvita_updater::source::channel_url;
use medvita_updater::state::EnvelopeState;
use medvita_updater::{MedvitaServerSource, UpdateError};
use serde_json::json;
use url::Url;
use velopack::sources::UpdateSource;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT: &str = "medvita-desktop";
const PACKAGE: &[u8] = b"pretend this is a nupkg with enough bytes to report progress";

fn feed() -> Vec<u8> {
    serde_json::to_vec(&json!({"Assets": [{
        "PackageId": "Medvita", "Version": "0.2.0", "Type": "Full",
        "FileName": "Medvita-0.2.0-full.nupkg", "SHA1": "", "SHA256": "",
        "Size": PACKAGE.len(), "NotesMarkdown": "", "NotesHtml": ""
    }]}))
    .unwrap()
}

struct Fixture {
    _server: MockServer,
    _state_dir: tempfile::TempDir,
    source: MedvitaServerSource,
}

async fn fixture(
    envelope: Option<Vec<u8>>,
    served_feed: Vec<u8>,
    verify_with: &SigningKey,
) -> Fixture {
    let server = MockServer::start().await;
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    let prefix = channel_url(&base, CLIENT, "stable")
        .unwrap()
        .path()
        .to_owned();
    Mock::given(path(format!("{prefix}releases.win.json")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(served_feed))
        .mount(&server)
        .await;
    let envelope_response = match envelope {
        Some(bytes) => ResponseTemplate::new(200).set_body_bytes(bytes),
        None => ResponseTemplate::new(404),
    };
    Mock::given(path(format!("{prefix}releases.win.sig.json")))
        .respond_with(envelope_response)
        .mount(&server)
        .await;
    Mock::given(path(format!("{prefix}Medvita-0.2.0-full.nupkg")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(PACKAGE))
        .mount(&server)
        .await;
    let state_dir = tempfile::tempdir().unwrap();
    let source = MedvitaServerSource::new(
        reqwest::Client::new(),
        tokio::runtime::Handle::current(),
        &base,
        CLIENT,
        "stable",
        verify_with.verifying_key(),
        EnvelopeState::new(state_dir.path()),
    )
    .unwrap();
    Fixture {
        _server: server,
        _state_dir: state_dir,
        source,
    }
}

fn envelope_for(feed: &[u8], channel: &str, signer: &SigningKey) -> Vec<u8> {
    serde_json::to_vec(&sign_envelope(feed, CLIENT, channel, Utc::now(), signer)).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn verified_feed_is_returned_and_packages_download_with_progress() {
    let signer = SigningKey::from_bytes(&[3; 32]);
    let f = fixture(
        Some(envelope_for(&feed(), "stable", &signer)),
        feed(),
        &signer,
    )
    .await;

    let (feed, payload) = f.source.fetch_verified_feed().await.unwrap();
    assert_eq!(feed.Assets.len(), 1);
    assert_eq!(payload.channel, "stable");

    // Through the synchronous Velopack trait, as the UpdateManager calls it.
    let source = std::sync::Arc::new(f.source);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("pkg.nupkg");
    let (sender, receiver) = mpsc::channel();
    let asset = feed.Assets[0].clone();
    let trait_source = source.clone();
    let target_path = target.clone();
    tokio::task::spawn_blocking(move || {
        let manifest = velopack::bundle::Manifest::default();
        trait_source.get_release_feed("win", &manifest, "").unwrap();
        trait_source
            .download_release_entry(&asset, &target_path, Some(sender))
            .unwrap();
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), PACKAGE);
    let progress: Vec<i16> = receiver.try_iter().collect();
    assert_eq!(progress.last(), Some(&100));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feed_without_envelope_is_refused() {
    let signer = SigningKey::from_bytes(&[3; 32]);
    let f = fixture(None, feed(), &signer).await;
    assert!(matches!(
        f.source.fetch_verified_feed().await,
        Err(UpdateError::Envelope(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn envelope_signed_by_another_key_is_refused() {
    let signer = SigningKey::from_bytes(&[3; 32]);
    let intruder = SigningKey::from_bytes(&[4; 32]);
    let f = fixture(
        Some(envelope_for(&feed(), "stable", &intruder)),
        feed(),
        &signer,
    )
    .await;
    assert!(matches!(
        f.source.fetch_verified_feed().await,
        Err(UpdateError::Envelope(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feed_swapped_after_signing_is_refused() {
    let signer = SigningKey::from_bytes(&[3; 32]);
    let other_feed = br#"{"Assets":[]}"#.to_vec();
    let f = fixture(
        Some(envelope_for(&feed(), "stable", &signer)),
        other_feed,
        &signer,
    )
    .await;
    assert!(matches!(
        f.source.fetch_verified_feed().await,
        Err(UpdateError::Envelope(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn envelope_for_another_channel_is_refused() {
    let signer = SigningKey::from_bytes(&[3; 32]);
    let f = fixture(
        Some(envelope_for(&feed(), "beta", &signer)),
        feed(),
        &signer,
    )
    .await;
    assert!(matches!(
        f.source.fetch_verified_feed().await,
        Err(UpdateError::Envelope(_))
    ));
}
