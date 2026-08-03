use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use radroots_studio_application::{
    Clock, RelayRuntimeMode, RemovalConfirmationToken, SdkNostrClient,
    relay_configuration_from_environment,
};
use radroots_studio_domain::{PublicKey, SafeError, SecretKeyInput, UnixTimestamp};
use radroots_studio_storage::{OsKeyringSecretStore, RuntimeActorHandle};

use crate::{AccountDto, AppSnapshotDto};

const DATABASE_QUALIFIER: &str = "org";
const DATABASE_ORGANIZATION: &str = "radroots";
const DATABASE_APPLICATION: &str = "studio";
const DATABASE_FILENAME: &str = "studio.sqlite3";
pub(crate) const ACTOR_MAILBOX_CAPACITY: usize = 64;

#[derive(Debug, uniffi::Error)]
pub enum StudioError {
    Failure { code: String, safe_message: String },
}

impl Display for StudioError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure { safe_message, .. } => formatter.write_str(safe_message),
        }
    }
}

impl std::error::Error for StudioError {}

impl From<SafeError> for StudioError {
    fn from(error: SafeError) -> Self {
        Self::Failure {
            code: format!("{:?}", error.code()),
            safe_message: error.message().as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct GeneratedAccountDto {
    pub account: AccountDto,
    pub snapshot: AppSnapshotDto,
    pub nsec: String,
}

#[derive(uniffi::Object)]
pub struct RemovalRequest {
    public_key_hex: String,
    token: Mutex<Option<RemovalConfirmationToken>>,
}

#[uniffi::export]
impl RemovalRequest {
    pub fn public_key_hex(&self) -> String {
        self.public_key_hex.clone()
    }
}

pub(crate) struct RuntimeCore {
    pub(crate) actor: RuntimeActorHandle,
    pub(crate) observers: Mutex<
        BTreeMap<radroots_studio_application::ChangeSubscriptionId, tokio::task::JoinHandle<()>>,
    >,
    pub(crate) closed: AtomicBool,
}

#[derive(uniffi::Object)]
pub struct StudioAppCore {
    pub(crate) inner: Arc<RuntimeCore>,
}

#[uniffi::export]
impl StudioAppCore {
    /// Opens the canonical application database and runtime services.
    ///
    /// # Errors
    ///
    /// Returns a safe configuration or storage error.
    #[uniffi::constructor]
    pub fn open(development_mode: bool) -> Result<Arc<Self>, StudioError> {
        let path = canonical_database_path()?;
        std::fs::create_dir_all(path.parent().ok_or_else(path_unavailable)?)
            .map_err(|_| path_unavailable())?;
        Self::open_path(&path, development_mode)
    }

    /// Restores durable public application state.
    ///
    /// # Errors
    ///
    /// Returns a safe storage, recovery, or application-state error.
    pub async fn bootstrap(&self) -> Result<AppSnapshotDto, StudioError> {
        self.inner
            .actor
            .bootstrap()
            .await
            .map(|snapshot| (&snapshot).into())
            .map_err(StudioError::from)
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshotDto {
        (&self.inner.actor.snapshot()).into()
    }

    /// Generates and stores one local account with a one-time backup receipt.
    ///
    /// # Errors
    ///
    /// Returns a safe keyring, storage, or account error.
    pub async fn generate_account(&self) -> Result<GeneratedAccountDto, StudioError> {
        self.inner
            .actor
            .generate_account()
            .await
            .map(|receipt| GeneratedAccountDto {
                account: receipt.account().into(),
                snapshot: (&self.inner.actor.snapshot()).into(),
                nsec: receipt.generated_nsec().with_exposed_secret(str::to_owned),
            })
            .map_err(StudioError::from)
    }

    /// Imports one nsec or canonical secret-key hex value.
    ///
    /// # Errors
    ///
    /// Returns a safe validation, keyring, storage, or account error.
    pub async fn import_secret_key(
        &self,
        secret_key: String,
    ) -> Result<AppSnapshotDto, StudioError> {
        let input = SecretKeyInput::parse(secret_key).map_err(StudioError::from)?;
        self.inner
            .actor
            .import_secret_key(input)
            .await
            .map(|_| (&self.inner.actor.snapshot()).into())
            .map_err(StudioError::from)
    }

    /// Selects one saved account without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key, account, or storage error.
    pub async fn select_account(
        &self,
        public_key_hex: String,
    ) -> Result<AppSnapshotDto, StudioError> {
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .select_account(public_key)
            .await
            .map(|snapshot| (&snapshot).into())
            .map_err(StudioError::from)
    }

    /// Activates one saved account after validating its credential.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key, credential, account, or storage error.
    pub async fn activate_account(
        &self,
        public_key_hex: String,
    ) -> Result<AppSnapshotDto, StudioError> {
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .activate_account(public_key)
            .await
            .map(|snapshot| (&snapshot).into())
            .map_err(StudioError::from)
    }

    /// Signs out while retaining accounts and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error.
    pub async fn sign_out(&self) -> Result<AppSnapshotDto, StudioError> {
        self.inner
            .actor
            .sign_out()
            .await
            .map(|snapshot| (&snapshot).into())
            .map_err(StudioError::from)
    }

    /// Refreshes the active Nostr profile from configured relays.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error.
    pub async fn refresh_active_profile(&self) -> Result<AppSnapshotDto, StudioError> {
        self.inner
            .actor
            .refresh_active_profile()
            .await
            .map(|snapshot| (&snapshot).into())
            .map_err(StudioError::from)
    }

    /// Issues a revision-bound removal confirmation object.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key or account error.
    pub async fn request_account_removal(
        &self,
        public_key_hex: String,
    ) -> Result<Arc<RemovalRequest>, StudioError> {
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .request_account_removal(public_key)
            .await
            .map(|token| {
                Arc::new(RemovalRequest {
                    public_key_hex,
                    token: Mutex::new(Some(token)),
                })
            })
            .map_err(StudioError::from)
    }

    /// Permanently removes the account represented by a one-time request.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, recovery, or storage error.
    pub async fn confirm_account_removal(
        &self,
        request: Arc<RemovalRequest>,
    ) -> Result<AppSnapshotDto, StudioError> {
        let token = request
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(confirmation_expired)?;
        self.inner
            .actor
            .confirm_account_removal(token)
            .await
            .map(|snapshot| (&snapshot).into())
            .map_err(StudioError::from)
    }
}

impl StudioAppCore {
    fn open_path(path: &Path, development_mode: bool) -> Result<Arc<Self>, StudioError> {
        let mode = if development_mode {
            RelayRuntimeMode::Development
        } else {
            RelayRuntimeMode::Packaged
        };
        let relays = relay_configuration_from_environment(mode)?;
        let actor = RuntimeActorHandle::open(
            path,
            relays,
            Arc::new(OsKeyringSecretStore::default()),
            Arc::new(SystemClock),
            Arc::new(SdkNostrClient::new(Duration::from_secs(5))),
            NonZeroUsize::new(ACTOR_MAILBOX_CAPACITY).expect("nonzero actor mailbox capacity"),
            runtime().handle(),
        )?;
        Ok(Arc::new(Self {
            inner: Arc::new(RuntimeCore {
                actor,
                observers: Mutex::new(BTreeMap::new()),
                closed: AtomicBool::new(false),
            }),
        }))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixTimestamp {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
            });
        UnixTimestamp::from_seconds(seconds).expect("system time is nonnegative")
    }
}

fn canonical_database_path() -> Result<PathBuf, StudioError> {
    ProjectDirs::from(
        DATABASE_QUALIFIER,
        DATABASE_ORGANIZATION,
        DATABASE_APPLICATION,
    )
    .map(|project| project.data_dir().join(DATABASE_FILENAME))
    .ok_or_else(path_unavailable)
}

fn parse_public_key(value: &str) -> Result<PublicKey, StudioError> {
    PublicKey::from_hex(value).map_err(StudioError::from)
}

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("radroots-studio-core")
            .build()
            .expect("Tokio runtime construction")
    })
}

