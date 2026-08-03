use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use radroots_studio_domain::{PublicKey, SafeError, SafeErrorCode, SafeMessage, UnixTimestamp};

use crate::{
    AccountRepository, AppSnapshot, AppStateRepository, RelayConfiguration, SnapshotRevision,
    StateMachine, StateTransition,
};

pub struct RemovalConfirmationToken {
    id: u64,
    public_key: PublicKey,
    revision: SnapshotRevision,
    expires_at: UnixTimestamp,
    impact: RemovalImpact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemovalImpact {
    deletes_local_credential: bool,
    signs_out: bool,
}

impl RemovalImpact {
    #[must_use]
    pub const fn deletes_local_credential(self) -> bool {
        self.deletes_local_credential
    }
    #[must_use]
    pub const fn signs_out(self) -> bool {
        self.signs_out
    }
}

impl RemovalConfirmationToken {
    #[must_use]
    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }
    #[must_use]
    pub const fn revision(&self) -> SnapshotRevision {
        self.revision
    }
    #[must_use]
    pub const fn expires_at(&self) -> UnixTimestamp {
        self.expires_at
    }
    #[must_use]
    pub const fn impact(&self) -> RemovalImpact {
        self.impact
    }
}

#[derive(Clone, Copy)]
struct RemovalTokenState {
    public_key: PublicKey,
    revision: SnapshotRevision,
    expires_at: UnixTimestamp,
    impact: RemovalImpact,
}

struct CoreState {
    state_machine: StateMachine,
    removal_tokens: BTreeMap<u64, RemovalTokenState>,
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
        public_key: PublicKey,
        now: UnixTimestamp,
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
        let expires_at = UnixTimestamp::from_seconds(
            now.as_seconds()
                .checked_add(300)
                .ok_or_else(invalid_application_state)?,
        )
        .ok_or_else(invalid_application_state)?;
        let impact = RemovalImpact {
            deletes_local_credential: true,
            signs_out: state
                .state_machine
                .snapshot()
                .active_account()
                .is_some_and(|active| active.account().public_key() == public_key),
        };
        state.removal_tokens.insert(
            id,
            RemovalTokenState {
                public_key,
                revision,
                expires_at,
                impact,
            },
        );
        Ok(RemovalConfirmationToken {
            id,
            public_key,
            revision,
            expires_at,
            impact,
        })
    }

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn consume_removal_token(
        &self,
        token: RemovalConfirmationToken,
        now: UnixTimestamp,
    ) -> Result<PublicKey, SafeError> {
        let RemovalConfirmationToken {
            id,
            public_key,
            revision,
            expires_at,
            impact,
        } = token;
        let mut state = self.lock_state();
        let stored = state.removal_tokens.remove(&id);
        if stored.is_none_or(|stored| {
            stored.public_key != public_key
                || stored.revision != revision
                || stored.expires_at != expires_at
                || stored.impact != impact
        }) || state.state_machine.snapshot().revision() != revision
            || now.as_seconds() > expires_at.as_seconds()
        {
            return Err(invalid_application_state());
        }
        Ok(public_key)
    }

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn cancel_removal_token(&self, token: RemovalConfirmationToken) -> bool {
        self.lock_state().removal_tokens.remove(&token.id).is_some()
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
