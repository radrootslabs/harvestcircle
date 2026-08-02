use radroots_studio_domain::{AccountSummary, PublicKey, SafeError, SafeErrorCode, SafeMessage};

use crate::{ActiveAccountSnapshot, AppLifecycle, AppSnapshot, RelayConfiguration, SessionState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateTransition {
    Bootstrap,
    ReplaceRegistry {
        accounts: Vec<AccountSummary>,
        selected: Option<PublicKey>,
    },
    Select(PublicKey),
    BeginActivation(PublicKey),
    ActivationSucceeded(Box<ActiveAccountSnapshot>),
    ActivationFailed(SafeError),
    SignOut,
    SetProblem(Option<SafeError>),
}

#[derive(Clone)]
struct PreviousSession {
    session: SessionState,
    active_account: Option<ActiveAccountSnapshot>,
}

pub struct StateMachine {
    snapshot: AppSnapshot,
    pending_activation: Option<(PublicKey, PreviousSession)>,
}

impl StateMachine {
    #[must_use]
    pub fn booting() -> Self {
        Self {
            snapshot: AppSnapshot::booting(),
            pending_activation: None,
        }
    }

    #[must_use]
    pub const fn snapshot(&self) -> &AppSnapshot {
        &self.snapshot
    }

    /// Applies one deterministic state transition and returns the new snapshot.
    ///
    /// # Errors
    ///
    /// Returns a safe application error when the transition violates account,
    /// revision, activation, or snapshot invariants.
    pub fn apply(
        &mut self,
        transition: StateTransition,
        relay_configuration: &RelayConfiguration,
    ) -> Result<AppSnapshot, SafeError> {
        let next_revision = self
            .snapshot
            .revision()
            .next()
            .ok_or_else(invalid_application_state)?;

        let next = match transition {
            StateTransition::Bootstrap => self.bootstrap(next_revision, relay_configuration)?,
            StateTransition::ReplaceRegistry { accounts, selected } => {
                self.replace_registry(next_revision, accounts, selected)?
            }
            StateTransition::Select(public_key) => self.select(next_revision, public_key)?,
            StateTransition::BeginActivation(public_key) => {
                self.begin_activation(next_revision, public_key)?
            }
            StateTransition::ActivationSucceeded(active_account) => {
                self.activation_succeeded(next_revision, *active_account)?
            }
            StateTransition::ActivationFailed(problem) => {
                self.activation_failed(next_revision, problem)?
            }
            StateTransition::SignOut => self.sign_out(next_revision)?,
            StateTransition::SetProblem(problem) => self.copy_ready(
                next_revision,
                self.snapshot.selected_account(),
                self.snapshot.session(),
                self.snapshot.active_account().cloned(),
                problem,
            )?,
        };
        self.snapshot = next.clone();
        Ok(next)
    }

    fn bootstrap(
        &self,
        revision: crate::SnapshotRevision,
        relay_configuration: &RelayConfiguration,
    ) -> Result<AppSnapshot, SafeError> {
        if !matches!(self.snapshot.lifecycle(), AppLifecycle::Booting) {
            return Ok(self.snapshot.clone());
        }
        AppSnapshot::ready(
            revision,
            relay_configuration.clone(),
            Vec::new(),
            None,
            SessionState::SignedOut,
            None,
            None,
        )
    }

    fn replace_registry(
        &mut self,
        revision: crate::SnapshotRevision,
        accounts: Vec<AccountSummary>,
        selected: Option<PublicKey>,
    ) -> Result<AppSnapshot, SafeError> {
        self.pending_activation = None;
        AppSnapshot::ready(
            revision,
            self.snapshot.relay_configuration().clone(),
            accounts,
            selected,
            SessionState::SignedOut,
            None,
            None,
        )
    }

    fn select(
        &self,
        revision: crate::SnapshotRevision,
        public_key: PublicKey,
    ) -> Result<AppSnapshot, SafeError> {
        self.require_account(public_key)?;
        self.copy_ready(
            revision,
            Some(public_key),
            self.snapshot.session(),
            self.snapshot.active_account().cloned(),
            None,
        )
    }

    fn begin_activation(
        &mut self,
        revision: crate::SnapshotRevision,
        public_key: PublicKey,
    ) -> Result<AppSnapshot, SafeError> {
        self.require_account(public_key)?;
        if self.pending_activation.is_some() {
            return Err(invalid_application_state());
        }
        self.pending_activation = Some((
            public_key,
            PreviousSession {
                session: self.snapshot.session(),
                active_account: self.snapshot.active_account().cloned(),
            },
        ));
        self.copy_ready(
            revision,
            self.snapshot.selected_account(),
            SessionState::Activating(public_key),
            self.snapshot.active_account().cloned(),
            None,
        )
    }

    fn activation_succeeded(
        &mut self,
        revision: crate::SnapshotRevision,
        active_account: ActiveAccountSnapshot,
    ) -> Result<AppSnapshot, SafeError> {
        let Some((target, _previous)) = self.pending_activation.as_ref() else {
            return Err(invalid_application_state());
        };
        if active_account.account().public_key() != *target {
            return Err(invalid_application_state());
        }
        let target = *target;
        self.pending_activation = None;
        self.copy_ready(
            revision,
            Some(target),
            SessionState::Active,
            Some(active_account),
            None,
        )
    }

    fn activation_failed(
        &mut self,
        revision: crate::SnapshotRevision,
        problem: SafeError,
    ) -> Result<AppSnapshot, SafeError> {
        let Some((_target, previous)) = self.pending_activation.take() else {
            return Err(invalid_application_state());
        };
        self.copy_ready(
            revision,
            self.snapshot.selected_account(),
            previous.session,
            previous.active_account,
            Some(problem),
        )
    }

    fn sign_out(&mut self, revision: crate::SnapshotRevision) -> Result<AppSnapshot, SafeError> {
        self.pending_activation = None;
        self.copy_ready(
            revision,
            self.snapshot.selected_account(),
            SessionState::SignedOut,
            None,
            None,
        )
    }

    fn require_account(&self, public_key: PublicKey) -> Result<(), SafeError> {
        if self
            .snapshot
            .accounts()
            .iter()
            .any(|account| account.public_key() == public_key)
        {
            Ok(())
        } else {
            Err(account_not_found())
        }
    }

    fn copy_ready(
        &self,
        revision: crate::SnapshotRevision,
        selected_account: Option<PublicKey>,
        session: SessionState,
        active_account: Option<ActiveAccountSnapshot>,
        recoverable_problem: Option<SafeError>,
    ) -> Result<AppSnapshot, SafeError> {
        AppSnapshot::ready(
            revision,
            self.snapshot.relay_configuration().clone(),
            self.snapshot.accounts().to_vec(),
            selected_account,
            session,
            active_account,
            recoverable_problem,
        )
    }
}

const fn invalid_application_state() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The application state is invalid."),
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
    use radroots_studio_domain::{
        AccountCreatedAt, AccountSummary, KeyAvailability, Npub, PublicKey, SafeError,
        SafeErrorCode, SafeMessage, SignerKind, UnixTimestamp,
    };

    use crate::{
        ActiveAccountSnapshot, ProfileLoadState, RelayConfiguration, RelayConnectionState,
        SessionState, StateMachine, StateTransition,
    };

    const NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

    fn account(key_byte: u8) -> AccountSummary {
        AccountSummary::new(
            PublicKey::from_bytes([key_byte; 32]),
            Npub::from_encoded(NPUB.to_owned()).expect("valid npub"),
            SignerKind::LocalSecret,
            KeyAvailability::Available,
            None,
            AccountCreatedAt::new(UnixTimestamp::from_seconds(1).expect("valid time")),
            None,
        )
    }

    fn active(account: AccountSummary) -> ActiveAccountSnapshot {
        ActiveAccountSnapshot::new(
            account,
            RelayConnectionState::Disconnected,
            ProfileLoadState::Empty,
            None,
        )
    }

    #[test]
    fn state_machine_command_trace_preserves_working_session_on_failed_replacement() {
        let first = account(1);
        let second = account(2);
        let mut machine = StateMachine::booting();
        let relays = RelayConfiguration::default();
        let problem = SafeError::new(
            SafeErrorCode::CredentialMissing,
            SafeMessage::new("The account credential is missing."),
        );

        machine
            .apply(StateTransition::Bootstrap, &relays)
            .expect("bootstrap");
        machine
            .apply(
                StateTransition::ReplaceRegistry {
                    accounts: vec![first.clone(), second.clone()],
                    selected: Some(first.public_key()),
                },
                &relays,
            )
            .expect("load registry");
        machine
            .apply(
                StateTransition::BeginActivation(first.public_key()),
                &relays,
            )
            .expect("begin first activation");
        machine
            .apply(
                StateTransition::ActivationSucceeded(Box::new(active(first.clone()))),
                &relays,
            )
            .expect("activate first");
        machine
            .apply(StateTransition::Select(second.public_key()), &relays)
            .expect("select second");
        let pending = machine
            .apply(
                StateTransition::BeginActivation(second.public_key()),
                &relays,
            )
            .expect("begin replacement");
        let restored = machine
            .apply(StateTransition::ActivationFailed(problem), &relays)
            .expect("fail replacement");

        assert_eq!(
            pending.session(),
            SessionState::Activating(second.public_key())
        );
        assert_eq!(
            pending
                .active_account()
                .map(|value| value.account().public_key()),
            Some(first.public_key())
        );
        assert_eq!(restored.session(), SessionState::Active);
        assert_eq!(restored.selected_account(), Some(second.public_key()));
        assert_eq!(
            restored
                .active_account()
                .map(|value| value.account().public_key()),
            Some(first.public_key())
        );
        assert_eq!(restored.recoverable_problem(), Some(problem));
        assert_eq!(restored.revision().value(), 7);
    }

    #[test]
    fn state_machine_rejects_missing_targets_and_signs_out_without_deleting() {
        let account = account(1);
        let mut machine = StateMachine::booting();
        let relays = RelayConfiguration::default();
        machine
            .apply(StateTransition::Bootstrap, &relays)
            .expect("bootstrap");
        machine
            .apply(
                StateTransition::ReplaceRegistry {
                    accounts: vec![account.clone()],
                    selected: Some(account.public_key()),
                },
                &relays,
            )
            .expect("load registry");

        let error = machine
            .apply(
                StateTransition::Select(PublicKey::from_bytes([9_u8; 32])),
                &relays,
            )
            .expect_err("missing account");
        assert_eq!(error.code(), SafeErrorCode::AccountNotFound);

        machine
            .apply(
                StateTransition::BeginActivation(account.public_key()),
                &relays,
            )
            .expect("begin activation");
        machine
            .apply(
                StateTransition::ActivationSucceeded(Box::new(active(account.clone()))),
                &relays,
            )
            .expect("activate");
        let signed_out = machine
            .apply(StateTransition::SignOut, &relays)
            .expect("sign out");

        assert_eq!(signed_out.accounts(), &[account]);
        assert_eq!(signed_out.session(), SessionState::SignedOut);
        assert!(signed_out.active_account().is_none());
    }
}