fn path_unavailable() -> StudioError {
    StudioError::Failure {
        code: "StorageUnavailable".to_owned(),
        safe_message: "The application data directory is unavailable.".to_owned(),
    }
}

fn confirmation_expired() -> StudioError {
    StudioError::Failure {
        code: "InvalidApplicationState".to_owned(),
        safe_message: "The account removal confirmation is no longer valid.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    use radroots_studio_application::{InMemorySecretStore, RelayConfiguration, SdkNostrClient};
    use radroots_studio_storage::RuntimeActorHandle;

    use radroots_studio_storage::{CREDENTIAL_SERVICE, CURRENT_SCHEMA_VERSION};

    use super::{
        ACTOR_MAILBOX_CAPACITY, DATABASE_APPLICATION, DATABASE_FILENAME, DATABASE_ORGANIZATION,
        DATABASE_QUALIFIER, RuntimeCore, StudioAppCore, SystemClock, runtime,
    };

    fn in_memory_core() -> Arc<StudioAppCore> {
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            Arc::new(InMemorySecretStore::default()),
            Arc::new(SystemClock),
            Arc::new(SdkNostrClient::new(std::time::Duration::from_millis(10))),
            NonZeroUsize::new(ACTOR_MAILBOX_CAPACITY).expect("capacity"),
            runtime().handle(),
        )
        .expect("in-memory actor");
        Arc::new(StudioAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                observers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
            }),
        })
    }

    #[tokio::test]
    async fn exported_bootstrap_and_snapshot_are_revisioned() {
        let core = in_memory_core();
        let bootstrapped = core.bootstrap().await.expect("bootstrap");
        let current = core.snapshot();

        assert_eq!(bootstrapped, current);
        assert_eq!(current.revision, 1);
    }

    #[test]
    fn v5_compatibility_fixture_preserves_external_coordinates() {
        let fixture = include_str!("../../../compatibility/v5-baseline.properties");
        let property = |key: &str| {
            fixture.lines().find_map(|line| {
                line.split_once('=')
                    .filter(|(candidate, _)| *candidate == key)
                    .map(|(_, value)| value)
            })
        };

        assert_eq!(property("baseline.id"), Some("studio-runtime-v5"));
        assert_eq!(property("schema.version"), Some("5"));
        assert_eq!(CURRENT_SCHEMA_VERSION, 9);
        assert_eq!(property("ffi.contract"), Some("legacy-unversioned-v1"));
        assert_eq!(property("ffi.snapshot.schema"), Some("1"));
        assert_eq!(property("ffi.runtime.version"), Some("0.1.0-alpha"));
        assert_eq!(property("database.qualifier"), Some(DATABASE_QUALIFIER));
        assert_eq!(
            property("database.organization"),
            Some(DATABASE_ORGANIZATION)
        );
        assert_eq!(property("database.application"), Some(DATABASE_APPLICATION));
        assert_eq!(property("database.filename"), Some(DATABASE_FILENAME));
        assert_eq!(property("keyring.service"), Some(CREDENTIAL_SERVICE));
        assert_eq!(
            property("keyring.account"),
            Some("canonical-lowercase-public-key-hex")
        );
    }
}
