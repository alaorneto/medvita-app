//! PKCE (RFC 7636, always S256) and random values for `state` and `nonce`.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};

/// 32 random bytes, base64url without padding (43 characters).
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("operating system random source unavailable");
    URL_SAFE_NO_PAD.encode(bytes)
}

pub struct Pkce {
    pub verifier: SecretString,
    pub challenge: String,
}

pub const METHOD: &str = "S256";

impl Pkce {
    pub fn new() -> Self {
        Self::from_verifier(random_token())
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = challenge_of(&verifier);
        Self {
            verifier: SecretString::from(verifier),
            challenge,
        }
    }

    pub fn verifier(&self) -> &str {
        self.verifier.expose_secret()
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

pub fn challenge_of(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_7636_appendix_b_vector() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_values_are_long_and_distinct() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }
}
