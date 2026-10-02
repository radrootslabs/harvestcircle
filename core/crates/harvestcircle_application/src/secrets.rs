use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput};
use secrecy::{ExposeSecret, SecretString};

use crate::{BoxFuture, DurableRequestId};

pub trait SecretStore: Send + Sync {
    /// Stores a credential under its canonical public key without overwriting.
    ///
    /// # Errors
    ///
    /// Returns a safe duplicate or keyring error without exposing the credential.
    fn put<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>>;
    /// Verifies the original request and full canonical secret without changing custody.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict, missing-credential, or keyring error. Adapters without
    /// request-bound verification fail closed rather than loading an unbound credential.
    fn verify<'a>(
        &'a self,
        _request_id: &'a DurableRequestId,
        _public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            drop(secret);
            Err(keyring_unavailable())
        })
    }
    /// Loads a credential into a non-cloneable redacted boundary value.
    ///
    /// # Errors
    ///
    /// Returns a safe missing-credential or keyring error.
    fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>>;
    /// Reports whether a credential exists without exposing it.
    ///
    /// # Errors
    ///
    /// Returns a safe keyring error when availability cannot be determined.
    fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>>;
    /// Deletes a credential without affecting public identity metadata.
    ///
    /// # Errors
    ///
    /// Returns a safe missing-credential or keyring error.
    fn delete<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
    ) -> BoxFuture<'a, Result<(), SafeError>>;
}

#[derive(Default)]
pub struct InMemorySecretStore {
    credentials: Mutex<BTreeMap<PublicKey, StoredCredential>>,
}

struct StoredCredential {
    request_id: DurableRequestId,
    secret: SecretString,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SecretStoreOperation {
    Put,
    Verify,
    Load,
    Contains,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecretStoreCall {
    operation: SecretStoreOperation,
    public_key: PublicKey,
}

impl SecretStoreCall {
    #[must_use]
    pub const fn operation(self) -> SecretStoreOperation {
        self.operation
    }

    #[must_use]
    pub const fn public_key(self) -> PublicKey {
        self.public_key
    }
}

#[derive(Default)]
pub struct FailureSecretStore {
    inner: InMemorySecretStore,
    remaining_failures: Mutex<BTreeMap<SecretStoreOperation, usize>>,
    calls: Mutex<Vec<SecretStoreCall>>,
}

impl FailureSecretStore {
    pub fn fail_next(&self, operation: SecretStoreOperation) {
        if let Ok(mut failures) = self.remaining_failures.lock() {
            *failures.entry(operation).or_default() += 1;
        }
    }

    #[must_use]
    pub fn calls(&self) -> Vec<SecretStoreCall> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    fn record_and_should_fail(
        &self,
        operation: SecretStoreOperation,
        public_key: PublicKey,
    ) -> bool {
        let Ok(mut calls) = self.calls.lock() else {
            return true;
        };
        calls.push(SecretStoreCall {
            operation,
            public_key,
        });
        drop(calls);
        let Ok(mut failures) = self.remaining_failures.lock() else {
            return true;
        };
        let remaining = failures.entry(operation).or_default();
        let should_fail = *remaining > 0;
        *remaining = remaining.saturating_sub(1);
        should_fail
    }
}

impl SecretStore for FailureSecretStore {
    fn put<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            if self.record_and_should_fail(SecretStoreOperation::Put, public_key) {
                return Err(keyring_unavailable());
            }
            self.inner.put(request_id, public_key, secret).await
        })
    }

    fn verify<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            if self.record_and_should_fail(SecretStoreOperation::Verify, public_key) {
                return Err(keyring_unavailable());
            }
            self.inner.verify(request_id, public_key, secret).await
        })
    }

    fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
        Box::pin(async move {
            if self.record_and_should_fail(SecretStoreOperation::Load, public_key) {
                return Err(keyring_unavailable());
            }
            self.inner.load(public_key).await
        })
    }

    fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
        Box::pin(async move {
            if self.record_and_should_fail(SecretStoreOperation::Contains, public_key) {
                return Err(keyring_unavailable());
            }
            self.inner.contains(public_key).await
        })
    }

    fn delete<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            if self.record_and_should_fail(SecretStoreOperation::Delete, public_key) {
                return Err(keyring_unavailable());
            }
            self.inner.delete(request_id, public_key).await
        })
    }
}

