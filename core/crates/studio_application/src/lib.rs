#![doc = "Radroots Studio application runtime."]

pub mod accounts;
pub mod actor;
pub mod app_core;
mod change_stream;
pub mod config;
pub mod custody;
pub mod ports;
mod profile_refresh;
pub mod recovery;
pub mod secrets;
pub mod session;
pub mod snapshot;
pub mod state_machine;

#[cfg(test)]
mod test_support;

pub use accounts::{
    GenerateAccountReceipt, ImportAccountReceipt, InMemoryAccountRepository,
    InMemoryOperationJournal,
};
pub use actor::{
    ActorMailbox, CommandContext, CommandEnvelope, CommandReceipt, CommandRejection, CommandResult,
    CommandSubmission, CommandTicket, ForegroundSessionBinding, LifecycleGate, RequestId,
    RuntimeCommandClass, RuntimeLifecycle, SessionGeneration, TaskCorrelation,
};
pub use app_core::{AppCore, RemovalConfirmationToken, RemovalImpact};
pub use change_stream::{
    ChangeSubscriptionId, OrderedSnapshotChanges, SnapshotChange, SnapshotChangeReceiver,
};
pub use config::{
    RelayRuntimeMode, relay_configuration_from_environment, relay_configuration_from_value,
};
pub use custody::{
    GENERATED_KEY_STAGE_TTL, GeneratedKeyRecoveryHandle, GeneratedKeyStage, GeneratedKeyStageView,
    RecoveryStageId, StagedGeneratedKey,
};
pub use ports::{
    AccountNamespaceRepository, AccountOperationKind, AccountOperationPhase, AccountPreferenceKey,
    AccountRepository, AppStateRepository, BoxFuture, CachedProfile, Clock,
    DurableAccountOperation, DurableOperationKind, DurableOperationPhase, DurableOperationReceipt,
    DurableOperationRepository, DurableOperationStart, DurableRequestId, DurableTerminalOutcome,
    GeneratedKeyMaterial, ImportedKeyMaterial, KeyMaterialProvider, NostrClient,
    OperationDiagnostic, OperationId, OperationJournal, OperationPriorState,
    PendingAccountOperation, ProfileFetchResult, ProfileRefreshStatus, ProfileRepository,
    RelayFetchCompleteness,
};
pub use profile_refresh::ProfileRefreshPlan;
pub use secrets::{
    FailureSecretStore, InMemorySecretStore, SecretStore, SecretStoreCall, SecretStoreOperation,
};
pub use snapshot::{
    ActiveAccountSnapshot, AppLifecycle, AppSnapshot, MAX_CONFIGURED_RELAYS, ProfileLoadState,
    RelayConfiguration, RelayConnectionState, SessionState, SnapshotRevision,
};
pub use state_machine::{StateMachine, StateTransition};
