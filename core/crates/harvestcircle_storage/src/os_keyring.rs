use std::sync::{Mutex, MutexGuard};

use harvestcircle_application::{BoxFuture, DurableRequestId, SecretStore};
use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use harvestcircle_product::KEYRING_SERVICE;
use zeroize::Zeroizing;

pub const CREDENTIAL_SERVICE: &str = KEYRING_SERVICE;

const CREDENTIAL_ENVELOPE_DOMAIN: &[u8] = b"harvestcircle.credential.v1\0";
#[cfg(target_os = "linux")]
const CREDENTIAL_OPERATION_ATTRIBUTE: &str = "harvestcircle-operation";
#[cfg(target_os = "linux")]
const CREDENTIAL_ACCOUNT_ATTRIBUTE: &str = "account";
#[cfg(target_os = "linux")]
const CREDENTIAL_SERVICE_ATTRIBUTE: &str = "service";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreateError {
    Existing,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadError {
    Missing,
    Unavailable,
}

#[derive(Default)]
pub struct OsKeyringSecretStore {
    operation_lock: Mutex<()>,
}

impl OsKeyringSecretStore {
    fn operation(&self) -> Result<MutexGuard<'_, ()>, SafeError> {
        self.operation_lock
            .lock()
            .map_err(|_| keyring_unavailable())
    }
}

impl SecretStore for OsKeyringSecretStore {
    fn put<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let _operation = self.operation()?;
            let account = public_key.to_hex();
            let encoded = encode_credential(request_id, &secret);
            match platform_create(&account, request_id, encoded.as_slice()) {
                Ok(()) => Ok(()),
                Err(CreateError::Existing) => {
                    let existing = Zeroizing::new(platform_read(&account).map_err(map_read_error)?);
                    verify_existing_replay(request_id, &secret, existing.as_slice())
                }
                Err(CreateError::Unavailable) => Err(keyring_unavailable()),
            }
        })
    }

    fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
        Box::pin(async move {
            let _operation = self.operation()?;
            let account = public_key.to_hex();
            let encoded = Zeroizing::new(platform_read(&account).map_err(map_read_error)?);
            decode_credential(encoded.as_slice()).map(|(_, secret)| secret)
        })
    }

    fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
        Box::pin(async move {
            let _operation = self.operation()?;
            let account = public_key.to_hex();
            match platform_read(&account) {
                Ok(encoded) => {
                    let encoded = Zeroizing::new(encoded);
                    decode_credential(encoded.as_slice())?;
                    Ok(true)
                }
                Err(ReadError::Missing) => Ok(false),
                Err(ReadError::Unavailable) => Err(keyring_unavailable()),
            }
        })
    }

    fn delete<'a>(
        &'a self,
        _request_id: &'a DurableRequestId,
        public_key: PublicKey,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let _operation = self.operation()?;
            let account = public_key.to_hex();
            platform_delete(&account).map_err(map_read_error)
        })
    }
}

fn encode_credential(request_id: &DurableRequestId, secret: &SecretKeyInput) -> Zeroizing<Vec<u8>> {
    secret.with_exposed_secret(|value| {
        let mut encoded = Zeroizing::new(Vec::with_capacity(
            CREDENTIAL_ENVELOPE_DOMAIN.len() + 36 + 1 + value.len(),
        ));
        encoded.extend_from_slice(CREDENTIAL_ENVELOPE_DOMAIN);
        encoded.extend_from_slice(request_id.as_str().as_bytes());
        encoded.push(0);
        encoded.extend_from_slice(value.as_bytes());
        encoded
    })
}

fn decode_credential(encoded: &[u8]) -> Result<(DurableRequestId, SecretKeyInput), SafeError> {
    let request_start = CREDENTIAL_ENVELOPE_DOMAIN.len();
    let request_end = request_start + 36;
    let secret_start = request_end + 1;
    if encoded.len() != secret_start + 64
        || !encoded.starts_with(CREDENTIAL_ENVELOPE_DOMAIN)
        || encoded.get(request_end) != Some(&0)
    {
        return Err(keyring_unavailable());
    }
    let request = std::str::from_utf8(&encoded[request_start..request_end])
        .map_err(|_| keyring_unavailable())?;
    let request = DurableRequestId::parse(request).map_err(|_| keyring_unavailable())?;
    let secret = SecretKeyInput::parse_bytes(encoded[secret_start..].to_vec())
        .map_err(|_| keyring_unavailable())?;
    Ok((request, secret))
}

fn verify_existing_replay(
    request_id: &DurableRequestId,
    secret: &SecretKeyInput,
    encoded: &[u8],
) -> Result<(), SafeError> {
    let (existing_request, existing_secret) = decode_credential(encoded)?;
    if existing_request == *request_id
        && existing_secret
            .with_exposed_secret(|value| secret.with_exposed_secret(|expected| value == expected))
    {
        Ok(())
    } else {
        Err(credential_exists())
    }
}

const fn map_read_error(error: ReadError) -> SafeError {
    match error {
        ReadError::Missing => credential_missing(),
        ReadError::Unavailable => keyring_unavailable(),
    }
}

