use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use radroots_studio_domain::{SafeError, SafeErrorCode, SafeMessage};

use crate::{
    AccountRepository, AppSnapshot, AppStateRepository, RelayConfiguration, SnapshotRevision,
    StateMachine, StateTransition,
};

pub struct RemovalConfirmationToken {
    id: u64,
    public_key: radroots_studio_domain::PublicKey,
    revision: SnapshotRevision,
}

struct CoreState {
    state_machine: StateMachine,
    removal_tokens: BTreeMap<u64, (radroots_studio_domain::PublicKey, SnapshotRevision)>,
    next_removal_token: u64,
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
                removal_tokens: BTreeMap::new(),
                next_removal_token: 1,
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

    pub(crate) fn apply_transition(
        &self,
        transition: StateTransition,
    ) -> Result<AppSnapshot, SafeError> {
        self.lock_state()
            .state_machine
            .apply(transition, &self.relay_configuration)
    }

    pub(crate) fn issue_removal_token(
        &self,
        public_key: radroots_studio_domain::PublicKey,
    ) -> Result<RemovalConfirmationToken, SafeError> {
        let mut state = self.lock_state();
        if !state
            .state_machine
            .snapshot()
            .accounts()
            .iter()
            .any(|account| account.public_key() == public_key)
        {
            return Err(account_not_found());
        }
        let id = state.next_removal_token;
        state.next_removal_token = id.checked_add(1).ok_or_else(invalid_application_state)?;
        let revision = state.state_machine.snapshot().revision();
        state.removal_tokens.insert(id, (public_key, revision));
        Ok(RemovalConfirmationToken {
            id,
            public_key,
            revision,
        })
    }

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn consume_removal_token(
        &self,
        token: RemovalConfirmationToken,
    ) -> Result<radroots_studio_domain::PublicKey, SafeError> {
        let RemovalConfirmationToken {
            id,
            public_key,
            revision,
        } = token;
        let mut state = self.lock_state();
        let stored = state.removal_tokens.remove(&id);
        if stored != Some((public_key, revision))
            || state.state_machine.snapshot().revision() != revision
        {
            return Err(invalid_application_state());
        }
        Ok(public_key)
    }

    fn lock_state(&self) -> MutexGuard<'_, CoreState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

const fn invalid_application_state() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The account removal confirmation is no longer valid."),
    )
}

const fn account_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::AccountNotFound,
        SafeMessage::new("The account was not found."),
    )
}

#[cfg(test)]
mod tests {
    use crate::{AppCore, AppLifecycle, RelayConfiguration};

    #[test]
    fn bootstrap_is_idempotent_and_advances_only_once() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let ready = core.bootstrap().expect("bootstrap");
        let repeated = core.bootstrap().expect("idempotent bootstrap");

        assert_eq!(ready.lifecycle(), AppLifecycle::Ready);
        assert_eq!(ready.revision().value(), 1);
        assert_eq!(repeated, ready);
    }

    #[test]
    fn core_instances_never_share_state() {
        let first = AppCore::in_memory(RelayConfiguration::default());
        let second = AppCore::in_memory(RelayConfiguration::default());

        first.bootstrap().expect("first bootstrap");

        assert_eq!(first.snapshot().revision().value(), 1);
        assert_eq!(second.snapshot().revision().value(), 0);
    }
}
