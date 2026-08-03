#![doc = "Radroots Studio application runtime."]

pub mod accounts;
pub mod actor;
pub mod app_core;
pub mod config;
pub mod nostr_client;
pub mod ports;
mod profile_refresh;
pub mod recovery;
pub mod secrets;
pub mod session;
pub mod snapshot;
pub mod state_machine;

pub use accounts::{
    GenerateAccountReceipt, ImportAccountReceipt, InMemoryAccountRepository,
    InMemoryOperationJournal,
};
pub use actor::{
    ActorMailbox, CommandContext, CommandReceipt, CommandRejection, CommandResult,
    CommandSubmission, CommandTicket, LifecycleGate, RequestId, RuntimeCommandClass,
    RuntimeLifecycle,
};
pub use app_core::{AppCore, AppObserver, ObserverHandle, RemovalConfirmationToken};
pub use config::{
    RelayRuntimeMode, relay_configuration_from_environment, relay_configuration_from_value,
};
pub use nostr_client::SdkNostrClient;
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
