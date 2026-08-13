use std::collections::HashSet;

use harvestcircle_domain::{
    NostrIdentity, ProfileMetadata, PublicKey, RelayEndpoint, SafeError, SafeErrorCode, SafeMessage,
};

pub const MAX_CONFIGURED_RELAYS: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SnapshotRevision(u64);

impl SnapshotRevision {
    #[must_use]
    pub const fn initial() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppLifecycle {
    Booting,
    Ready,
    Fatal(SafeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    SignedOut,
    Activating(PublicKey),
    Active,
    SigningOut,
    Failed(SafeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelayConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Degraded,
    Error(SafeError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileLoadState {
    Empty,
    Loading,
    Cached,
    Fresh,
    Error(SafeError),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelayConfiguration(Vec<RelayEndpoint>);

impl RelayConfiguration {
    /// Creates a bounded, explicitly classified relay configuration.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration error before runtime or network work when
    /// the relay count exceeds the HarvestCircle policy.
    pub fn new(relays: Vec<RelayEndpoint>) -> Result<Self, SafeError> {
        if relays.len() > MAX_CONFIGURED_RELAYS {
            return Err(relay_limit_exceeded());
        }
        Ok(Self(relays))
    }

    #[must_use]
    pub fn relays(&self) -> &[RelayEndpoint] {
        &self.0
    }
}

const fn relay_limit_exceeded() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidRelayConfiguration,
        SafeMessage::new("The Nostr relay configuration exceeds its limit."),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveIdentitySnapshot {
    identity: NostrIdentity,
    relay_state: RelayConnectionState,
    profile_state: ProfileLoadState,
    profile: Option<ProfileMetadata>,
}

impl ActiveIdentitySnapshot {
    #[must_use]
    pub const fn new(
        identity: NostrIdentity,
        relay_state: RelayConnectionState,
        profile_state: ProfileLoadState,
        profile: Option<ProfileMetadata>,
    ) -> Self {
        Self {
            identity,
            relay_state,
            profile_state,
            profile,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> &NostrIdentity {
        &self.identity
    }

    #[must_use]
    pub const fn relay_state(&self) -> RelayConnectionState {
        self.relay_state
    }

    #[must_use]
    pub const fn profile_state(&self) -> ProfileLoadState {
        self.profile_state
    }

    #[must_use]
    pub const fn profile(&self) -> Option<&ProfileMetadata> {
        self.profile.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSnapshot {
    revision: SnapshotRevision,
    lifecycle: AppLifecycle,
    relay_configuration: RelayConfiguration,
    identities: Vec<NostrIdentity>,
    selected_identity: Option<PublicKey>,
    session: SessionState,
    active_identity: Option<ActiveIdentitySnapshot>,
    recoverable_problem: Option<SafeError>,
}

impl AppSnapshot {
    #[must_use]
    pub fn booting() -> Self {
        Self {
            revision: SnapshotRevision::initial(),
            lifecycle: AppLifecycle::Booting,
            relay_configuration: RelayConfiguration::default(),
            identities: Vec::new(),
            selected_identity: None,
            session: SessionState::SignedOut,
            active_identity: None,
            recoverable_problem: None,
        }
    }

    #[must_use]
    pub fn fatal(
        revision: SnapshotRevision,
        relay_configuration: RelayConfiguration,
        error: SafeError,
    ) -> Self {
        Self {
            revision,
            lifecycle: AppLifecycle::Fatal(error),
            relay_configuration,
            identities: Vec::new(),
            selected_identity: None,
            session: SessionState::SignedOut,
            active_identity: None,
            recoverable_problem: None,
        }
    }

    /// Constructs a ready immutable snapshot after validating state invariants.
    ///
    /// # Errors
    ///
    /// Returns a safe invalid-state error for duplicate identities, invalid
    /// selection, or inconsistent active-session state.
    pub fn ready(
        revision: SnapshotRevision,
        relay_configuration: RelayConfiguration,
        identities: Vec<NostrIdentity>,
        selected_identity: Option<PublicKey>,
        session: SessionState,
        active_identity: Option<ActiveIdentitySnapshot>,
        recoverable_problem: Option<SafeError>,
    ) -> Result<Self, SafeError> {
        validate_snapshot(
            &identities,
            selected_identity,
            session,
            active_identity.as_ref(),
        )?;
        Ok(Self {
            revision,
            lifecycle: AppLifecycle::Ready,
            relay_configuration,
            identities,
            selected_identity,
            session,
            active_identity,
            recoverable_problem,
        })
    }

    #[must_use]
    pub const fn revision(&self) -> SnapshotRevision {
        self.revision
    }

    #[must_use]
    pub const fn lifecycle(&self) -> AppLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn relay_configuration(&self) -> &RelayConfiguration {
        &self.relay_configuration
    }

    #[must_use]
    pub fn identities(&self) -> &[NostrIdentity] {
        &self.identities
    }

    #[must_use]
    pub const fn selected_identity(&self) -> Option<PublicKey> {
        self.selected_identity
    }

    #[must_use]
    pub const fn session(&self) -> SessionState {
        self.session
    }

    #[must_use]
    pub const fn active_identity(&self) -> Option<&ActiveIdentitySnapshot> {
        self.active_identity.as_ref()
    }

    #[must_use]
    pub const fn recoverable_problem(&self) -> Option<SafeError> {
        self.recoverable_problem
    }
}

fn validate_snapshot(
    identities: &[NostrIdentity],
    selected_identity: Option<PublicKey>,
    session: SessionState,
    active_identity: Option<&ActiveIdentitySnapshot>,
) -> Result<(), SafeError> {
    let unique_identities = identities
        .iter()
        .map(NostrIdentity::public_key)
        .collect::<HashSet<_>>();
    if unique_identities.len() != identities.len()
        || (identities.is_empty() != selected_identity.is_none())
        || selected_identity.is_some_and(|key| !unique_identities.contains(&key))
        || active_identity.is_some_and(|active| !identities.contains(active.identity()))
        || (matches!(session, SessionState::Active) && active_identity.is_none())
        || (matches!(session, SessionState::SignedOut) && active_identity.is_some())
    {
        return Err(invalid_snapshot());
    }
    Ok(())
}

const fn invalid_snapshot() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidApplicationState,
        SafeMessage::new("The application state is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use harvestcircle_domain::{
        IdentityCreatedAt, LocalKeyringBinding, NostrIdentity, NostrIdentityReference,
        RelayDestinationPolicy, RelayEndpoint, SafeErrorCode, SignerAvailability, UnixTimestamp,
    };

    use super::{
        ActiveIdentitySnapshot, AppLifecycle, AppSnapshot, ProfileLoadState, RelayConfiguration,
        RelayConnectionState, SessionState, SnapshotRevision,
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

    #[test]
    fn snapshot_boots_empty_and_secret_free() {
        let snapshot = AppSnapshot::booting();
        let debug = format!("{snapshot:?}");

        assert_eq!(snapshot.revision(), SnapshotRevision::initial());
        assert_eq!(snapshot.lifecycle(), AppLifecycle::Booting);
        assert_eq!(snapshot.session(), SessionState::SignedOut);
        assert!(snapshot.identities().is_empty());
        assert!(snapshot.selected_identity().is_none());
        assert!(snapshot.active_identity().is_none());
        assert!(snapshot.relay_configuration().relays().is_empty());
        assert!(snapshot.recoverable_problem().is_none());
        assert!(!debug.contains("nsec1"));
        assert!(!debug.contains(&"11".repeat(32)));
    }

    #[test]
    fn relay_configuration_rejects_excess_targets_before_runtime_work() {
        let relays = (0..=super::MAX_CONFIGURED_RELAYS)
            .map(|index| {
                RelayEndpoint::parse(
                    format!("wss://relay-{index}.example").as_str(),
                    RelayDestinationPolicy::Public,
                    true,
                    true,
                )
                .expect("relay")
            })
            .collect();
        let error = RelayConfiguration::new(relays).expect_err("relay limit");
        assert_eq!(error.code(), SafeErrorCode::InvalidRelayConfiguration);
    }

    #[test]
    fn revision_helper_is_monotonic_and_checked() {
        assert_eq!(
            SnapshotRevision::initial()
                .next()
                .map(SnapshotRevision::value),
            Some(1)
        );
        assert_eq!(SnapshotRevision::from_value(u64::MAX).next(), None);
    }

    #[test]
    fn ready_snapshot_requires_valid_selection_and_active_session() {
        let first = identity(1);
        let second = identity(2);
        let active = ActiveIdentitySnapshot::new(
            second.clone(),
            RelayConnectionState::Disconnected,
            ProfileLoadState::Empty,
            None,
        );
        let valid = AppSnapshot::ready(
            SnapshotRevision::from_value(1),
            RelayConfiguration::default(),
            vec![first.clone(), second.clone()],
            Some(first.public_key()),
            SessionState::Active,
            Some(active),
            None,
        )
        .expect("valid ready snapshot");

        assert_eq!(valid.lifecycle(), AppLifecycle::Ready);
        assert_eq!(valid.selected_identity(), Some(first.public_key()));
        assert_eq!(
            valid
                .active_identity()
                .map(|value| value.identity().public_key()),
            Some(second.public_key())
        );

        assert!(
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                vec![first.clone(), first],
                Some(second.public_key()),
                SessionState::SignedOut,
                None,
                None,
            )
            .is_err()
        );
        assert!(
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                vec![second.clone()],
                Some(second.public_key()),
                SessionState::Active,
                None,
                None,
            )
            .is_err()
        );

        let missing = identity(3);
        for result in [
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                Vec::new(),
                Some(missing.public_key()),
                SessionState::SignedOut,
                None,
                None,
            ),
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                vec![second.clone()],
                None,
                SessionState::SignedOut,
                None,
                None,
            ),
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                vec![second.clone()],
                Some(missing.public_key()),
                SessionState::SignedOut,
                None,
                None,
            ),
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                vec![second.clone()],
                Some(second.public_key()),
                SessionState::SignedOut,
                Some(ActiveIdentitySnapshot::new(
                    missing,
                    RelayConnectionState::Disconnected,
                    ProfileLoadState::Empty,
                    None,
                )),
                None,
            ),
            AppSnapshot::ready(
                SnapshotRevision::initial(),
                RelayConfiguration::default(),
                vec![second.clone()],
                Some(second.public_key()),
                SessionState::SignedOut,
                Some(ActiveIdentitySnapshot::new(
                    second,
                    RelayConnectionState::Disconnected,
                    ProfileLoadState::Empty,
                    None,
                )),
                None,
            ),
        ] {
            assert!(result.is_err());
        }
    }
}
