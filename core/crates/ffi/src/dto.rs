use radroots_studio_application::{
    ActiveAccountSnapshot, AppLifecycle, AppSnapshot, ProfileLoadState, RelayConnectionState,
    SessionState,
};
use radroots_studio_domain::{AccountSummary, BindingAvailability, ProfileMetadata, SafeError};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct SafeErrorDto {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum AppLifecycleDto {
    Booting,
    Ready,
    Fatal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SessionStateDto {
    SignedOut,
    Activating,
    Active,
    SigningOut,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum RelayConnectionStateDto {
    Disconnected,
    Connecting,
    Connected,
    Degraded,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum ProfileLoadStateDto {
    Empty,
    Loading,
    Cached,
    Fresh,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SignerKindDto {
    LocalSecret,
    WatchOnly,
    RemoteNip46,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum KeyAvailabilityDto {
    Available,
    CredentialMissing,
    StoreUnavailable,
    NotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ProfileDto {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
    pub picture: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct AccountDto {
    pub public_key_hex: String,
    pub npub: String,
    pub display_label: String,
    pub signer_kind: SignerKindDto,
    pub key_availability: KeyAvailabilityDto,
    pub created_at_seconds: i64,
    pub last_used_at_seconds: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ActiveAccountDto {
    pub account: AccountDto,
    pub relay_state: RelayConnectionStateDto,
    pub profile_state: ProfileLoadStateDto,
    pub profile: Option<ProfileDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct AppSnapshotDto {
    pub revision: u64,
    pub lifecycle: AppLifecycleDto,
    pub lifecycle_error: Option<SafeErrorDto>,
    pub configured_relays: Vec<String>,
    pub accounts: Vec<AccountDto>,
    pub selected_public_key_hex: Option<String>,
    pub session: SessionStateDto,
    pub session_subject_public_key_hex: Option<String>,
    pub session_error: Option<SafeErrorDto>,
    pub active_account: Option<ActiveAccountDto>,
    pub recoverable_problem: Option<SafeErrorDto>,
}

impl From<&AppSnapshot> for AppSnapshotDto {
    fn from(snapshot: &AppSnapshot) -> Self {
        let (lifecycle, lifecycle_error) = match snapshot.lifecycle() {
            AppLifecycle::Booting => (AppLifecycleDto::Booting, None),
            AppLifecycle::Ready => (AppLifecycleDto::Ready, None),
            AppLifecycle::Fatal(error) => (AppLifecycleDto::Fatal, Some(error.into())),
        };
        let (session, session_subject_public_key_hex, session_error) = match snapshot.session() {
            SessionState::SignedOut => (SessionStateDto::SignedOut, None, None),
            SessionState::Activating(public_key) => {
                (SessionStateDto::Activating, Some(public_key.to_hex()), None)
            }
            SessionState::Active => (SessionStateDto::Active, None, None),
            SessionState::SigningOut => (SessionStateDto::SigningOut, None, None),
            SessionState::Failed(error) => (SessionStateDto::Failed, None, Some(error.into())),
        };
        Self {
            revision: snapshot.revision().value(),
            lifecycle,
            lifecycle_error,
            configured_relays: snapshot
                .relay_configuration()
                .relays()
                .iter()
                .map(|relay| relay.as_str().to_owned())
                .collect(),
            accounts: snapshot.accounts().iter().map(AccountDto::from).collect(),
            selected_public_key_hex: snapshot
                .selected_account()
                .map(radroots_studio_domain::PublicKey::to_hex),
            session,
            session_subject_public_key_hex,
            session_error,
            active_account: snapshot.active_account().map(ActiveAccountDto::from),
            recoverable_problem: snapshot.recoverable_problem().map(SafeErrorDto::from),
        }
    }
}

impl From<&AccountSummary> for AccountDto {
    fn from(account: &AccountSummary) -> Self {
        Self {
            public_key_hex: account.public_key().to_hex(),
            npub: account.npub().as_str().to_owned(),
            display_label: account.display_label(),
            signer_kind: SignerKindDto::LocalSecret,
            key_availability: account.signer().availability().into(),
            created_at_seconds: account.created_at().timestamp().as_seconds(),
            last_used_at_seconds: account
                .last_used_at()
                .map(radroots_studio_domain::UnixTimestamp::as_seconds),
        }
    }
}

impl From<&ActiveAccountSnapshot> for ActiveAccountDto {
    fn from(active: &ActiveAccountSnapshot) -> Self {
        Self {
            account: active.account().into(),
            relay_state: active.relay_state().into(),
            profile_state: active.profile_state().into(),
            profile: active.profile().map(ProfileDto::from),
        }
    }
}

impl From<&ProfileMetadata> for ProfileDto {
    fn from(profile: &ProfileMetadata) -> Self {
        Self {
            name: profile.name().map(str::to_owned),
            display_name: profile.display_name().map(str::to_owned),
            nip05: profile.nip05().map(str::to_owned),
            about: profile.about().map(str::to_owned),
            picture: profile.picture().map(str::to_owned),
        }
    }
}

impl From<SafeError> for SafeErrorDto {
    fn from(error: SafeError) -> Self {
        Self {
            code: format!("{:?}", error.code()),
            message: error.message().as_str().to_owned(),
        }
    }
}

impl From<BindingAvailability> for KeyAvailabilityDto {
    fn from(value: BindingAvailability) -> Self {
        match value {
            BindingAvailability::Available => Self::Available,
            BindingAvailability::CredentialMissing => Self::CredentialMissing,
            BindingAvailability::StoreUnavailable => Self::StoreUnavailable,
        }
    }
}

impl From<RelayConnectionState> for RelayConnectionStateDto {
    fn from(value: RelayConnectionState) -> Self {
        match value {
            RelayConnectionState::Disconnected => Self::Disconnected,
            RelayConnectionState::Connecting => Self::Connecting,
            RelayConnectionState::Connected => Self::Connected,
            RelayConnectionState::Degraded => Self::Degraded,
            RelayConnectionState::Error(_) => Self::Error,
        }
    }
}

impl From<ProfileLoadState> for ProfileLoadStateDto {
    fn from(value: ProfileLoadState) -> Self {
        match value {
            ProfileLoadState::Empty => Self::Empty,
            ProfileLoadState::Loading => Self::Loading,
            ProfileLoadState::Cached => Self::Cached,
            ProfileLoadState::Fresh => Self::Fresh,
            ProfileLoadState::Error(_) => Self::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use radroots_studio_application::{AppCore, RelayConfiguration};

    use super::AppSnapshotDto;

    #[test]
    fn snapshot_dto_is_revisioned_public_and_secret_free() {
        let core = AppCore::in_memory(RelayConfiguration::default());
        let snapshot = core.bootstrap().expect("bootstrap");
        let dto = AppSnapshotDto::from(&snapshot);
        let debug = format!("{dto:?}");

        assert_eq!(dto.revision, 1);
        assert!(dto.accounts.is_empty());
        assert!(!debug.contains("nsec"));
        assert!(!debug.contains("secret_key"));
        assert!(!debug.contains("server_url"));
    }
}
