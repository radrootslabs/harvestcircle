use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use radroots_studio_application::{
    Clock, RelayRuntimeMode, RemovalConfirmationToken, SdkNostrClient, SecretStore,
    relay_configuration_from_environment,
};
use radroots_studio_domain::{PublicKey, SafeError, SecretKeyInput, UnixTimestamp};
use radroots_studio_storage::{OsKeyringSecretStore, PersistentAppCore};

use crate::{AccountDto, AppSnapshotDto};

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
    pub(crate) adapter: PersistentAppCore,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) clock: SystemClock,
    pub(crate) nostr: SdkNostrClient,
    pub(crate) observers: Mutex<BTreeSet<radroots_studio_application::ObserverHandle>>,
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
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            inner
                .adapter
                .bootstrap(inner.secrets.as_ref(), &inner.clock)
                .map(|snapshot| (&snapshot).into())
        })
        .await
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshotDto {
        (&self.inner.adapter.core().snapshot()).into()
    }

    /// Generates and stores one local account with a one-time backup receipt.
    ///
    /// # Errors
    ///
    /// Returns a safe keyring, storage, or account error.
    pub async fn generate_account(&self) -> Result<GeneratedAccountDto, StudioError> {
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            let receipt = inner
                .adapter
                .generate_account(inner.secrets.as_ref(), &inner.clock)?;
            Ok(GeneratedAccountDto {
                account: receipt.account().into(),
                snapshot: (&inner.adapter.core().snapshot()).into(),
                nsec: receipt.generated_nsec().with_exposed_secret(str::to_owned),
            })
        })
        .await
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
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            inner
                .adapter
                .import_secret_key(input, inner.secrets.as_ref(), &inner.clock)?;
            Ok((&inner.adapter.core().snapshot()).into())
        })
        .await
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
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            inner
                .adapter
                .select_account(public_key)
                .map(|snapshot| (&snapshot).into())
        })
        .await
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
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            inner
                .adapter
                .activate_account(public_key, inner.secrets.as_ref(), &inner.clock)
                .map(|snapshot| (&snapshot).into())
        })
        .await
    }

    /// Signs out while retaining accounts and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error.
    pub async fn sign_out(&self) -> Result<AppSnapshotDto, StudioError> {
        let inner = Arc::clone(&self.inner);
        blocking(move || inner.adapter.sign_out().map(|snapshot| (&snapshot).into())).await
    }

    /// Refreshes the active Nostr profile from configured relays.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error.
    pub async fn refresh_active_profile(&self) -> Result<AppSnapshotDto, StudioError> {
        let inner = Arc::clone(&self.inner);
        runtime()
            .spawn(async move {
                inner
                    .adapter
                    .core()
                    .refresh_active_profile(inner.adapter.database(), &inner.nostr, &inner.clock)
                    .await
                    .map(|snapshot| (&snapshot).into())
                    .map_err(StudioError::from)
            })
            .await
            .map_err(|_| runtime_unavailable())?
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
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            let token = inner.adapter.request_account_removal(public_key)?;
            Ok(Arc::new(RemovalRequest {
                public_key_hex,
                token: Mutex::new(Some(token)),
            }))
        })
        .await
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
        let inner = Arc::clone(&self.inner);
        blocking(move || {
            inner
                .adapter
                .confirm_account_removal(token, inner.secrets.as_ref(), &inner.clock)
                .map(|snapshot| (&snapshot).into())
        })
        .await
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
        let adapter = PersistentAppCore::open(path, relays)?;
        Ok(Arc::new(Self {
            inner: Arc::new(RuntimeCore {
                adapter,
                secrets: Arc::new(OsKeyringSecretStore),
                clock: SystemClock,
                nostr: SdkNostrClient::new(Duration::from_secs(5)),
                observers: Mutex::new(BTreeSet::new()),
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
    ProjectDirs::from("org", "radroots", "studio")
        .map(|project| project.data_dir().join("studio.sqlite3"))
        .ok_or_else(path_unavailable)
}

fn parse_public_key(value: &str) -> Result<PublicKey, StudioError> {
    PublicKey::from_hex(value).map_err(StudioError::from)
}

async fn blocking<T, F>(operation: F) -> Result<T, StudioError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, SafeError> + Send + 'static,
{
    runtime()
        .spawn_blocking(operation)
        .await
        .map_err(|_| runtime_unavailable())?
        .map_err(StudioError::from)
}

fn runtime() -> &'static tokio::runtime::Runtime {
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

fn runtime_unavailable() -> StudioError {
    StudioError::Failure {
        code: "InvalidApplicationState".to_owned(),
        safe_message: "The application runtime is unavailable.".to_owned(),
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
    use std::sync::Arc;

    use radroots_studio_application::RelayConfiguration;
    use radroots_studio_storage::PersistentAppCore;

    use super::{RuntimeCore, StudioAppCore, SystemClock};

    fn in_memory_core() -> Arc<StudioAppCore> {
        Arc::new(StudioAppCore {
            inner: Arc::new(RuntimeCore {
                adapter: PersistentAppCore::in_memory(RelayConfiguration::default())
                    .expect("in-memory core"),
                secrets: Arc::new(radroots_studio_application::InMemorySecretStore::default()),
                clock: SystemClock,
                nostr: radroots_studio_application::SdkNostrClient::new(
                    std::time::Duration::from_millis(10),
                ),
                observers: std::sync::Mutex::new(std::collections::BTreeSet::new()),
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
}
