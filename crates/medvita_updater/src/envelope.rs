//! The signed envelope of the update feed (`releases.win.sig.json`).
//!
//! ```json
//! {"payload": {"client_id": "medvita-desktop", "channel": "stable", "feed_sha256": "<hex>",
//!              "signed_at": "2026-11-20T12:00:00+00:00", "expires_at": "2027-03-20T12:00:00+00:00"},
//!  "signature": "<base64 Ed25519 over the canonical JSON of payload>"}
//! ```
//!
//! Same primitives as the core's release index: PEM SubjectPublicKeyInfo key, standard base64
//! signature, canonical JSON (see [`crate::canonical`]).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::UpdateError;
use crate::canonical::to_canonical_bytes;

/// Lifetime of a freshly signed envelope; `resign-feed.yml` renews it every month.
pub const VALIDITY_DAYS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub client_id: String,
    pub channel: String,
    pub feed_sha256: String,
    pub signed_at: String,
    pub expires_at: String,
}

impl Payload {
    pub fn signed_at(&self) -> Result<DateTime<Utc>, UpdateError> {
        parse_time(&self.signed_at)
    }

    pub fn expires_at(&self) -> Result<DateTime<Utc>, UpdateError> {
        parse_time(&self.expires_at)
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).expect("payload is plain strings");
        to_canonical_bytes(&value).expect("payload is plain strings")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub payload: Payload,
    pub signature: String,
}

/// What the client expects of an envelope.
pub struct Expectation<'a> {
    pub client_id: &'a str,
    pub channel: &'a str,
    pub now: DateTime<Utc>,
    /// `signed_at` of the last envelope accepted on this computer, for rollback protection.
    pub last_signed_at: Option<DateTime<Utc>>,
}

fn parse_time(value: &str) -> Result<DateTime<Utc>, UpdateError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| UpdateError::Envelope(format!("invalid timestamp {value:?}")))
}

pub fn feed_digest(feed: &[u8]) -> String {
    hex::encode(Sha256::digest(feed))
}

pub fn parse_public_key(pem: &str) -> Result<VerifyingKey, UpdateError> {
    VerifyingKey::from_public_key_pem(pem.trim())
        .map_err(|_| UpdateError::Envelope("release public key not configured".to_owned()))
}

/// Verifies `envelope_bytes` for `feed` and returns the payload. Nothing about the feed may be used
/// before this succeeds.
pub fn verify_envelope(
    envelope_bytes: &[u8],
    feed: &[u8],
    key: &VerifyingKey,
    expected: &Expectation<'_>,
) -> Result<Payload, UpdateError> {
    let reject = |reason: &str| UpdateError::Envelope(reason.to_owned());
    let envelope: Envelope =
        serde_json::from_slice(envelope_bytes).map_err(|_| reject("not a valid envelope"))?;
    let signature_bytes = STANDARD
        .decode(envelope.signature.as_bytes())
        .map_err(|_| reject("signature is not base64"))?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| reject("signature length"))?;
    key.verify(&envelope.payload.canonical_bytes(), &signature)
        .map_err(|_| reject("bad signature"))?;

    let payload = envelope.payload;
    if payload.client_id != expected.client_id {
        return Err(reject("envelope is for another client"));
    }
    if payload.channel != expected.channel {
        return Err(reject("envelope is for another channel"));
    }
    if !payload.feed_sha256.eq_ignore_ascii_case(&feed_digest(feed)) {
        return Err(reject("feed does not match the envelope"));
    }
    if payload.expires_at()? <= expected.now {
        return Err(reject("envelope expired"));
    }
    let signed_at = payload.signed_at()?;
    if let Some(last) = expected.last_signed_at
        && signed_at < last
    {
        return Err(reject("envelope older than the last one accepted"));
    }
    Ok(payload)
}

/// Builds and signs an envelope (used by `cargo xtask sign-feed` and by the tests).
pub fn sign_envelope(
    feed: &[u8],
    client_id: &str,
    channel: &str,
    signed_at: DateTime<Utc>,
    key: &ed25519_dalek::SigningKey,
) -> Envelope {
    use ed25519_dalek::Signer;
    let payload = Payload {
        client_id: client_id.to_owned(),
        channel: channel.to_owned(),
        feed_sha256: feed_digest(feed),
        signed_at: signed_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        expires_at: (signed_at + chrono::Duration::days(VALIDITY_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
    };
    let signature = key.sign(&payload.canonical_bytes());
    Envelope {
        payload,
        signature: STANDARD.encode(signature.to_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    const FEED: &[u8] = br#"{"Assets":[]}"#;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-20T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn expectation(last: Option<DateTime<Utc>>) -> Expectation<'static> {
        Expectation {
            client_id: "medvita-desktop",
            channel: "stable",
            now: now(),
            last_signed_at: last,
        }
    }

    fn signed(channel: &str, at: DateTime<Utc>, signer: &SigningKey) -> Vec<u8> {
        serde_json::to_vec(&sign_envelope(FEED, "medvita-desktop", channel, at, signer)).unwrap()
    }

    #[test]
    fn valid_envelope_is_accepted() {
        let signer = key(7);
        let bytes = signed("stable", now(), &signer);
        let payload =
            verify_envelope(&bytes, FEED, &signer.verifying_key(), &expectation(None)).unwrap();
        assert_eq!(payload.channel, "stable");
    }

    #[test]
    fn every_mismatch_is_rejected() {
        let signer = key(7);
        let good = signed("stable", now(), &signer);
        let vk = signer.verifying_key();
        type Case<'a> = (
            &'a str,
            Vec<u8>,
            &'a [u8],
            VerifyingKey,
            Option<DateTime<Utc>>,
        );
        let cases: Vec<Case> = vec![
            (
                "other key",
                good.clone(),
                FEED,
                key(8).verifying_key(),
                None,
            ),
            ("other feed", good.clone(), b"{}", vk, None),
            (
                "other channel",
                signed("beta", now(), &signer),
                FEED,
                vk,
                None,
            ),
            (
                "expired",
                signed(
                    "stable",
                    now() - chrono::Duration::days(VALIDITY_DAYS + 1),
                    &signer,
                ),
                FEED,
                vk,
                None,
            ),
            (
                "rollback",
                good.clone(),
                FEED,
                vk,
                Some(now() + chrono::Duration::days(1)),
            ),
            ("not json", b"nope".to_vec(), FEED, vk, None),
        ];
        for (name, bytes, feed, vk, last) in cases {
            assert!(
                verify_envelope(&bytes, feed, &vk, &expectation(last)).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn tampered_payload_breaks_the_signature() {
        let signer = key(7);
        let mut envelope = sign_envelope(FEED, "medvita-desktop", "stable", now(), &signer);
        envelope.payload.expires_at = "2099-01-01T00:00:00+00:00".into();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        assert!(
            verify_envelope(&bytes, FEED, &signer.verifying_key(), &expectation(None)).is_err()
        );
    }

    #[test]
    fn unset_key_is_not_a_key() {
        assert!(parse_public_key("UNSET").is_err());
    }
}
