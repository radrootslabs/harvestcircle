use std::future::Future;
use std::pin::Pin;

use radroots_studio_domain::{
    AccountSummary, Kind0ProfileCandidate, PublicKey, RelayUrl, SafeError, UnixTimestamp,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileRefreshStatus {
    Success,
    Offline,
    InvalidData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedProfile {
    candidate: Kind0ProfileCandidate,
    refreshed_at: UnixTimestamp,
    refresh_status: ProfileRefreshStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountPreferenceKey {
    NamespaceProbe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountOperationKind {
    Add,
    Import,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountOperationPhase {
    IntentRecorded,
    CredentialWritten,
    MetadataCommitted,
    CompensationPending,
    CredentialDeleted,
    MetadataDeleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationDiagnostic {
    StorageUnavailable,
    KeyringUnavailable,
    CredentialMissing,
    CompensationFailed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(u64);

impl OperationId {
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingAccountOperation {
    id: OperationId,
    kind: AccountOperationKind,
    subject: PublicKey,
    phase: AccountOperationPhase,
    updated_at: UnixTimestamp,
    diagnostic: Option<OperationDiagnostic>,
}

impl PendingAccountOperation {
    #[must_use]
    pub const fn new(
        id: OperationId,
        kind: AccountOperationKind,
        subject: PublicKey,
        phase: AccountOperationPhase,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> Self {
        Self {
            id,
            kind,
            subject,
            phase,
            updated_at,
            diagnostic,
        }
    }

    #[must_use]
    pub const fn id(&self) -> OperationId {
        self.id
    }
    #[must_use]
    pub const fn kind(&self) -> AccountOperationKind {
        self.kind
    }
    #[must_use]
    pub const fn subject(&self) -> PublicKey {
        self.subject
    }
    #[must_use]
    pub const fn phase(&self) -> AccountOperationPhase {
        self.phase
    }
    #[must_use]
    pub const fn updated_at(&self) -> UnixTimestamp {
        self.updated_at
    }
    #[must_use]
    pub const fn diagnostic(&self) -> Option<OperationDiagnostic> {
        self.diagnostic
    }
}

impl CachedProfile {
    #[must_use]
    pub const fn new(
        candidate: Kind0ProfileCandidate,
        refreshed_at: UnixTimestamp,
        refresh_status: ProfileRefreshStatus,
    ) -> Self {
        Self {
            candidate,
            refreshed_at,
            refresh_status,
        }
    }

    #[must_use]
    pub const fn candidate(&self) -> &Kind0ProfileCandidate {
        &self.candidate
    }

    #[must_use]
    pub const fn refreshed_at(&self) -> UnixTimestamp {
        self.refreshed_at
    }

    #[must_use]
    pub const fn refresh_status(&self) -> ProfileRefreshStatus {
        self.refresh_status
    }
}

pub trait AccountRepository: Send + Sync {
    /// Lists saved public account records in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when records cannot be read.
    fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError>;
    /// Finds one saved public account record.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the lookup cannot complete.
    fn find_account(&self, public_key: PublicKey) -> Result<Option<AccountSummary>, SafeError>;
    /// Inserts one public account record.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the durable write fails.
    fn insert_account(&self, account: &AccountSummary) -> Result<(), SafeError>;
    /// Updates one existing public account record.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or account-not-found error when the durable
    /// update cannot complete.
    fn update_account(&self, account: &AccountSummary) -> Result<(), SafeError>;
    /// Removes one public account record.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the durable delete fails.
    fn remove_account(&self, public_key: PublicKey) -> Result<(), SafeError>;
}

pub trait ProfileRepository: Send + Sync {
    /// Loads cached public profile metadata.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the cache cannot be read.
    fn load_profile(&self, public_key: PublicKey) -> Result<Option<CachedProfile>, SafeError>;
    /// Saves a verified kind-0 profile candidate.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the cache cannot be committed.
    fn save_profile(&self, profile: &CachedProfile) -> Result<(), SafeError>;
    /// Records the result of a profile refresh without replacing cached metadata.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the cache cannot be committed.
    fn record_refresh_status(
        &self,
        public_key: PublicKey,
        refreshed_at: UnixTimestamp,
        status: ProfileRefreshStatus,
    ) -> Result<(), SafeError>;
    /// Removes cached profile metadata for an account.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the cache cannot be deleted.
    fn remove_profile(&self, public_key: PublicKey) -> Result<(), SafeError>;
}

pub trait AccountNamespaceRepository: Send + Sync {
    /// Reads one internal non-secret account-scoped value.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the value cannot be read.
    fn get_value(
        &self,
        owner: PublicKey,
        key: AccountPreferenceKey,
    ) -> Result<Option<String>, SafeError>;
    /// Writes one internal non-secret account-scoped value.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the value cannot be committed.
    fn set_value(
        &self,
        owner: PublicKey,
        key: AccountPreferenceKey,
        value: &str,
    ) -> Result<(), SafeError>;
    /// Removes all internal values owned by an account.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when cleanup cannot be committed.
    fn clear_owner(&self, owner: PublicKey) -> Result<(), SafeError>;
}

pub trait AppStateRepository: Send + Sync {
    /// Loads the persisted selected account.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when application state cannot be read.
    fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError>;
    /// Persists the selected account or the empty selection.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when application state cannot be committed.
    fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError>;
}

pub trait OperationJournal: Send + Sync {
    /// Records one cross-resource account operation intent.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the entry cannot be committed.
    fn begin_operation(
        &self,
        kind: AccountOperationKind,
        subject: PublicKey,
        updated_at: UnixTimestamp,
    ) -> Result<OperationId, SafeError>;
    /// Advances an operation to a durable recovery phase.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the entry cannot be updated.
    fn update_operation(
        &self,
        id: OperationId,
        phase: AccountOperationPhase,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> Result<(), SafeError>;
    /// Loads all unfinished operations in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when entries cannot be read.
    fn list_pending_operations(&self) -> Result<Vec<PendingAccountOperation>, SafeError>;
    /// Deletes one fully reconciled operation entry.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when finalization cannot be committed.
    fn finalize_operation(&self, id: OperationId) -> Result<(), SafeError>;
}

pub trait SecretStore: Send + Sync {}

pub trait NostrClient: Send + Sync {
    fn fetch_profile<'a>(
        &'a self,
        public_key: PublicKey,
        relays: &'a [RelayUrl],
    ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> UnixTimestamp;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use radroots_studio_domain::{
        AccountSummary, Kind0ProfileCandidate, PublicKey, RelayUrl, SafeError, UnixTimestamp,
    };

    use super::{
        AccountNamespaceRepository, AccountOperationKind, AccountOperationPhase,
        AccountPreferenceKey, AccountRepository, AppStateRepository, BoxFuture, CachedProfile,
        Clock, NostrClient, OperationDiagnostic, OperationId, OperationJournal,
        PendingAccountOperation, ProfileRefreshStatus, ProfileRepository, SecretStore,
    };

    #[derive(Default)]
    struct FakePorts {
        selected: Mutex<Option<PublicKey>>,
    }

    impl AccountRepository for FakePorts {
        fn list_accounts(&self) -> Result<Vec<AccountSummary>, SafeError> {
            Ok(Vec::new())
        }

        fn find_account(
            &self,
            _public_key: PublicKey,
        ) -> Result<Option<AccountSummary>, SafeError> {
            Ok(None)
        }

        fn insert_account(&self, _account: &AccountSummary) -> Result<(), SafeError> {
            Ok(())
        }

        fn update_account(&self, _account: &AccountSummary) -> Result<(), SafeError> {
            Ok(())
        }

        fn remove_account(&self, _public_key: PublicKey) -> Result<(), SafeError> {
            Ok(())
        }
    }

    impl ProfileRepository for FakePorts {
        fn load_profile(&self, _public_key: PublicKey) -> Result<Option<CachedProfile>, SafeError> {
            Ok(None)
        }

        fn save_profile(&self, _profile: &CachedProfile) -> Result<(), SafeError> {
            Ok(())
        }

        fn record_refresh_status(
            &self,
            _public_key: PublicKey,
            _refreshed_at: UnixTimestamp,
            _status: ProfileRefreshStatus,
        ) -> Result<(), SafeError> {
            Ok(())
        }

        fn remove_profile(&self, _public_key: PublicKey) -> Result<(), SafeError> {
            Ok(())
        }
    }

    impl AccountNamespaceRepository for FakePorts {
        fn get_value(
            &self,
            _owner: PublicKey,
            _key: AccountPreferenceKey,
        ) -> Result<Option<String>, SafeError> {
            Ok(None)
        }

        fn set_value(
            &self,
            _owner: PublicKey,
            _key: AccountPreferenceKey,
            _value: &str,
        ) -> Result<(), SafeError> {
            Ok(())
        }

        fn clear_owner(&self, _owner: PublicKey) -> Result<(), SafeError> {
            Ok(())
        }
    }

    impl AppStateRepository for FakePorts {
        fn load_selected_account(&self) -> Result<Option<PublicKey>, SafeError> {
            Ok(*self.selected.lock().expect("selected lock"))
        }

        fn save_selected_account(&self, public_key: Option<PublicKey>) -> Result<(), SafeError> {
            *self.selected.lock().expect("selected lock") = public_key;
            Ok(())
        }
    }

    impl OperationJournal for FakePorts {
        fn begin_operation(
            &self,
            _kind: AccountOperationKind,
            _subject: PublicKey,
            _updated_at: UnixTimestamp,
        ) -> Result<OperationId, SafeError> {
            Ok(OperationId::from_raw(1))
        }

        fn update_operation(
            &self,
            _id: OperationId,
            _phase: AccountOperationPhase,
            _updated_at: UnixTimestamp,
            _diagnostic: Option<OperationDiagnostic>,
        ) -> Result<(), SafeError> {
            Ok(())
        }

        fn list_pending_operations(&self) -> Result<Vec<PendingAccountOperation>, SafeError> {
            Ok(Vec::new())
        }

        fn finalize_operation(&self, _id: OperationId) -> Result<(), SafeError> {
            Ok(())
        }
    }

    impl SecretStore for FakePorts {}

    impl NostrClient for FakePorts {
        fn fetch_profile<'a>(
            &'a self,
            _public_key: PublicKey,
            _relays: &'a [RelayUrl],
        ) -> BoxFuture<'a, Result<Option<Kind0ProfileCandidate>, SafeError>> {
            Box::pin(async { Ok(None) })
        }
    }

    impl Clock for FakePorts {
        fn now(&self) -> UnixTimestamp {
            UnixTimestamp::from_seconds(1).expect("valid fake time")
        }
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn ports_accept_send_sync_test_fakes() {
        assert_send_sync::<FakePorts>();

        let ports = FakePorts::default();
        ports
            .save_selected_account(Some(PublicKey::from_bytes([1_u8; 32])))
            .expect("save selection");
        assert_eq!(
            ports.load_selected_account().expect("load selection"),
            Some(PublicKey::from_bytes([1_u8; 32]))
        );
        assert_eq!(ports.now().as_seconds(), 1);
    }
}