#[cfg(target_os = "macos")]
fn platform_create(
    account: &str,
    _request_id: &DurableRequestId,
    secret: &[u8],
) -> Result<(), CreateError> {
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework_sys::base::errSecDuplicateItem;

    let keychain = SecKeychain::default().map_err(|_| CreateError::Unavailable)?;
    keychain
        .add_generic_password(CREDENTIAL_SERVICE, account, secret)
        .map_err(|error| {
            if error.code() == errSecDuplicateItem {
                CreateError::Existing
            } else {
                CreateError::Unavailable
            }
        })
}

#[cfg(target_os = "macos")]
fn platform_read(account: &str) -> Result<Vec<u8>, ReadError> {
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework_sys::base::errSecItemNotFound;

    let keychain = SecKeychain::default().map_err(|_| ReadError::Unavailable)?;
    keychain
        .find_generic_password(CREDENTIAL_SERVICE, account)
        .map(|(password, _item)| password.as_ref().to_vec())
        .map_err(|error| {
            if error.code() == errSecItemNotFound {
                ReadError::Missing
            } else {
                ReadError::Unavailable
            }
        })
}

#[cfg(target_os = "macos")]
fn platform_delete(account: &str) -> Result<(), ReadError> {
    use security_framework::item::{ItemClass, ItemSearchOptions};
    use security_framework_sys::base::errSecItemNotFound;

    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service(CREDENTIAL_SERVICE)
        .account(account);
    match query.delete() {
        Ok(()) => Ok(()),
        Err(error) if error.code() == errSecItemNotFound => Err(ReadError::Missing),
        Err(_) => Err(ReadError::Unavailable),
    }
}

#[cfg(target_os = "linux")]
fn linux_service() -> Result<secret_service::blocking::SecretService<'static>, ReadError> {
    use secret_service::EncryptionType;
    use secret_service::blocking::SecretService;

    SecretService::connect(EncryptionType::Dh).map_err(|_| ReadError::Unavailable)
}

#[cfg(target_os = "linux")]
fn linux_items<'a>(
    service: &'a secret_service::blocking::SecretService<'a>,
    account: &'a str,
) -> Result<Vec<secret_service::blocking::Item<'a>>, ReadError> {
    let attributes = std::collections::HashMap::from([
        (CREDENTIAL_SERVICE_ATTRIBUTE, CREDENTIAL_SERVICE),
        (CREDENTIAL_ACCOUNT_ATTRIBUTE, account),
    ]);
    let mut result = service
        .search_items(attributes)
        .map_err(|_| ReadError::Unavailable)?;
    if !result.locked.is_empty() {
        let locked = result.locked.iter().collect::<Vec<_>>();
        service
            .unlock_all(&locked)
            .map_err(|_| ReadError::Unavailable)?;
        result.unlocked.append(&mut result.locked);
    }
    Ok(result.unlocked)
}

#[cfg(target_os = "linux")]
fn platform_create(
    account: &str,
    request_id: &DurableRequestId,
    secret: &[u8],
) -> Result<(), CreateError> {
    let service = linux_service().map_err(|_| CreateError::Unavailable)?;
    let existing = linux_items(&service, account).map_err(|_| CreateError::Unavailable)?;
    if !existing.is_empty() {
        return Err(CreateError::Existing);
    }

    let collection = service
        .get_default_collection()
        .map_err(|_| CreateError::Unavailable)?;
    collection
        .ensure_unlocked()
        .map_err(|_| CreateError::Unavailable)?;
    let attributes = std::collections::HashMap::from([
        (CREDENTIAL_SERVICE_ATTRIBUTE, CREDENTIAL_SERVICE),
        (CREDENTIAL_ACCOUNT_ATTRIBUTE, account),
        (CREDENTIAL_OPERATION_ATTRIBUTE, request_id.as_str()),
    ]);
    let created = collection
        .create_item(
            "HarvestCircle Nostr identity",
            attributes,
            secret,
            false,
            "application/octet-stream",
        )
        .map_err(|_| CreateError::Unavailable)?;

    let all = linux_items(&service, account).map_err(|_| CreateError::Unavailable)?;
    if all.len() == 1 && all.first() == Some(&created) {
        Ok(())
    } else {
        let _ = created.delete();
        Err(CreateError::Existing)
    }
}

#[cfg(target_os = "linux")]
fn platform_read(account: &str) -> Result<Vec<u8>, ReadError> {
    let service = linux_service()?;
    let mut items = linux_items(&service, account)?;
    if items.is_empty() {
        return Err(ReadError::Missing);
    }
    if items.len() != 1 {
        return Err(ReadError::Unavailable);
    }
    items
        .pop()
        .expect("single item checked")
        .get_secret()
        .map_err(|_| ReadError::Unavailable)
}

