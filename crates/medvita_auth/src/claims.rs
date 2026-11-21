//! Checks on the ID token received from the token endpoint over TLS.
//!
//! OIDC Core §3.1.3.7 lets a client that got the ID token directly from the token endpoint over TLS
//! rely on TLS instead of the signature; the claims still have to match. The profile itself comes
//! from the API (`me/`), not from the token.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use crate::discovery::normalize_issuer;
use crate::error::AuthError;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, client_id: &str) -> bool {
        match self {
            Self::One(value) => value == client_id,
            Self::Many(values) => values.iter().any(|value| value == client_id),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    exp: i64,
    nonce: Option<String>,
}

pub struct Expected<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub nonce: &'a str,
    /// Current Unix time, in seconds.
    pub now: i64,
}

/// Tolerance for clock differences between the computer and the server.
const LEEWAY_SECONDS: i64 = 120;

pub fn verify(id_token: &str, expected: &Expected<'_>) -> Result<(), AuthError> {
    let invalid = |reason: &str| AuthError::InvalidIdToken(reason.to_owned());
    let mut parts = id_token.split('.');
    let (Some(_header), Some(payload), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid("not a JWT"));
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| invalid("payload is not base64url"))?;
    let claims: Claims =
        serde_json::from_slice(&bytes).map_err(|_| invalid("payload is not JSON"))?;

    if normalize_issuer(&claims.iss) != normalize_issuer(expected.issuer) {
        return Err(invalid("issuer"));
    }
    if !claims.aud.contains(expected.client_id) {
        return Err(invalid("audience"));
    }
    if claims.exp + LEEWAY_SECONDS < expected.now {
        return Err(invalid("expired"));
    }
    if claims.nonce.as_deref() != Some(expected.nonce) {
        return Err(invalid("nonce"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn fake_id_token(claims: &serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    format!("{header}.{payload}.c2lnbmF0dXJl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000;

    fn expected() -> Expected<'static> {
        Expected {
            issuer: "https://medvita.example",
            client_id: "medvita-desktop",
            nonce: "n-1",
            now: NOW,
        }
    }

    fn token(overrides: serde_json::Value) -> String {
        let mut claims = json!({
            "iss": "https://medvita.example",
            "aud": "medvita-desktop",
            "exp": NOW + 300,
            "nonce": "n-1",
            "sub": "1"
        });
        for (key, value) in overrides.as_object().unwrap() {
            claims[key] = value.clone();
        }
        fake_id_token(&claims)
    }

    #[test]
    fn matching_claims_are_accepted() {
        verify(&token(json!({})), &expected()).unwrap();
        verify(
            &token(json!({"aud": ["other", "medvita-desktop"]})),
            &expected(),
        )
        .unwrap();
        verify(
            &token(json!({"iss": "https://medvita.example/"})),
            &expected(),
        )
        .unwrap();
    }

    #[test]
    fn mismatches_are_refused() {
        for overrides in [
            json!({"iss": "https://evil.example"}),
            json!({"aud": "other"}),
            json!({"exp": NOW - 3600}),
            json!({"nonce": "n-2"}),
            json!({"nonce": null}),
        ] {
            assert!(
                verify(&token(overrides.clone()), &expected()).is_err(),
                "{overrides}"
            );
        }
        assert!(verify("not-a-jwt", &expected()).is_err());
    }
}
