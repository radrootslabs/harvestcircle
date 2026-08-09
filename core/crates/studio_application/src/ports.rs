use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use radroots_studio_domain::{
    AccountSummary, BindingAvailability, Kind0ProfileCandidate, Npub, Nsec, PublicKey, RelayUrl,
    SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, UnixTimestamp,
};

const MAX_DURABLE_REQUEST_ID_BYTES: usize = 128;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DurableRequestId(String);

impl DurableRequestId {
    /// Validates an opaque caller-generated idempotency key.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the value is empty, oversized, or contains anything
    /// other than visible ASCII characters.
    pub fn parse(value: impl Into<String>) -> Result<Self, SafeError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_DURABLE_REQUEST_ID_BYTES
            || !value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(invalid_request_id());
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableOperationKind {
    Create,
    Import,
    Repair,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableOperationPhase {
    IntentRecorded,
    CredentialWritten,
    MetadataCommitted,
    SelectionCommitted,
    CompensationPending,
    CredentialDeleted,
    MetadataDeleted,
    Finalized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableTerminalOutcome {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationPriorState {
    selected_account: Option<PublicKey>,
    binding_availability: Option<BindingAvailability>,
}

impl OperationPriorState {
    #[must_use]
    pub const fn new(
        selected_account: Option<PublicKey>,
        binding_availability: Option<BindingAvailability>,
    ) -> Self {
        Self {
            selected_account,
            binding_availability,
        }
    }

    #[must_use]
    pub const fn selected_account(self) -> Option<PublicKey> {
        self.selected_account
    }

    #[must_use]
    pub const fn binding_availability(self) -> Option<BindingAvailability> {
        self.binding_availability
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableOperationReceipt {
    request_id: DurableRequestId,
    account: PublicKey,
    outcome: DurableTerminalOutcome,
    resulting_revision: Option<u64>,
}

impl DurableOperationReceipt {
    #[must_use]
    pub const fn new(
        request_id: DurableRequestId,
        account: PublicKey,
        outcome: DurableTerminalOutcome,
        resulting_revision: Option<u64>,
    ) -> Self {
        Self {
            request_id,
            account,
            outcome,
            resulting_revision,
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> &DurableRequestId {
        &self.request_id
    }

    #[must_use]
    pub const fn account(&self) -> PublicKey {
        self.account
    }

    #[must_use]
    pub const fn outcome(&self) -> DurableTerminalOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn resulting_revision(&self) -> Option<u64> {
        self.resulting_revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableAccountOperation {
    request_id: DurableRequestId,
    kind: DurableOperationKind,
    account: PublicKey,
    expected_revision: Option<u64>,
    phase: DurableOperationPhase,
    prior: OperationPriorState,
    updated_at: UnixTimestamp,
    diagnostic: Option<OperationDiagnostic>,
    terminal: Option<DurableOperationReceipt>,
}

impl DurableAccountOperation {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        request_id: DurableRequestId,
        kind: DurableOperationKind,
        account: PublicKey,
        expected_revision: Option<u64>,
        phase: DurableOperationPhase,
        prior: OperationPriorState,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
        terminal: Option<DurableOperationReceipt>,
    ) -> Self {
        Self {
            request_id,
            kind,
            account,
            expected_revision,
            phase,
            prior,
            updated_at,
            diagnostic,
            terminal,
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> &DurableRequestId {
        &self.request_id
    }
    #[must_use]
    pub const fn kind(&self) -> DurableOperationKind {
        self.kind
    }
    #[must_use]
    pub const fn account(&self) -> PublicKey {
        self.account
    }
    #[must_use]
    pub const fn expected_revision(&self) -> Option<u64> {
        self.expected_revision
    }
    #[must_use]
    pub const fn phase(&self) -> DurableOperationPhase {
        self.phase
    }
    #[must_use]
    pub const fn prior(&self) -> OperationPriorState {
        self.prior
    }
    #[must_use]
    pub const fn updated_at(&self) -> UnixTimestamp {
        self.updated_at
    }
    #[must_use]
    pub const fn diagnostic(&self) -> Option<OperationDiagnostic> {
        self.diagnostic
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<&DurableOperationReceipt> {
        self.terminal.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DurableOperationStart {
    Started(DurableAccountOperation),
    Existing(DurableAccountOperation),
}

const fn invalid_request_id() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The request identifier is invalid."),
    )
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileRefreshStatus {
    Success,
    Offline,
    InvalidData,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayFetchCompleteness {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileFetchResult {
    candidate: Option<Kind0ProfileCandidate>,
    completeness: RelayFetchCompleteness,
}

impl ProfileFetchResult {
    #[must_use]
    pub const fn complete(candidate: Option<Kind0ProfileCandidate>) -> Self {
        Self {
            candidate,
            completeness: RelayFetchCompleteness::Complete,
        }
    }

    #[must_use]
    pub const fn partial(candidate: Option<Kind0ProfileCandidate>) -> Self {
        Self {
            candidate,
            completeness: RelayFetchCompleteness::Partial,
        }
    }

    #[must_use]
    pub fn into_parts(self) -> (Option<Kind0ProfileCandidate>, RelayFetchCompleteness) {
        (self.candidate, self.completeness)
    }
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
    Conflict,
    Expired,
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

pub trait DurableOperationRepository: Send + Sync {
    /// Records one idempotent durable operation or returns the existing matching request.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict or storage error when the request cannot be recorded.
    #[allow(clippy::too_many_arguments)]
    fn begin_durable_operation(
        &self,
        request_id: &DurableRequestId,
        kind: DurableOperationKind,
        account: PublicKey,
        expected_revision: Option<u64>,
        prior: OperationPriorState,
        updated_at: UnixTimestamp,
    ) -> Result<DurableOperationStart, SafeError>;
    /// Loads one durable operation by its idempotency key.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when the lookup cannot complete.
    fn load_durable_operation(
        &self,
        request_id: &DurableRequestId,
    ) -> Result<Option<DurableAccountOperation>, SafeError>;
    /// Advances one operation only from the caller's expected phase.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict or storage error when the transition cannot commit.
    fn advance_durable_operation(
        &self,
        request_id: &DurableRequestId,
        expected_phase: DurableOperationPhase,
        next_phase: DurableOperationPhase,
        updated_at: UnixTimestamp,
        diagnostic: Option<OperationDiagnostic>,
    ) -> Result<DurableAccountOperation, SafeError>;
    /// Finalizes one operation and durably retains its recoverable receipt.
    ///
    /// # Errors
    ///
    /// Returns a safe conflict or storage error when finalization cannot commit.
    fn finalize_durable_operation(
        &self,
        request_id: &DurableRequestId,
        expected_phase: DurableOperationPhase,
        outcome: DurableTerminalOutcome,
        resulting_revision: Option<u64>,
        updated_at: UnixTimestamp,
    ) -> Result<DurableOperationReceipt, SafeError>;
    /// Lists unfinished operations in deterministic request order.
    ///
    /// # Errors
    ///
    /// Returns a safe storage error when operations cannot be read.
    fn list_unfinished_durable_operations(&self)
    -> Result<Vec<DurableAccountOperation>, SafeError>;
}

pub trait NostrClient: Send + Sync {
    fn fetch_profile<'a>(
        &'a self,
        public_key: PublicKey,
        relays: &'a [RelayUrl],
        deadline: Instant,
    ) -> BoxFuture<'a, Result<ProfileFetchResult, SafeError>>;
}

pub struct GeneratedKeyMaterial {
    public_key: PublicKey,
    npub: Npub,
    secret: SecretKeyInput,
    nsec: Nsec,
}

impl GeneratedKeyMaterial {
    #[must_use]
    pub const fn new(
        public_key: PublicKey,
        npub: Npub,
        secret: SecretKeyInput,
        nsec: Nsec,
    ) -> Self {
        Self {
            public_key,
            npub,
            secret,
            nsec,
        }
    }

    #[must_use]
    pub fn into_parts(self) -> (PublicKey, Npub, SecretKeyInput, Nsec) {
        (self.public_key, self.npub, self.secret, self.nsec)
    }
}

pub struct ImportedKeyMaterial {
    public_key: PublicKey,
    npub: Npub,
    secret: SecretKeyInput,
}

impl ImportedKeyMaterial {
    #[must_use]
    pub const fn new(public_key: PublicKey, npub: Npub, secret: SecretKeyInput) -> Self {
        Self {
            public_key,
            npub,
            secret,
        }
    }

    #[must_use]
    pub fn into_parts(self) -> (PublicKey, Npub, SecretKeyInput) {
        (self.public_key, self.npub, self.secret)
    }
}

pub trait KeyMaterialProvider: Send + Sync {
    /// Generates one keypair from host-provided cryptographic entropy.
    ///
    /// # Errors
    ///
    /// Returns a redacted key or entropy error.
    fn generate(&self) -> Result<GeneratedKeyMaterial, SafeError>;

    /// Canonicalizes imported secret material and derives its public identity.
    ///
    /// # Errors
    ///
    /// Returns a redacted validation error.
    fn import(&self, input: SecretKeyInput) -> Result<ImportedKeyMaterial, SafeError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> UnixTimestamp;
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use std::sync::Mutex;

    use radroots_studio_domain::{AccountSummary, PublicKey, RelayUrl, SafeError, UnixTimestamp};

    use super::{
        AccountNamespaceRepository, AccountOperationKind, AccountOperationPhase,
        AccountPreferenceKey, AccountRepository, AppStateRepository, BoxFuture, CachedProfile,
        Clock, DurableOperationReceipt, DurableRequestId, DurableTerminalOutcome, NostrClient,
        OperationDiagnostic, OperationId, OperationJournal, PendingAccountOperation,
        ProfileFetchResult, ProfileRefreshStatus, ProfileRepository,
    };

    #[test]
    fn durable_request_ids_and_terminal_receipts_are_bounded_and_public() {
        let request = DurableRequestId::parse("create:desktop:0001").expect("request id");
        let receipt = DurableOperationReceipt::new(
            request.clone(),
            PublicKey::from_bytes([7; 32]).expect("valid public key"),
            DurableTerminalOutcome::Completed,
            Some(42),
        );
        assert_eq!(receipt.request_id(), &request);
        assert_eq!(receipt.resulting_revision(), Some(42));
        for invalid in ["", "contains space", &"x".repeat(129)] {
            assert!(DurableRequestId::parse(invalid).is_err());
        }
    }

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

    impl NostrClient for FakePorts {
        fn fetch_profile<'a>(
            &'a self,
            _public_key: PublicKey,
            _relays: &'a [RelayUrl],
            _deadline: Instant,
        ) -> BoxFuture<'a, Result<ProfileFetchResult, SafeError>> {
            Box::pin(async { Ok(ProfileFetchResult::complete(None)) })
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
            .save_selected_account(Some(
                PublicKey::from_bytes([7_u8; 32]).expect("valid public key"),
            ))
            .expect("save selection");
        assert_eq!(
            ports.load_selected_account().expect("load selection"),
            Some(PublicKey::from_bytes([7_u8; 32]).expect("valid public key"))
        );
        assert_eq!(ports.now().as_seconds(), 1);
    }
}