#[cfg(target_os = "linux")]
fn platform_delete(account: &str) -> Result<(), ReadError> {
    let service = linux_service()?;
    let mut items = linux_items(&service, account)?;
    if items.is_empty() {
        return Err(ReadError::Missing);
    }
    if items.len() != 1 {
        return Err(ReadError::Unavailable);
    }
    items
        .pop()
        .expect("single item checked")
        .delete()
        .map_err(|_| ReadError::Unavailable)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_create(
    _account: &str,
    _request_id: &DurableRequestId,
    _secret: &[u8],
) -> Result<(), CreateError> {
    Err(CreateError::Unavailable)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_read(_account: &str) -> Result<Vec<u8>, ReadError> {
    Err(ReadError::Unavailable)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn platform_delete(_account: &str) -> Result<(), ReadError> {
    Err(ReadError::Unavailable)
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
    use harvestcircle_application::{DurableRequestId, SecretStore};
    use harvestcircle_domain::{PublicKey, SafeErrorCode, SecretKeyInput};

    use super::{
        CREDENTIAL_ENVELOPE_DOMAIN, CREDENTIAL_SERVICE, OsKeyringSecretStore, decode_credential,
        encode_credential, verify_existing_replay,
    };

    const SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn request_id() -> DurableRequestId {
        DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000249").expect("request")
    }

    fn public_key() -> PublicKey {
        PublicKey::from_hex("7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7")
            .expect("valid public key")
    }

    #[test]
    fn credential_envelope_binds_uuidv7_operation_and_secret() {
        let secret = SecretKeyInput::parse(SECRET.to_owned()).expect("secret");
        let encoded = encode_credential(&request_id(), &secret);
        assert_eq!(
            encoded.len(),
            CREDENTIAL_ENVELOPE_DOMAIN.len() + 36 + 1 + 64
        );

        let (operation, decoded) = decode_credential(encoded.as_slice()).expect("decode");
        assert_eq!(operation, request_id());
        assert!(decoded.with_exposed_secret(|value| value == SECRET));
    }

    #[test]
    fn malformed_credential_envelopes_fail_closed() {
        let secret = SecretKeyInput::parse(SECRET.to_owned()).expect("secret");
        let encoded = encode_credential(&request_id(), &secret);
        for candidate in [
            encoded[..encoded.len() - 1].to_vec(),
            {
                let mut value = encoded.to_vec();
                value[0] ^= 1;
                value
            },
            {
                let mut value = encoded.to_vec();
                value[CREDENTIAL_ENVELOPE_DOMAIN.len() + 14] = b'4';
                value
            },
            {
                let mut value = encoded.to_vec();
                value[CREDENTIAL_ENVELOPE_DOMAIN.len() + 36] = b'x';
                value
            },
        ] {
            let error = match decode_credential(&candidate) {
                Err(error) => error,
                Ok(_) => panic!("malformed envelope was accepted"),
            };
            assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        }
    }

    #[test]
    fn only_exact_same_operation_replay_is_idempotent() {
        let secret = SecretKeyInput::parse(SECRET.to_owned()).expect("secret");
        let encoded = encode_credential(&request_id(), &secret);
        assert!(verify_existing_replay(&request_id(), &secret, &encoded).is_ok());

        let another_request =
            DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000250").expect("request");
        let request_conflict = verify_existing_replay(&another_request, &secret, &encoded)
            .expect_err("another operation must conflict");
        assert_eq!(
            request_conflict.code(),
            SafeErrorCode::IdentityAlreadyExists
        );

        let another_secret = SecretKeyInput::parse(
            "0000000000000000000000000000000000000000000000000000000000000002".to_owned(),
        )
        .expect("secret");
        let secret_conflict = verify_existing_replay(&request_id(), &another_secret, &encoded)
            .expect_err("same operation cannot change the secret");
        assert_eq!(secret_conflict.code(), SafeErrorCode::IdentityAlreadyExists);
    }

    #[test]
    fn keyring_coordinates_are_stable_and_public() {
        assert_eq!(CREDENTIAL_SERVICE, "org.harvestcircle.desktop.nostr");
        assert_eq!(
            public_key().to_hex(),
            "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7"
        );
    }

    #[tokio::test]
    async fn poisoned_operation_lock_fails_closed_before_keyring_access() {
        let store = OsKeyringSecretStore::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _operation = store.operation_lock.lock().expect("operation lock");
            panic!("injected operation failure");
        }));
        assert!(panic.is_err());

        let error = store
            .contains(public_key())
            .await
            .expect_err("poison must reject");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
    }

    #[tokio::test]
    #[ignore = "mutates the current user's operating-system credential store"]
    async fn real_keyring_smoke_round_trips_and_deletes() {
        let store = OsKeyringSecretStore::default();
        let request_id = request_id();
        let _ = store.delete(&request_id, public_key()).await;
        store
            .put(
                &request_id,
                public_key(),
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("keyring put");
        assert!(
            store
                .contains(public_key())
                .await
                .expect("keyring contains")
        );
        let loaded = store.load(public_key()).await.expect("keyring load");
        assert!(loaded.with_exposed_secret(|value| value == SECRET));
        store
            .delete(&request_id, public_key())
            .await
            .expect("keyring delete");
    }
}
