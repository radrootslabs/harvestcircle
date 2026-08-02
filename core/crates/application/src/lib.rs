#![doc = "Radroots Studio application runtime."]

pub mod app_core;
pub mod ports;
pub mod snapshot;
pub mod state_machine;

pub use app_core::{AppCore, AppObserver, ObserverHandle};
pub use ports::{
    AccountNamespaceRepository, AccountRepository, AppStateRepository, BoxFuture, CachedProfile,
    Clock, NostrClient, ProfileRefreshStatus, ProfileRepository, SecretStore,
};
pub use snapshot::{
    ActiveAccountSnapshot, AppLifecycle, AppSnapshot, ProfileLoadState, RelayConfiguration,
    RelayConnectionState, SessionState, SnapshotRevision,
};
pub use state_machine::{StateMachine, StateTransition};
