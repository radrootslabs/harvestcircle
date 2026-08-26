use std::sync::{Mutex, MutexGuard};

use harvestcircle_application::SecretStore;
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use harvestcircle_product::KEYRING_SERVICE;
use keyring::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

pub const CREDENTIAL_SERVICE: &str = KEYRING_SERVICE;

#[derive(Default)]
pub struct OsKeyringSecretStore {
    operation_lock: Mutex<()>,
}

impl OsKeyringSecretStore {
    fn entry(public_key: PublicKey) -> Result<Entry, SafeError> {
        Entry::new(CREDENTIAL_SERVICE, &public_key.to_hex()).map_err(|_| keyring_unavailable())
    }

    fn operation(&self) -> Result<MutexGuard<'_, ()>, SafeError> {
        self.operation_lock
            .lock()
            .map_err(|_| keyring_unavailable())
    }
}

impl SecretStore for OsKeyringSecretStore {
    fn put(&self, public_key: PublicKey, secret: SecretKeyInput) -> Result<(), SafeError> {
        let _operation = self.operation()?;
        let entry = Self::entry(public_key)?;
        match entry.get_password() {
            Ok(password) => {
                drop(Zeroizing::new(password));
                return Err(credential_exists());
            }
            Err(KeyringError::NoEntry) => {}
            Err(_) => return Err(keyring_unavailable()),
        }
        secret
            .with_exposed_secret(|value| entry.set_password(value))
            .map_err(|_| keyring_unavailable())
    }

    fn load(&self, public_key: PublicKey) -> Result<SecretKeyInput, SafeError> {
        let _operation = self.operation()?;
        let password = Self::entry(public_key)?
            .get_password()
            .map_err(|error| map_read_error(&error))?;
        SecretKeyInput::parse(password)
    }

    fn contains(&self, public_key: PublicKey) -> Result<bool, SafeError> {
        let _operation = self.operation()?;
        match Self::entry(public_key)?.get_password() {
            Ok(password) => {
                drop(Zeroizing::new(password));
                Ok(true)
            }
            Err(KeyringError::NoEntry) => Ok(false),
            Err(_) => Err(keyring_unavailable()),
        }
    }

    fn delete(&self, public_key: PublicKey) -> Result<(), SafeError> {
        let _operation = self.operation()?;
        Self::entry(public_key)?
            .delete_credential()
            .map_err(|error| map_read_error(&error))
    }
}

const fn map_read_error(error: &KeyringError) -> SafeError {
    match error {
        KeyringError::NoEntry => credential_missing(),
        _ => keyring_unavailable(),
    }
}

const fn credential_exists() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityAlreadyExists,
        SafeMessage::new("The Nostr identity credential already exists."),
    )
}

const fn credential_missing() -> SafeError {
    SafeError::new(
        SafeErrorCode::CredentialMissing,
        SafeMessage::new("The Nostr identity credential is missing."),
    )
}

const fn keyring_unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::KeyringUnavailable,
        SafeMessage::new("The operating system credential store is unavailable."),
    )
}

#[cfg(test)]
mod tests {
    use harvestcircle_application::SecretStore;
    use harvestcircle_domain::{PublicKey, SafeErrorCode, SecretKeyInput};

    use super::{CREDENTIAL_SERVICE, OsKeyringSecretStore};

    #[test]
    fn keyring_coordinates_are_stable_and_public() {
        let public_key =
            PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
                .expect("valid public key");
        assert_eq!(CREDENTIAL_SERVICE, "org.harvestcircle.desktop.nostr");
        assert_eq!(
            public_key.to_hex(),
            "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"
        );
    }

    #[test]
    fn poisoned_operation_lock_fails_closed_before_keyring_access() {
        let store = OsKeyringSecretStore::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _operation = store.operation_lock.lock().expect("operation lock");
            panic!("injected operation failure");
        }));
        assert!(panic.is_err());

        let public_key =
            PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
                .expect("valid public key");
        let error = store.contains(public_key).expect_err("poison must reject");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
    }

    #[test]
    #[ignore = "mutates the current user's operating-system credential store"]
    fn real_keyring_smoke_round_trips_and_deletes() {
        let store = OsKeyringSecretStore::default();
        let public_key =
            PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
                .expect("valid public key");
        let _ = store.delete(public_key);
        store
            .put(
                public_key,
                SecretKeyInput::parse("11".repeat(32)).expect("secret"),
            )
            .expect("keyring put");
        assert!(store.contains(public_key).expect("keyring contains"));
        let loaded = store.load(public_key).expect("keyring load");
        assert_eq!(loaded.with_exposed_secret(str::len), 64);
        store.delete(public_key).expect("keyring delete");
    }
}
