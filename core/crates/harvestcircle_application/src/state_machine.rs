use harvestcircle_domain::{NostrIdentity, PublicKey, SafeError, SafeErrorCode, SafeMessage};

use crate::{ActiveIdentitySnapshot, AppLifecycle, AppSnapshot, RelayConfiguration, SessionState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateTransition {
    Bootstrap,
    BootstrapRegistry {
        identities: Vec<NostrIdentity>,
        selected: Option<PublicKey>,
    },
    Fatal(SafeError),
    ReplaceRegistry {
        identities: Vec<NostrIdentity>,
        selected: Option<PublicKey>,
    },
    ReplaceRegistryPreservingSession {
        identities: Vec<NostrIdentity>,
        selected: Option<PublicKey>,
    },
    Select(PublicKey),
    BeginActivation(PublicKey),
    ActivationSucceeded(Box<ActiveIdentitySnapshot>),
    ActivationFailed(SafeError),
    UpdateActiveIdentity {
        expected: PublicKey,
        active_identity: Box<ActiveIdentitySnapshot>,
        problem: Option<SafeError>,
    },
    SignOut,
    SetProblem(Option<SafeError>),
}

#[derive(Clone)]
struct PreviousSession {
    session: SessionState,
    active_identity: Option<ActiveIdentitySnapshot>,
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
    /// Returns a safe application error when the transition violates identity,
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
            StateTransition::BootstrapRegistry {
                identities,
                selected,
            } => {
                self.bootstrap_registry(next_revision, relay_configuration, identities, selected)?
            }
            StateTransition::Fatal(error) => {
                AppSnapshot::fatal(next_revision, relay_configuration.clone(), error)
            }
            StateTransition::ReplaceRegistry {
                identities,
                selected,
            } => self.replace_registry(next_revision, identities, selected)?,
            StateTransition::ReplaceRegistryPreservingSession {
                identities,
                selected,
            } => self.replace_registry_preserving_session(next_revision, identities, selected)?,
            StateTransition::Select(public_key) => self.select(next_revision, public_key)?,
            StateTransition::BeginActivation(public_key) => {
                self.begin_activation(next_revision, public_key)?
            }
            StateTransition::ActivationSucceeded(active_identity) => {
                self.activation_succeeded(next_revision, *active_identity)?
            }
            StateTransition::ActivationFailed(problem) => {
                self.activation_failed(next_revision, problem)?
            }
            StateTransition::UpdateActiveIdentity {
                expected,
                active_identity,
                problem,
            } => self.update_active_identity(next_revision, expected, *active_identity, problem)?,
            StateTransition::SignOut => self.sign_out(next_revision)?,
            StateTransition::SetProblem(problem) => self.copy_ready(
                next_revision,
                self.snapshot.selected_identity(),
                self.snapshot.session(),
                self.snapshot.active_identity().cloned(),
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

    fn bootstrap_registry(
        &self,
        revision: crate::SnapshotRevision,
        relay_configuration: &RelayConfiguration,
        identities: Vec<NostrIdentity>,
        selected: Option<PublicKey>,
    ) -> Result<AppSnapshot, SafeError> {
        if !matches!(self.snapshot.lifecycle(), AppLifecycle::Booting) {
            return Ok(self.snapshot.clone());
        }
        AppSnapshot::ready(
            revision,
            relay_configuration.clone(),
            identities,
            selected,
            SessionState::SignedOut,
            None,
            None,
        )
    }

    fn replace_registry(
        &mut self,
        revision: crate::SnapshotRevision,
        identities: Vec<NostrIdentity>,
        selected: Option<PublicKey>,
    ) -> Result<AppSnapshot, SafeError> {
        self.pending_activation = None;
        AppSnapshot::ready(
            revision,
            self.snapshot.relay_configuration().clone(),
            identities,
            selected,
            SessionState::SignedOut,
            None,
            None,
        )
    }

    fn replace_registry_preserving_session(
        &mut self,
        revision: crate::SnapshotRevision,
        identities: Vec<NostrIdentity>,
        selected: Option<PublicKey>,
    ) -> Result<AppSnapshot, SafeError> {
        self.pending_activation = None;
        AppSnapshot::ready(
            revision,
            self.snapshot.relay_configuration().clone(),
            identities,
            selected,
            self.snapshot.session(),
            self.snapshot.active_identity().cloned(),
            None,
        )
    }

    fn select(
        &self,
        revision: crate::SnapshotRevision,
        public_key: PublicKey,
    ) -> Result<AppSnapshot, SafeError> {
        self.require_identity(public_key)?;
        self.copy_ready(
            revision,
            Some(public_key),
            self.snapshot.session(),
            self.snapshot.active_identity().cloned(),
            None,
        )
    }

    fn begin_activation(
        &mut self,
        revision: crate::SnapshotRevision,
        public_key: PublicKey,
    ) -> Result<AppSnapshot, SafeError> {
        self.require_identity(public_key)?;
        if self.pending_activation.is_some() {
            return Err(invalid_application_state());
        }
        self.pending_activation = Some((
            public_key,
            PreviousSession {
                session: self.snapshot.session(),
                active_identity: self.snapshot.active_identity().cloned(),
            },
        ));
        self.copy_ready(
            revision,
            self.snapshot.selected_identity(),
            SessionState::Activating(public_key),
            self.snapshot.active_identity().cloned(),
            None,
        )
    }

    fn activation_succeeded(
        &mut self,
        revision: crate::SnapshotRevision,
        active_identity: ActiveIdentitySnapshot,
    ) -> Result<AppSnapshot, SafeError> {
        let Some((target, _previous)) = self.pending_activation.as_ref() else {
            return Err(invalid_application_state());
        };
        if active_identity.identity().public_key() != *target {
            return Err(invalid_application_state());
        }
        let target = *target;
        self.pending_activation = None;
        self.copy_ready(
            revision,
            Some(target),
            SessionState::Active,
            Some(active_identity),
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
            self.snapshot.selected_identity(),
            previous.session,
            previous.active_identity,
            Some(problem),
        )
    }

    fn sign_out(&mut self, revision: crate::SnapshotRevision) -> Result<AppSnapshot, SafeError> {
        self.pending_activation = None;
        self.copy_ready(
            revision,
            self.snapshot.selected_identity(),
            SessionState::SignedOut,
            None,
            None,
        )
    }

    fn update_active_identity(
        &self,
        revision: crate::SnapshotRevision,
        expected: PublicKey,
        active_identity: ActiveIdentitySnapshot,
        problem: Option<SafeError>,
    ) -> Result<AppSnapshot, SafeError> {
        if !matches!(self.snapshot.session(), SessionState::Active)
            || self
                .snapshot
                .active_identity()
                .map(|active| active.identity().public_key())
                != Some(expected)
            || active_identity.identity().public_key() != expected
        {
            return Err(invalid_application_state());
        }
        self.copy_ready(
            revision,
            self.snapshot.selected_identity(),
            SessionState::Active,
            Some(active_identity),
            problem,
        )
    }

    fn require_identity(&self, public_key: PublicKey) -> Result<(), SafeError> {
        if self
            .snapshot
            .identities()
            .iter()
            .any(|identity| identity.public_key() == public_key)
        {
            Ok(())
        } else {
            Err(identity_not_found())
        }
    }

    fn copy_ready(
        &self,
        revision: crate::SnapshotRevision,
        selected_identity: Option<PublicKey>,
        session: SessionState,
        active_identity: Option<ActiveIdentitySnapshot>,
        recoverable_problem: Option<SafeError>,
    ) -> Result<AppSnapshot, SafeError> {
        AppSnapshot::ready(
            revision,
            self.snapshot.relay_configuration().clone(),
            self.snapshot.identities().to_vec(),
            selected_identity,
            session,
            active_identity,
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

const fn identity_not_found() -> SafeError {
    SafeError::new(
        SafeErrorCode::IdentityNotFound,
        SafeMessage::new("The identity was not found."),
    )
}

#[cfg(test)]
mod tests {
    use harvestcircle_domain::{
        IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference, SafeError,
        SafeErrorCode, SafeMessage, SignerAvailability, UnixTimestamp,
    };

    use crate::{
        ActiveIdentitySnapshot, ProfileLoadState, RelayConfiguration, RelayConnectionState,
        SessionState, StateMachine, StateTransition,
    };

    fn identity(key_byte: u8) -> NostrIdentity {
        let public_key =
            crate::test_support::valid_test_public_key(key_byte).expect("valid public key");
        NostrIdentity::new(
            NostrIdentityReference::derive(public_key).expect("identity"),
            LocalKeyringBinding::new(public_key, SignerAvailability::Available),
            None,
            IdentityCreatedAt::new(UnixTimestamp::from_seconds(1).expect("valid time")),
            None,
        )
        .expect("identity")
    }

    fn active(identity: NostrIdentity) -> ActiveIdentitySnapshot {
        ActiveIdentitySnapshot::new(
            identity,
            RelayConnectionState::Disconnected,
            ProfileLoadState::Empty,
            None,
        )
    }

    #[test]
    fn state_machine_command_trace_preserves_working_session_on_failed_replacement() {
        let first = identity(1);
        let second = identity(2);
        let mut machine = StateMachine::booting();
        let relays = RelayConfiguration::default();
        let problem = SafeError::new(
            SafeErrorCode::CredentialMissing,
            SafeMessage::new("The identity credential is missing."),
        );

        machine
            .apply(StateTransition::Bootstrap, &relays)
            .expect("bootstrap");
        machine
            .apply(
                StateTransition::ReplaceRegistry {
                    identities: vec![first.clone(), second.clone()],
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
                .active_identity()
                .map(|value| value.identity().public_key()),
            Some(first.public_key())
        );
        assert_eq!(restored.session(), SessionState::Active);
        assert_eq!(restored.selected_identity(), Some(second.public_key()));
        assert_eq!(
            restored
                .active_identity()
                .map(|value| value.identity().public_key()),
            Some(first.public_key())
        );
        assert_eq!(restored.recoverable_problem(), Some(problem));
        assert_eq!(restored.revision().value(), 7);
    }

    #[test]
    fn state_machine_rejects_missing_targets_and_signs_out_without_deleting() {
        let identity = identity(1);
        let mut machine = StateMachine::booting();
        let relays = RelayConfiguration::default();
        machine
            .apply(StateTransition::Bootstrap, &relays)
            .expect("bootstrap");
        machine
            .apply(
                StateTransition::ReplaceRegistry {
                    identities: vec![identity.clone()],
                    selected: Some(identity.public_key()),
                },
                &relays,
            )
            .expect("load registry");

        let error = machine
            .apply(
                StateTransition::Select(
                    crate::test_support::valid_test_public_key(9).expect("valid public key"),
                ),
                &relays,
            )
            .expect_err("missing identity");
        assert_eq!(error.code(), SafeErrorCode::IdentityNotFound);

        machine
            .apply(
                StateTransition::BeginActivation(identity.public_key()),
                &relays,
            )
            .expect("begin activation");
        machine
            .apply(
                StateTransition::ActivationSucceeded(Box::new(active(identity.clone()))),
                &relays,
            )
            .expect("activate");
        let signed_out = machine
            .apply(StateTransition::SignOut, &relays)
            .expect("sign out");

        assert_eq!(signed_out.identities(), &[identity]);
        assert_eq!(signed_out.session(), SessionState::SignedOut);
        assert!(signed_out.active_identity().is_none());
    }

    #[test]
    fn activation_state_policy_rejects_every_stale_or_mismatched_transition() {
        let first = identity(1);
        let second = identity(2);
        let relays = RelayConfiguration::default();
        let problem = SafeError::new(
            SafeErrorCode::CredentialMissing,
            SafeMessage::new("The identity credential is missing."),
        );
        let mut machine = StateMachine::booting();
        machine
            .apply(
                StateTransition::BootstrapRegistry {
                    identities: vec![first.clone(), second.clone()],
                    selected: Some(first.public_key()),
                },
                &relays,
            )
            .expect("registry");
        let unchanged = machine
            .apply(
                StateTransition::BootstrapRegistry {
                    identities: Vec::new(),
                    selected: None,
                },
                &relays,
            )
            .expect("repeated bootstrap is idempotent");
        assert_eq!(unchanged.identities().len(), 2);

        assert!(
            machine
                .apply(
                    StateTransition::ActivationSucceeded(Box::new(active(first.clone()))),
                    &relays,
                )
                .is_err()
        );
        assert!(
            machine
                .apply(StateTransition::ActivationFailed(problem), &relays)
                .is_err()
        );
        machine
            .apply(
                StateTransition::BeginActivation(first.public_key()),
                &relays,
            )
            .expect("begin activation");
        assert!(
            machine
                .apply(
                    StateTransition::BeginActivation(second.public_key()),
                    &relays,
                )
                .is_err()
        );
        assert!(
            machine
                .apply(
                    StateTransition::ActivationSucceeded(Box::new(active(second.clone()))),
                    &relays,
                )
                .is_err()
        );
        machine
            .apply(
                StateTransition::ActivationSucceeded(Box::new(active(first.clone()))),
                &relays,
            )
            .expect("activate first");

        for (expected, candidate) in [
            (second.public_key(), first.clone()),
            (first.public_key(), second.clone()),
        ] {
            assert!(
                machine
                    .apply(
                        StateTransition::UpdateActiveIdentity {
                            expected,
                            active_identity: Box::new(active(candidate)),
                            problem: None,
                        },
                        &relays,
                    )
                    .is_err()
            );
        }

        machine
            .apply(StateTransition::SignOut, &relays)
            .expect("sign out");
        assert!(
            machine
                .apply(
                    StateTransition::UpdateActiveIdentity {
                        expected: first.public_key(),
                        active_identity: Box::new(active(first)),
                        problem: None,
                    },
                    &relays,
                )
                .is_err()
        );
    }
}
