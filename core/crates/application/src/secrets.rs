use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use radroots_studio_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use secrecy::{ExposeSecret, SecretString};

pub trait SecretStore: Send + Sync {
    /// Stores a credential under its canonical public key without overwriting.
    ///
    /// # Errors
    ///
    /// Returns a safe duplicate or keyring error without exposing the credential.
    fn put(&self, public_key: PublicKey, secret: SecretKeyInput) -> Result<(), SafeError>;
    /// Loads a credential into a non-cloneable redacted boundary value.
    ///
    /// # Errors
    ///
    /// Returns a safe missing-credential or keyring error.
    fn load(&self, public_key: PublicKey) -> Result<SecretKeyInput, SafeError>;
    /// Reports whether a credential exists without exposing it.
    ///
    /// # Errors
    ///
    /// Returns a safe keyring error when availability cannot be determined.
    fn contains(&self, public_key: PublicKey) -> Result<bool, SafeError>;
    /// Deletes a credential without affecting public account metadata.
    ///
    /// # Errors
    ///
    /// Returns a safe missing-credential or keyring error.
    fn delete(&self, public_key: PublicKey) -> Result<(), SafeError>;
}

#[derive(Default)]
pub struct InMemorySecretStore {
    credentials: Mutex<BTreeMap<PublicKey, SecretString>>,
}

impl InMemorySecretStore {
    fn credentials(&self) -> MutexGuard<'_, BTreeMap<PublicKey, SecretString>> {
        self.credentials
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for InMemorySecretStore {
    fn put(&self, public_key: PublicKey, secret: SecretKeyInput) -> Result<(), SafeError> {
        let mut credentials = self.credentials();
        if credentials.contains_key(&public_key) {
            return Err(credential_exists());
        }
        let value = secret.with_exposed_secret(ToOwned::to_owned);
        credentials.insert(public_key, SecretString::from(value));
        Ok(())
    }

    fn load(&self, public_key: PublicKey) -> Result<SecretKeyInput, SafeError> {
        let credentials = self.credentials();
        let secret = credentials
            .get(&public_key)
            .ok_or_else(credential_missing)?;
        SecretKeyInput::parse(secret.expose_secret().to_owned()).map_err(|_| credential_missing())
    }

    fn contains(&self, public_key: PublicKey) -> Result<bool, SafeError> {
        Ok(self.credentials().contains_key(&public_key))
    }

    fn delete(&self, public_key: PublicKey) -> Result<(), SafeError> {
        self.credentials()
            .remove(&public_key)
            .map(|_| ())
            .ok_or_else(credential_missing)
    }
}

const fn credential_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountAlreadyExists,
        SafeMessage::new("The Nostr account credential already exists."),
    )
}

const fn credential_missing() -> SafeError {
    SafeError::new(
        SafeErrorCode::CredentialMissing,
        SafeMessage::new("The Nostr account credential is missing."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_domain::{PublicKey, SafeErrorCode, SecretKeyInput};

    use super::{InMemorySecretStore, SecretStore};

    const SECRET: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

    #[test]
    fn secret_store_puts_loads_checks_and_deletes_redacted_credentials() {
        let store = InMemorySecretStore::default();
        let public_key = PublicKey::from_bytes([1; 32]);
        assert!(!store.contains(public_key).expect("contains"));
        store
            .put(
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .expect("put");
        assert!(store.contains(public_key).expect("contains"));
        let loaded = store.load(public_key).expect("load");
        assert_eq!(loaded.with_exposed_secret(str::len), 64);
        assert!(!format!("{loaded:?}").contains(SECRET));
        store.delete(public_key).expect("delete");
        assert!(!store.contains(public_key).expect("contains"));
    }

    #[test]
    fn secret_store_rejects_duplicates_and_reports_missing_credentials() {
        let store = InMemorySecretStore::default();
        let public_key = PublicKey::from_bytes([2; 32]);
        let missing = store.load(public_key).expect_err("missing");
        assert_eq!(missing.code(), SafeErrorCode::CredentialMissing);
        store
            .put(
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .expect("put");
        let duplicate = store
            .put(
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .expect_err("duplicate");
        assert_eq!(duplicate.code(), SafeErrorCode::AccountAlreadyExists);
        store.delete(public_key).expect("delete");
        let missing = store.delete(public_key).expect_err("missing delete");
        assert_eq!(missing.code(), SafeErrorCode::CredentialMissing);
    }
}