impl InMemorySecretStore {
    fn credentials(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<PublicKey, StoredCredential>>, SafeError> {
        self.credentials.lock().map_err(|_| keyring_unavailable())
    }
}

impl SecretStore for InMemorySecretStore {
    fn put<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let mut credentials = self.credentials()?;
            if credentials.contains_key(&public_key) {
                return Err(credential_exists());
            }
            let value = secret.with_exposed_secret(ToOwned::to_owned);
            credentials.insert(
                public_key,
                StoredCredential {
                    request_id: request_id.clone(),
                    secret: SecretString::from(value),
                },
            );
            Ok(())
        })
    }

    fn verify<'a>(
        &'a self,
        request_id: &'a DurableRequestId,
        public_key: PublicKey,
        secret: SecretKeyInput,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            let credentials = self.credentials()?;
            let existing = credentials
                .get(&public_key)
                .ok_or_else(credential_missing)?;
            if existing.request_id != *request_id
                || !secret
                    .with_exposed_secret(|expected| existing.secret.expose_secret() == expected)
            {
                return Err(replay_conflict());
            }
            Ok(())
        })
    }

    fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
        Box::pin(async move {
            let credentials = self.credentials()?;
            let secret = credentials
                .get(&public_key)
                .ok_or_else(credential_missing)?;
            SecretKeyInput::parse(secret.secret.expose_secret().to_owned())
                .map_err(|_| credential_missing())
        })
    }

    fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
        Box::pin(async move { Ok(self.credentials()?.contains_key(&public_key)) })
    }

    fn delete<'a>(
        &'a self,
        _request_id: &'a DurableRequestId,
        public_key: PublicKey,
    ) -> BoxFuture<'a, Result<(), SafeError>> {
        Box::pin(async move {
            self.credentials()?
                .remove(&public_key)
                .map(|_| ())
                .ok_or_else(credential_missing)
        })
    }
}

