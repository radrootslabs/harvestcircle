use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};

use crate::{
    AccountRepository, AppSnapshot, AppStateRepository, RelayConfiguration, StateMachine,
    StateTransition,
};

pub trait AppObserver: Send + Sync {
    fn on_snapshot_changed(&self, snapshot: AppSnapshot);
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObserverHandle(u64);

impl ObserverHandle {
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

struct CoreState {
    state_machine: StateMachine,
    observers: BTreeMap<ObserverHandle, Arc<dyn AppObserver>>,
    next_observer: u64,
}

pub struct AppCore {
    relay_configuration: RelayConfiguration,
    state: Mutex<CoreState>,
}

impl AppCore {
    #[must_use]
    pub fn in_memory(relay_configuration: RelayConfiguration) -> Self {
        Self {
            relay_configuration,
            state: Mutex::new(CoreState {
                state_machine: StateMachine::booting(),
                observers: BTreeMap::new(),
                next_observer: 1,
            }),
        }
    }

    /// Moves the in-memory core from booting to an empty ready snapshot.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error if the ready snapshot invariant
    /// cannot be constructed.
    pub fn bootstrap(&self) -> Result<AppSnapshot, SafeError> {
        self.apply_transition(StateTransition::Bootstrap)
    }

    /// Loads the durable public registry and selection into a signed-out snapshot.
    ///
    /// # Errors
    ///
    /// Returns the safe persistence error after publishing a fatal snapshot when
    /// durable state cannot be read or violates application invariants.
    pub fn bootstrap_from(
        &self,
        accounts: &(impl AccountRepository + ?Sized),
        app_state: &(impl AppStateRepository + ?Sized),
    ) -> Result<AppSnapshot, SafeError> {
        let loaded = accounts.list_accounts().and_then(|accounts| {
            app_state
                .load_selected_account()
                .map(|selected| (accounts, selected))
        });
        match loaded {
            Ok((accounts, selected)) => {
                self.apply_transition(StateTransition::BootstrapRegistry { accounts, selected })
            }
            Err(error) => {
                self.apply_transition(StateTransition::Fatal(error))?;
                Err(error)
            }
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshot {
        self.lock_state().state_machine.snapshot().clone()
    }

    /// Registers an observer and immediately supplies the current snapshot.
    ///
    /// # Errors
    ///
    /// Returns a safe observer error if the handle space is exhausted.
    pub fn subscribe(&self, observer: Arc<dyn AppObserver>) -> Result<ObserverHandle, SafeError> {
        let (handle, snapshot, observer) = {
            let mut state = self.lock_state();
            let handle = ObserverHandle(state.next_observer);
            state.next_observer = state
                .next_observer
                .checked_add(1)
                .ok_or_else(observer_registration_failed)?;
            let registered = Arc::clone(&observer);
            state.observers.insert(handle, observer);
            (handle, state.state_machine.snapshot().clone(), registered)
        };
        observer.on_snapshot_changed(snapshot);
        Ok(handle)
    }

    #[must_use]
    pub fn unsubscribe(&self, handle: ObserverHandle) -> bool {
        self.lock_state().observers.remove(&handle).is_some()
    }

    pub(crate) fn apply_transition(
        &self,
        transition: StateTransition,
    ) -> Result<AppSnapshot, SafeError> {
        let (snapshot, observers) = {
            let mut state = self.lock_state();
            let previous_revision = state.state_machine.snapshot().revision();
            let snapshot = state
                .state_machine
                .apply(transition, &self.relay_configuration)?;
            let observers = if snapshot.revision() == previous_revision {
                Vec::new()
            } else {
                state.observers.values().cloned().collect::<Vec<_>>()
            };
            (snapshot, observers)
        };
        for observer in observers {
            observer.on_snapshot_changed(snapshot.clone());
        }
        Ok(snapshot)
    }

    fn lock_state(&self) -> MutexGuard<'_, CoreState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

const fn observer_registration_failed() -> SafeError {
    SafeError::new(
        SafeErrorCode::ObserverRegistrationFailed,
        SafeMessage::new("The application observer could not be registered."),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, Weak};

    use crate::{AppCore, AppLifecycle, AppObserver, AppSnapshot, RelayConfiguration};

    #[derive(Default)]
    struct RecordingObserver {
        snapshots: Mutex<Vec<AppSnapshot>>,
        core: Mutex<Option<Weak<AppCore>>>,
    }

    impl AppObserver for RecordingObserver {
        fn on_snapshot_changed(&self, snapshot: AppSnapshot) {
            if let Some(core) = self
                .core
                .lock()
                .expect("observer core lock")
                .as_ref()
                .and_then(Weak::upgrade)
            {
                assert_eq!(core.snapshot().revision(), snapshot.revision());
            }
            self.snapshots
                .lock()
                .expect("observer snapshots lock")
                .push(snapshot);
        }
    }

    #[test]
    fn bootstrap_publishes_ready_snapshot_outside_the_core_lock() {
        let core = Arc::new(AppCore::in_memory(RelayConfiguration::default()));
        let observer = Arc::new(RecordingObserver::default());
        *observer.core.lock().expect("observer core lock") = Some(Arc::downgrade(&core));

        let handle = core.subscribe(observer.clone()).expect("register observer");
        let ready = core.bootstrap().expect("bootstrap");
        let repeated = core.bootstrap().expect("idempotent bootstrap");

        assert_eq!(handle.value(), 1);
        assert_eq!(ready.lifecycle(), AppLifecycle::Ready);
        assert_eq!(ready.revision().value(), 1);
        assert_eq!(repeated, ready);
        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 2);
    }

    #[test]
    fn deregistered_observer_receives_no_later_bootstrap_update() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let observer = Arc::new(RecordingObserver::default());
        let handle = core.subscribe(observer.clone()).expect("register observer");

        assert!(core.unsubscribe(handle));
        assert!(!core.unsubscribe(handle));
        core.bootstrap().expect("bootstrap");

        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 1);
    }

    #[test]
    fn core_instances_never_share_state_or_observers() {
        let first = AppCore::in_memory(RelayConfiguration::default());
        let second = AppCore::in_memory(RelayConfiguration::default());
        let observer = Arc::new(RecordingObserver::default());
        first
            .subscribe(observer.clone())
            .expect("register observer");

        first.bootstrap().expect("first bootstrap");

        assert_eq!(first.snapshot().revision().value(), 1);
        assert_eq!(second.snapshot().revision().value(), 0);
        assert_eq!(observer.snapshots.lock().expect("snapshots").len(), 2);
    }
}
