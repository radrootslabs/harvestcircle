use std::collections::HashSet;

use radroots_studio_domain::{
    AccountSummary, ProfileMetadata, PublicKey, RelayUrl, SafeError, SafeErrorCode, SafeMessage,
};

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
pub struct RelayConfiguration(Vec<RelayUrl>);

impl RelayConfiguration {
    #[must_use]
    pub fn new(relays: Vec<RelayUrl>) -> Self {
        Self(relays)
    }

    #[must_use]
    pub fn relays(&self) -> &[RelayUrl] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveAccountSnapshot {
    account: AccountSummary,
    relay_state: RelayConnectionState,
    profile_state: ProfileLoadState,
    profile: Option<ProfileMetadata>,
}

impl ActiveAccountSnapshot {
    #[must_use]
    pub const fn new(
        account: AccountSummary,
        relay_state: RelayConnectionState,
        profile_state: ProfileLoadState,
        profile: Option<ProfileMetadata>,
    ) -> Self {
        Self {
            account,
            relay_state,
            profile_state,
            profile,
        }
    }

    #[must_use]
    pub const fn account(&self) -> &AccountSummary {
        &self.account
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
    accounts: Vec<AccountSummary>,
    selected_account: Option<PublicKey>,
    session: SessionState,
    active_account: Option<ActiveAccountSnapshot>,
    recoverable_problem: Option<SafeError>,
}

impl AppSnapshot {
    #[must_use]
    pub fn booting() -> Self {
        Self {
            revision: SnapshotRevision::initial(),
            lifecycle: AppLifecycle::Booting,
            relay_configuration: RelayConfiguration::default(),
            accounts: Vec::new(),
            selected_account: None,
            session: SessionState::SignedOut,
            active_account: None,
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
            accounts: Vec::new(),
            selected_account: None,
            session: SessionState::SignedOut,
            active_account: None,
            recoverable_problem: None,
        }
    }

    /// Constructs a ready immutable snapshot after validating state invariants.
    ///
    /// # Errors
    ///
    /// Returns a safe invalid-state error for duplicate accounts, invalid
    /// selection, or inconsistent active-session state.
    pub fn ready(
        revision: SnapshotRevision,
        relay_configuration: RelayConfiguration,
        accounts: Vec<AccountSummary>,
        selected_account: Option<PublicKey>,
        session: SessionState,
        active_account: Option<ActiveAccountSnapshot>,
        recoverable_problem: Option<SafeError>,
    ) -> Result<Self, SafeError> {
        validate_snapshot(
            &accounts,
            selected_account,
            session,
            active_account.as_ref(),
        )?;
        Ok(Self {
            revision,
            lifecycle: AppLifecycle::Ready,
            relay_configuration,
            accounts,
            selected_account,
            session,
            active_account,
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
    pub fn accounts(&self) -> &[AccountSummary] {
        &self.accounts
    }

    #[must_use]
    pub const fn selected_account(&self) -> Option<PublicKey> {
        self.selected_account
    }

    #[must_use]
    pub const fn session(&self) -> SessionState {
        self.session
    }

    #[must_use]
    pub const fn active_account(&self) -> Option<&ActiveAccountSnapshot> {
        self.active_account.as_ref()
    }

    #[must_use]
    pub const fn recoverable_problem(&self) -> Option<SafeError> {
        self.recoverable_problem
    }
}

fn validate_snapshot(
    accounts: &[AccountSummary],
    selected_account: Option<PublicKey>,
    session: SessionState,
    active_account: Option<&ActiveAccountSnapshot>,
) -> Result<(), SafeError> {
    let unique_accounts = accounts
        .iter()
        .map(AccountSummary::public_key)
        .collect::<HashSet<_>>();
    if unique_accounts.len() != accounts.len()
        || (accounts.is_empty() != selected_account.is_none())
        || selected_account.is_some_and(|key| !unique_accounts.contains(&key))
        || active_account
            .is_some_and(|active| !unique_accounts.contains(&active.account().public_key()))
        || (matches!(session, SessionState::Active) && active_account.is_none())
        || (matches!(session, SessionState::SignedOut) && active_account.is_some())
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
    use radroots_studio_domain::{
        AccountCreatedAt, AccountSummary, KeyAvailability, Npub, PublicKey, SignerKind,
        UnixTimestamp,
    };

    use super::{
        ActiveAccountSnapshot, AppLifecycle, AppSnapshot, ProfileLoadState, RelayConfiguration,
        RelayConnectionState, SessionState, SnapshotRevision,
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

    #[test]
    fn snapshot_boots_empty_and_secret_free() {
        let snapshot = AppSnapshot::booting();
        let debug = format!("{snapshot:?}");

        assert_eq!(snapshot.revision(), SnapshotRevision::initial());
        assert_eq!(snapshot.lifecycle(), AppLifecycle::Booting);
        assert_eq!(snapshot.session(), SessionState::SignedOut);
        assert!(snapshot.accounts().is_empty());
        assert!(snapshot.selected_account().is_none());
        assert!(snapshot.active_account().is_none());
        assert!(snapshot.relay_configuration().relays().is_empty());
        assert!(snapshot.recoverable_problem().is_none());
        assert!(!debug.contains("nsec1"));
        assert!(!debug.contains(&"11".repeat(32)));
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
        let first = account(1);
        let second = account(2);
        let active = ActiveAccountSnapshot::new(
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
        assert_eq!(valid.selected_account(), Some(first.public_key()));
        assert_eq!(
            valid
                .active_account()
                .map(|value| value.account().public_key()),
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
    }
}
