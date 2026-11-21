//! Interface of the credential vault. The Windows implementation lives in `medvita_auth`, the only
//! crate allowed to touch tokens (AGENTS.md); tests use [`MemoryStore`].

use std::collections::HashMap;
use std::sync::Mutex;

use secrecy::{ExposeSecret, SecretString};

/// Service name of every vault entry (shown in the Windows Credential Manager).
pub const VAULT_SERVICE: &str = "Medvita Desktop";

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("credential vault unavailable: {0}")]
    Unavailable(String),
    #[error("secret too large for the credential vault")]
    TooLarge,
    #[error("credential vault failure: {0}")]
    Other(String),
}

/// A per-account secret store. `account` is the server host; the secret is the refresh token.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretStoreError>;
    fn set(&self, account: &str, secret: &SecretString) -> Result<(), SecretStoreError>;
    /// Removing an entry that does not exist is not an error.
    fn delete(&self, account: &str) -> Result<(), SecretStoreError>;
}

/// In-memory store for tests and for runs without a vault.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretStoreError> {
        let entries = self.entries.lock().expect("memory store poisoned");
        Ok(entries
            .get(account)
            .map(|value| SecretString::from(value.clone())))
    }

    fn set(&self, account: &str, secret: &SecretString) -> Result<(), SecretStoreError> {
        let mut entries = self.entries.lock().expect("memory store poisoned");
        entries.insert(account.to_owned(), secret.expose_secret().to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretStoreError> {
        let mut entries = self.entries.lock().expect("memory store poisoned");
        entries.remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_sets_gets_and_deletes() {
        let store = MemoryStore::new();
        assert!(store.get("host").unwrap().is_none());
        store.set("host", &SecretString::from("secret")).unwrap();
        assert_eq!(
            store.get("host").unwrap().unwrap().expose_secret(),
            "secret"
        );
        store.delete("host").unwrap();
        store.delete("host").unwrap();
        assert!(store.get("host").unwrap().is_none());
    }
}
