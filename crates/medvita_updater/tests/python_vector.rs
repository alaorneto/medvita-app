//! Interoperability with the core's Python primitives (`skeleton/system/packaging`): the vector
//! below was produced by `cryptography` + `json.dumps(sort_keys=True, separators=(",", ":"),
//! ensure_ascii=False)` from the Skeleton virtualenv, with the private key `bytes([9] * 32)`.
//! If this test fails, the server and the client disagree on what was signed.

use chrono::{DateTime, Utc};
use medvita_updater::canonical::to_canonical_bytes;
use medvita_updater::envelope::{Expectation, parse_public_key, verify_envelope};
use serde_json::json;

const PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA/RckOFqgx1tk+3jNYC+h2ZH96/drE8WO1wLqyDXp9hg=
-----END PUBLIC KEY-----
";

const ENVELOPE: &str = r#"{"payload": {"client_id": "medvita-desktop", "channel": "stable", "feed_sha256": "041e2b719b439eb948a28658a48b2dde418956613d82d9fa9fa9939d27729602", "signed_at": "2026-11-20T12:00:00+00:00", "expires_at": "2027-03-20T12:00:00+00:00"}, "signature": "qkPRSDy1/uZAY884KzzWwjaTPcBeq7uO3hu9+a12lhnUZtHZSPgtWsATuF+0z0CTLs4FHQ1eljsCdblCK228AA=="}"#;

const FEED: &[u8] = br#"{"Assets":[]}"#;

#[test]
fn envelope_signed_in_python_verifies_in_rust() {
    let key = parse_public_key(PUBLIC_KEY).unwrap();
    let now: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-12-01T00:00:00+00:00")
        .unwrap()
        .with_timezone(&Utc);
    let payload = verify_envelope(
        ENVELOPE.as_bytes(),
        FEED,
        &key,
        &Expectation {
            client_id: "medvita-desktop",
            channel: "stable",
            now,
            last_signed_at: None,
        },
    )
    .unwrap();
    assert_eq!(payload.signed_at, "2026-11-20T12:00:00+00:00");
}

#[test]
fn canonical_json_matches_python_byte_for_byte() {
    let value = json!({"nome": "Katuá", "b": [1, "\u{1}\n\"/"], "A": {"z": null, "y": true}});
    let python = "{\"A\":{\"y\":true,\"z\":null},\"b\":[1,\"\\u0001\\n\\\"/\"],\"nome\":\"Katuá\"}";
    assert_eq!(
        String::from_utf8(to_canonical_bytes(&value).unwrap()).unwrap(),
        python
    );
}
