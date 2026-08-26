#![doc = "HarvestCircle application runtime."]

pub mod actor;
pub mod app_core;
mod change_stream;
pub mod config;
pub mod custody;
pub mod identities;
pub mod ports;
mod profile_refresh;
pub mod recovery;
pub mod secrets;
pub mod session;
pub mod snapshot;
pub mod state_machine;

#[cfg(test)]
mod test_support;

pub use actor::{
    ActiveSessionBinding, ActorMailbox, CommandContext, CommandEnvelope, CommandReceipt,
    CommandRejection, CommandResult, CommandSubmission, CommandTicket, LifecycleGate, RequestId,
    RuntimeCommandClass, RuntimeLifecycle, SessionGeneration, TaskCorrelation,
};
pub use app_core::{AppCore, RemovalConfirmationToken, RemovalImpact};
pub use change_stream::{
    ChangeSubscriptionId, OrderedSnapshotChanges, SnapshotChange, SnapshotChangeReceiver,
};
pub use config::{RelayEndpointInput, relay_configuration_from_endpoints};
pub use custody::{
    GENERATED_KEY_STAGE_TTL, GeneratedKeyRecoveryHandle, GeneratedKeyStage, GeneratedKeyStageView,
    RecoveryStageId, StagedGeneratedKey,
};
#[cfg(test)]
pub use identities::InMemoryOperationJournal;
pub use identities::{GenerateIdentityReceipt, ImportIdentityReceipt, InMemoryIdentityRepository};
pub use ports::{
    AppStateRepository, BoxFuture, CachedProfile, Clock, DurableIdentityOperation,
    DurableOperationKind, DurableOperationPhase, DurableOperationReceipt,
    DurableOperationRepository, DurableOperationStart, DurableRequestId, DurableTerminalOutcome,
    GeneratedKeyMaterial, IdentityNamespaceRepository, IdentityPreferenceKey, IdentityRepository,
    ImportedKeyMaterial, KeyMaterialProvider, NostrClient, OperationDiagnostic,
    OperationPriorState, ProfileFetchResult, ProfileRefreshStatus, ProfileRepository,
    RelayFetchCompleteness,
};
#[cfg(test)]
pub use ports::{
    IdentityOperationKind, IdentityOperationPhase, OperationId, OperationJournal,
    PendingIdentityOperation,
};
pub use profile_refresh::ProfileRefreshPlan;
pub use secrets::{
    FailureSecretStore, InMemorySecretStore, SecretStore, SecretStoreCall, SecretStoreOperation,
};
pub use snapshot::{
    ActiveIdentitySnapshot, AppLifecycle, AppSnapshot, MAX_CONFIGURED_RELAYS, ProfileLoadState,
    RelayConfiguration, RelayConnectionState, SessionState, SnapshotRevision,
};
pub use state_machine::{StateMachine, StateTransition};
