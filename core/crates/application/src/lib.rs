#![doc = "Radroots Studio application runtime."]

pub mod accounts;
pub mod app_core;
pub mod config;
pub mod ports;
pub mod recovery;
pub mod secrets;
pub mod session;
pub mod snapshot;
pub mod state_machine;

pub use accounts::{
    GenerateAccountReceipt, ImportAccountReceipt, InMemoryAccountRepository,
    InMemoryOperationJournal,
};
pub use app_core::{AppCore, AppObserver, ObserverHandle, RemovalConfirmationToken};
pub use config::{
    RelayRuntimeMode, relay_configuration_from_environment, relay_configuration_from_value,
};
pub use ports::{
    AccountNamespaceRepository, AccountOperationKind, AccountOperationPhase, AccountPreferenceKey,
    AccountRepository, AppStateRepository, BoxFuture, CachedProfile, Clock, NostrClient,
    OperationDiagnostic, OperationId, OperationJournal, PendingAccountOperation,
    ProfileRefreshStatus, ProfileRepository,
};
pub use secrets::{
    FailureSecretStore, InMemorySecretStore, SecretStore, SecretStoreCall, SecretStoreOperation,
};
pub use snapshot::{
    ActiveAccountSnapshot, AppLifecycle, AppSnapshot, ProfileLoadState, RelayConfiguration,
    RelayConnectionState, SessionState, SnapshotRevision,
};
pub use state_machine::{StateMachine, StateTransition};
