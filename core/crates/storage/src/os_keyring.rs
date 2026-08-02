use keyring::{Entry, Error as KeyringError};
use radroots_studio_application::SecretStore;
use radroots_studio_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};

pub const CREDENTIAL_SERVICE: &str = "org.radroots.studio.nostr";

#[derive(Clone, Copy, Debug, Default)]
pub struct OsKeyringSecretStore;

impl OsKeyringSecretStore {
    fn entry(public_key: PublicKey) -> Result<Entry, SafeError> {
        Entry::new(CREDENTIAL_SERVICE, &public_key.to_hex()).map_err(|_| keyring_unavailable())
    }
}

impl SecretStore for OsKeyringSecretStore {
    fn put(&self, public_key: PublicKey, secret: SecretKeyInput) -> Result<(), SafeError> {
        let entry = Self::entry(public_key)?;
        match entry.get_password() {
            Ok(_) => return Err(credential_exists()),
            Err(KeyringError::NoEntry) => {}
            Err(_) => return Err(keyring_unavailable()),
        }
        secret
            .with_exposed_secret(|value| entry.set_password(value))
            .map_err(|_| keyring_unavailable())
    }

    fn load(&self, public_key: PublicKey) -> Result<SecretKeyInput, SafeError> {
        let password = Self::entry(public_key)?
            .get_password()
            .map_err(|error| map_read_error(&error))?;
        SecretKeyInput::parse(password)
    }

    fn contains(&self, public_key: PublicKey) -> Result<bool, SafeError> {
        match Self::entry(public_key)?.get_password() {
            Ok(_) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(_) => Err(keyring_unavailable()),
        }
    }

    fn delete(&self, public_key: PublicKey) -> Result<(), SafeError> {
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

const fn keyring_unavailable() -> SafeError {
    SafeError::new(
        SafeErrorCode::KeyringUnavailable,
        SafeMessage::new("The operating system credential store is unavailable."),
    )
}

#[cfg(test)]
mod tests {
    use radroots_studio_application::SecretStore;
    use radroots_studio_domain::{PublicKey, SecretKeyInput};

    use super::{CREDENTIAL_SERVICE, OsKeyringSecretStore};

    #[test]
    fn keyring_coordinates_are_stable_and_public() {
        let public_key = PublicKey::from_bytes([0xab; 32]);
        assert_eq!(CREDENTIAL_SERVICE, "org.radroots.studio.nostr");
        assert_eq!(public_key.to_hex(), "ab".repeat(32));
    }

    #[test]
    #[ignore = "mutates the current user's operating-system credential store"]
    fn real_keyring_smoke_round_trips_and_deletes() {
        let store = OsKeyringSecretStore;
        let public_key = PublicKey::from_bytes([0xcd; 32]);
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
