//! The Windows Credential Manager as a [`SecretStore`] (only the refresh token goes there).
//!
//! Entries are created with **local** persistence: the store's default is *Enterprise*, which
//! roams the credential with the Windows profile. A credential blob is limited to 2560 bytes.

use std::sync::Arc;

use medvita_platform::{MemoryStore, SecretStore};

/// Largest secret the Windows Credential Manager accepts (CRED_MAX_CREDENTIAL_BLOB_SIZE).
pub const MAX_SECRET_BYTES: usize = 5 * 512;

/// The store the app uses: the Windows vault, or memory where there is none (other platforms).
pub fn default_store() -> Arc<dyn SecretStore> {
    #[cfg(windows)]
    {
        match windows::KeyringStore::new() {
            Ok(store) => return Arc::new(store),
            Err(error) => {
                tracing::error!(%error, "Windows credential vault unavailable; using memory")
            }
        }
    }
    Arc::new(MemoryStore::new())
}

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::sync::Once;

    use medvita_platform::secret::VAULT_SERVICE;
    use medvita_platform::{SecretStore, SecretStoreError};
    use secrecy::{ExposeSecret, SecretString};

    use super::MAX_SECRET_BYTES;

    pub struct KeyringStore;

    static INIT: Once = Once::new();

    impl KeyringStore {
        pub fn new() -> Result<Self, SecretStoreError> {
            let mut result = Ok(());
            INIT.call_once(|| {
                result = windows_native_keyring_store::Store::new()
                    .map(|store| keyring_core::set_default_store(store))
                    .map_err(|error| SecretStoreError::Unavailable(error.to_string()));
            });
            result.map(|()| Self)
        }

        fn entry(account: &str) -> Result<keyring_core::Entry, SecretStoreError> {
            // "local": stays on this computer (the store's default, Enterprise, roams).
            let modifiers = HashMap::from([("persistence", "local")]);
            keyring_core::Entry::new_with_modifiers(VAULT_SERVICE, account, &modifiers)
                .map_err(other)
        }
    }

    fn other(error: keyring_core::Error) -> SecretStoreError {
        SecretStoreError::Other(error.to_string())
    }

    impl SecretStore for KeyringStore {
        fn get(&self, account: &str) -> Result<Option<SecretString>, SecretStoreError> {
            match Self::entry(account)?.get_password() {
                Ok(secret) => Ok(Some(SecretString::from(secret))),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(error) => Err(other(error)),
            }
        }

        fn set(&self, account: &str, secret: &SecretString) -> Result<(), SecretStoreError> {
            if secret.expose_secret().len() > MAX_SECRET_BYTES {
                return Err(SecretStoreError::TooLarge);
            }
            Self::entry(account)?
                .set_password(secret.expose_secret())
                .map_err(other)
        }

        fn delete(&self, account: &str) -> Result<(), SecretStoreError> {
            match Self::entry(account)?.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(error) => Err(other(error)),
            }
        }
    }
}
