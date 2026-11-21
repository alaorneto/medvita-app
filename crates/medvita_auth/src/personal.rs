//! Developer mode (debug builds only): sign in with a personal `skt_` token created on the web
//! system, so the shell can be exercised before the server offers OIDC. The token sits in the vault
//! under its own account; it cannot be renewed.

use std::sync::Arc;

use medvita_api::StaticTokenProvider;
use medvita_platform::SecretStore;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::error::AuthError;
use crate::vault_account;

fn account(server: &Url) -> String {
    format!("{} (token pessoal)", vault_account(server))
}

/// `skt_<8 hex>_<43 base64url>` (`skeleton/integrations/tokens/services.py`).
pub fn looks_like_personal_token(value: &str) -> bool {
    let mut parts = value.trim().splitn(3, '_');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some("skt"), Some(prefix), Some(secret))
            if prefix.len() == 8
                && prefix.chars().all(|c| c.is_ascii_hexdigit())
                && secret.len() >= 32
    )
}

pub fn save(
    store: &Arc<dyn SecretStore>,
    server: &Url,
    token: &SecretString,
) -> Result<StaticTokenProvider, AuthError> {
    let trimmed = SecretString::from(token.expose_secret().trim().to_owned());
    store.set(&account(server), &trimmed)?;
    Ok(StaticTokenProvider::new(trimmed))
}

pub fn restore(
    store: &Arc<dyn SecretStore>,
    server: &Url,
) -> Result<Option<StaticTokenProvider>, AuthError> {
    Ok(store.get(&account(server))?.map(StaticTokenProvider::new))
}

pub fn forget(store: &Arc<dyn SecretStore>, server: &Url) -> Result<(), AuthError> {
    Ok(store.delete(&account(server))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_token_format() {
        assert!(looks_like_personal_token(
            "skt_0123abcd_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_abcde"
        ));
        assert!(!looks_like_personal_token("skt_xyz_short"));
        assert!(!looks_like_personal_token("Bearer abc"));
    }
}