const fn replay_conflict() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The identity operation conflicts with the stored credential."),
    )
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
    use crate::{BoxFuture, DurableRequestId};

    use harvestcircle_domain::{PublicKey, SafeError, SafeErrorCode, SecretKeyInput};

    use super::{FailureSecretStore, InMemorySecretStore, SecretStore, SecretStoreOperation};

    const SECRET: &str = "7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";

    fn request_id() -> DurableRequestId {
        DurableRequestId::parse("01890f3e-7b1c-7000-8000-000000000301").expect("request")
    }

    struct UnverifiedSecretStore(InMemorySecretStore);

    impl SecretStore for UnverifiedSecretStore {
        fn put<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            public_key: PublicKey,
            secret: SecretKeyInput,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.0.put(request_id, public_key, secret)
        }

        fn load(&self, public_key: PublicKey) -> BoxFuture<'_, Result<SecretKeyInput, SafeError>> {
            self.0.load(public_key)
        }

        fn contains(&self, public_key: PublicKey) -> BoxFuture<'_, Result<bool, SafeError>> {
            self.0.contains(public_key)
        }

        fn delete<'a>(
            &'a self,
            request_id: &'a DurableRequestId,
            public_key: PublicKey,
        ) -> BoxFuture<'a, Result<(), SafeError>> {
            self.0.delete(request_id, public_key)
        }
    }

    #[tokio::test]
    async fn default_secret_verification_fails_closed_through_object_safe_port() {
        let store = UnverifiedSecretStore(InMemorySecretStore::default());
        let port: &dyn SecretStore = &store;
        let public_key = PublicKey::from_bytes([7; 32]).expect("public key");
        port.put(
            &request_id(),
            public_key,
            SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
        )
        .await
        .expect("put");
        let error = port
            .verify(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("unbound adapter must fail closed");
        let retained = port.load(public_key).await.expect("retained credential");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        assert!(retained.with_exposed_secret(|value| value == SECRET));
        assert!(!format!("{error:?}").contains(SECRET));
    }

    #[tokio::test]
    async fn memory_secret_verification_binds_request_and_full_secret_without_mutation() {
        let store = InMemorySecretStore::default();
        let request = request_id();
        let another_request = DurableRequestId::new_v7();
        let public_key = PublicKey::from_bytes([7; 32]).expect("public key");
        store
            .put(
                &request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        let exact = store
            .verify(
                &request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await;
        let changed_request = store
            .verify(
                &another_request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("original request required");
        let changed_secret = store
            .verify(
                &request,
                public_key,
                SecretKeyInput::parse(
                    "0000000000000000000000000000000000000000000000000000000000000001".to_owned(),
                )
                .expect("different secret"),
            )
            .await
            .expect_err("full secret required");
        let missing = store
            .verify(
                &request,
                PublicKey::from_bytes([8; 32]).expect("other public key"),
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("missing credential");
        let retained = store.load(public_key).await.expect("retained credential");
        let still_exact = store
            .verify(
                &request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await;
        assert!(exact.is_ok());
        assert!(still_exact.is_ok());
        assert_eq!(
            changed_request.code(),
            SafeErrorCode::InvalidApplicationState
        );
        assert_eq!(
            changed_secret.code(),
            SafeErrorCode::InvalidApplicationState
        );
        assert_eq!(missing.code(), SafeErrorCode::CredentialMissing);
        assert!(retained.with_exposed_secret(|value| value == SECRET));
        assert_eq!(store.credentials().expect("credentials").len(), 1);
        assert!(!format!("{changed_request:?} {changed_secret:?} {missing:?}").contains(SECRET));
    }

    #[tokio::test]
    async fn memory_secret_verification_fails_closed_on_poison() {
        let store = InMemorySecretStore::default();
        let public_key = PublicKey::from_bytes([7; 32]).expect("public key");
        store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _credentials = store.credentials.lock().expect("credentials lock");
            panic!("injected custody failure");
        }));
        let error = store
            .verify(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("poison must fail closed");
        assert!(panic.is_err());
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        assert!(!format!("{error:?}").contains(SECRET));
    }

    #[tokio::test]
    async fn failure_secret_verification_audits_only_public_identity_and_preserves_custody() {
        let store = FailureSecretStore::default();
        let request = request_id();
        let public_key = PublicKey::from_bytes([7; 32]).expect("public key");
        store
            .put(
                &request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        store.fail_next(SecretStoreOperation::Verify);
        let unavailable = store
            .verify(
                &request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("injected verification failure");
        let exact = store
            .verify(
                &request,
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await;
        let conflict = store
            .verify(
                &DurableRequestId::new_v7(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("request mismatch");
        let retained = store
            .inner
            .load(public_key)
            .await
            .expect("retained credential");
        let calls = store.calls();
        assert_eq!(unavailable.code(), SafeErrorCode::KeyringUnavailable);
        assert!(exact.is_ok());
        assert_eq!(conflict.code(), SafeErrorCode::InvalidApplicationState);
        assert!(retained.with_exposed_secret(|value| value == SECRET));
        assert_eq!(
            calls
                .iter()
                .map(|call| call.operation())
                .collect::<Vec<_>>(),
            vec![
                SecretStoreOperation::Put,
                SecretStoreOperation::Verify,
                SecretStoreOperation::Verify,
                SecretStoreOperation::Verify,
            ]
        );
        assert!(calls.iter().all(|call| call.public_key() == public_key));
        let public_evidence = format!("{calls:?} {unavailable:?} {conflict:?}");
        assert!(!public_evidence.contains(SECRET));
        assert!(!public_evidence.contains(request.as_str()));
    }

    #[tokio::test]
    async fn secret_store_puts_loads_checks_and_deletes_redacted_credentials() {
        let store = InMemorySecretStore::default();
        let public_key = PublicKey::from_bytes([7; 32]).expect("valid public key");
        assert!(!store.contains(public_key).await.expect("contains"));
        store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        assert!(store.contains(public_key).await.expect("contains"));
        let loaded = store.load(public_key).await.expect("load");
        assert_eq!(loaded.with_exposed_secret(str::len), 64);
        store
            .delete(&request_id(), public_key)
            .await
            .expect("delete");
        assert!(!store.contains(public_key).await.expect("contains"));
    }

    #[tokio::test]
    async fn secret_store_rejects_duplicates_and_reports_missing_credentials() {
        let store = InMemorySecretStore::default();
        let public_key = PublicKey::from_bytes([7; 32]).expect("valid public key");
        let Err(missing) = store.load(public_key).await else {
            panic!("missing credential was returned");
        };
        assert_eq!(missing.code(), SafeErrorCode::CredentialMissing);
        store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        let duplicate = store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("duplicate");
        assert_eq!(duplicate.code(), SafeErrorCode::IdentityAlreadyExists);
        store
            .delete(&request_id(), public_key)
            .await
            .expect("delete");
        let missing = store
            .delete(&request_id(), public_key)
            .await
            .expect_err("missing delete");
        assert_eq!(missing.code(), SafeErrorCode::CredentialMissing);
    }

    #[tokio::test]
    async fn failure_secret_store_injects_each_boundary_without_mutating_state() {
        let store = FailureSecretStore::default();
        let public_key = PublicKey::from_bytes([7; 32]).expect("valid public key");
        store.fail_next(SecretStoreOperation::Put);
        let error = store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect_err("put failure");
        assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        assert!(!store.contains(public_key).await.expect("not written"));

        store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        for operation in [
            SecretStoreOperation::Verify,
            SecretStoreOperation::Load,
            SecretStoreOperation::Contains,
            SecretStoreOperation::Delete,
        ] {
            store.fail_next(operation);
            let error = match operation {
                SecretStoreOperation::Verify => {
                    store
                        .verify(
                            &request_id(),
                            public_key,
                            SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
                        )
                        .await
                }
                SecretStoreOperation::Load => store.load(public_key).await.map(|_| ()),
                SecretStoreOperation::Contains => store.contains(public_key).await.map(|_| ()),
                SecretStoreOperation::Delete => store.delete(&request_id(), public_key).await,
                SecretStoreOperation::Put => unreachable!("put tested separately"),
            }
            .expect_err("injected failure");
            assert_eq!(error.code(), SafeErrorCode::KeyringUnavailable);
        }
        assert!(
            store
                .contains(public_key)
                .await
                .expect("credential retained")
        );
    }

    #[tokio::test]
    async fn failure_secret_store_call_log_contains_only_public_identity() {
        let store = FailureSecretStore::default();
        let public_key = PublicKey::from_bytes([7; 32]).expect("valid public key");
        store
            .put(
                &request_id(),
                public_key,
                SecretKeyInput::parse(SECRET.to_owned()).expect("secret"),
            )
            .await
            .expect("put");
        let calls = store.calls();
        assert_eq!(calls[0].operation(), SecretStoreOperation::Put);
        assert_eq!(calls[0].public_key(), public_key);
        assert!(!format!("{calls:?}").contains(SECRET));
    }
}
