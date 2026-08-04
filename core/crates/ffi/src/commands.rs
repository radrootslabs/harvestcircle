use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use radroots_studio_application::{
    Clock, DurableRequestId, GeneratedKeyRecoveryHandle, RelayRuntimeMode,
    RemovalConfirmationToken, SdkNostrClient, relay_configuration_from_environment,
};
use radroots_studio_domain::{PublicKey, SafeError, SecretKeyInput, UnixTimestamp};
use radroots_studio_storage::{OsKeyringSecretStore, RuntimeActorHandle};

use crate::{
    AccountDto, AppSnapshotDto, WireErrorCategory, WireErrorCode, WireRecoveryAction,
    dto::error_policy,
};

const DATABASE_QUALIFIER: &str = "org";
const DATABASE_ORGANIZATION: &str = "radroots";
const DATABASE_APPLICATION: &str = "studio";
const DATABASE_FILENAME: &str = "studio.sqlite3";
pub(crate) const ACTOR_MAILBOX_CAPACITY: usize = 64;
pub const FFI_CONTRACT_MAJOR: u16 = 2;
pub const FFI_CONTRACT_MINOR: u16 = 0;
pub const FFI_CONTRACT_HASH: &str = "radroots-studio-native-v2-2026-08-03";
const MAX_COMMAND_DEADLINE_MILLIS: u64 = 30_000;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct RequestContextDto {
    pub request_id: String,
    pub expected_revision: u64,
    pub deadline_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct AccountCommandReceiptDto {
    pub request_id: String,
    pub committed_revision: u64,
    pub snapshot: AppSnapshotDto,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct CompatibilityDescriptor {
    pub contract_major: u16,
    pub contract_minor: u16,
    pub contract_hash: String,
    pub minimum_schema_version: u32,
    pub current_schema_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct CompatibilityExpectation {
    pub contract_major: u16,
    pub minimum_contract_minor: u16,
    pub contract_hash: String,
    pub minimum_schema_version: u32,
    pub maximum_schema_version: u32,
}

#[uniffi::export]
pub fn compatibility_descriptor() -> CompatibilityDescriptor {
    CompatibilityDescriptor {
        contract_major: FFI_CONTRACT_MAJOR,
        contract_minor: FFI_CONTRACT_MINOR,
        contract_hash: FFI_CONTRACT_HASH.to_owned(),
        minimum_schema_version: 5,
        current_schema_version: radroots_studio_storage::CURRENT_SCHEMA_VERSION,
    }
}

#[derive(Debug, uniffi::Error)]
pub enum StudioError {
    Failure {
        code: WireErrorCode,
        category: WireErrorCategory,
        retryable: bool,
        recovery_action: WireRecoveryAction,
        correlation_id: Option<String>,
        safe_message: String,
    },
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
        let (category, retryable, recovery_action) = error_policy(error.code());
        Self::Failure {
            code: error.code().into(),
            category,
            retryable,
            recovery_action,
            correlation_id: None,
            safe_message: error.message().as_str().to_owned(),
        }
    }
}

impl StudioError {
    fn correlated(error: SafeError, correlation_id: &str) -> Self {
        let (category, retryable, recovery_action) = error_policy(error.code());
        Self::Failure {
            code: error.code().into(),
            category,
            retryable,
            recovery_action,
            correlation_id: Some(correlation_id.to_owned()),
            safe_message: error.message().as_str().to_owned(),
        }
    }
}

#[derive(uniffi::Object)]
pub struct GeneratedRecoveryRequest {
    handle: GeneratedKeyRecoveryHandle,
    resolved: AtomicBool,
}

#[uniffi::export]
impl GeneratedRecoveryRequest {
    pub fn account(&self) -> AccountDto {
        self.handle.view().account().into()
    }

    pub fn expires_at_seconds(&self) -> i64 {
        self.handle.view().expires_at().as_seconds()
    }

    /// Returns the recovery secret exactly once.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error after the first read.
    pub fn take_recovery_nsec(&self) -> Result<String, StudioError> {
        self.handle
            .take_recovery_nsec()
            .map(|nsec| nsec.with_exposed_secret(str::to_owned))
            .map_err(StudioError::from)
    }
}

#[derive(uniffi::Object)]
pub struct RemovalRequest {
    public_key_hex: String,
    deletes_local_credential: bool,
    signs_out: bool,
    expires_at_seconds: i64,
    token: Mutex<Option<RemovalConfirmationToken>>,
}

#[uniffi::export]
impl RemovalRequest {
    pub fn public_key_hex(&self) -> String {
        self.public_key_hex.clone()
    }

    pub fn deletes_local_credential(&self) -> bool {
        self.deletes_local_credential
    }

    pub fn signs_out(&self) -> bool {
        self.signs_out
    }

    pub fn expires_at_seconds(&self) -> i64 {
        self.expires_at_seconds
    }
}

pub(crate) struct RuntimeCore {
    pub(crate) actor: RuntimeActorHandle,
    pub(crate) observers: Mutex<
        BTreeMap<radroots_studio_application::ChangeSubscriptionId, tokio::task::JoinHandle<()>>,
    >,
    pub(crate) closed: AtomicBool,
}

impl RuntimeCore {
    pub(crate) fn snapshot_dto(&self) -> AppSnapshotDto {
        AppSnapshotDto::from_runtime(&self.actor.snapshot(), self.actor.lifecycle())
    }

    pub(crate) fn dto_for(
        &self,
        snapshot: &radroots_studio_application::AppSnapshot,
    ) -> AppSnapshotDto {
        AppSnapshotDto::from_runtime(snapshot, self.actor.lifecycle())
    }
}

#[derive(uniffi::Object)]
pub struct StudioAppCore {
    pub(crate) inner: Arc<RuntimeCore>,
}

#[uniffi::export]
impl StudioAppCore {
    /// Verifies the static contract before touching the application data path.
    ///
    /// # Errors
    ///
    /// Returns a safe compatibility error without opening or migrating storage.
    #[uniffi::constructor]
    #[allow(clippy::needless_pass_by_value)]
    pub fn open_compatible(
        expectation: CompatibilityExpectation,
        development_mode: bool,
    ) -> Result<Arc<Self>, StudioError> {
        let path = canonical_database_path()?;
        Self::open_path_compatible(&path, &expectation, development_mode)
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
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(StudioError::from)
    }

    #[must_use]
    pub fn snapshot(&self) -> AppSnapshotDto {
        self.inner.snapshot_dto()
    }

    /// Begins the exclusive generated-account recovery flow without persistence.
    ///
    /// # Errors
    ///
    /// Returns a safe key-generation, conflict, timeout, or lifecycle error.
    pub async fn begin_generated_account_v2(
        &self,
    ) -> Result<Arc<GeneratedRecoveryRequest>, StudioError> {
        self.inner
            .actor
            .begin_generated_key_stage()
            .await
            .map(|handle| {
                Arc::new(GeneratedRecoveryRequest {
                    handle,
                    resolved: AtomicBool::new(false),
                })
            })
            .map_err(StudioError::from)
    }

    /// Acknowledges recovery and commits the generated account once.
    ///
    /// # Errors
    ///
    /// Returns a safe recovery, credential, persistence, timeout, or lifecycle error.
    pub async fn acknowledge_generated_account_v2(
        &self,
        request: Arc<GeneratedRecoveryRequest>,
    ) -> Result<AppSnapshotDto, StudioError> {
        if request.resolved.swap(true, Ordering::AcqRel) {
            return Err(confirmation_expired());
        }
        self.inner
            .actor
            .acknowledge_generated_key_stage(request.handle.id())
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(StudioError::from)
    }

    /// Cancels the exclusive generated-account recovery flow.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or lifecycle error.
    pub async fn cancel_generated_account_v2(
        &self,
        request: Arc<GeneratedRecoveryRequest>,
    ) -> Result<bool, StudioError> {
        if request.resolved.swap(true, Ordering::AcqRel) {
            return Ok(false);
        }
        self.inner
            .actor
            .cancel_generated_key_stage()
            .await
            .map_err(StudioError::from)
    }

    /// Imports or repairs an account using a caller-owned idempotency key.
    ///
    /// # Errors
    ///
    /// Returns a correlated validation, conflict, timeout, credential, or storage error.
    pub async fn import_account_v2(
        &self,
        context: RequestContextDto,
        secret_key: Vec<u8>,
    ) -> Result<AccountCommandReceiptDto, StudioError> {
        let request_id = DurableRequestId::parse(context.request_id.clone())
            .map_err(|error| StudioError::correlated(error, &context.request_id))?;
        let timeout = command_timeout(context.deadline_millis, &context.request_id)?;
        let input = SecretKeyInput::parse_bytes(secret_key)
            .map_err(|error| StudioError::correlated(error, &context.request_id))?;
        self.inner
            .actor
            .import_secret_key_request(
                request_id,
                radroots_studio_application::SnapshotRevision::from_value(
                    context.expected_revision,
                ),
                input,
                timeout,
            )
            .await
            .map(|_| {
                let snapshot = self.inner.snapshot_dto();
                AccountCommandReceiptDto {
                    request_id: context.request_id.clone(),
                    committed_revision: snapshot.revision,
                    snapshot,
                }
            })
            .map_err(|error| StudioError::correlated(error, &context.request_id))
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
            .map(|snapshot| self.inner.dto_for(&snapshot))
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
            .map(|snapshot| self.inner.dto_for(&snapshot))
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
            .map(|snapshot| self.inner.dto_for(&snapshot))
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
            .map(|snapshot| self.inner.dto_for(&snapshot))
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
                let impact = token.impact();
                Arc::new(RemovalRequest {
                    public_key_hex,
                    deletes_local_credential: impact.deletes_local_credential(),
                    signs_out: impact.signs_out(),
                    expires_at_seconds: token.expires_at().as_seconds(),
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
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(StudioError::from)
    }
}

fn verify_compatibility(expectation: &CompatibilityExpectation) -> Result<(), StudioError> {
    let actual = compatibility_descriptor();
    if expectation.contract_major != actual.contract_major
        || expectation.minimum_contract_minor > actual.contract_minor
        || expectation.contract_hash != actual.contract_hash
        || expectation.minimum_schema_version > actual.current_schema_version
        || expectation.maximum_schema_version < actual.minimum_schema_version
    {
        return Err(compatibility_mismatch());
    }
    Ok(())
}

impl StudioAppCore {
    fn open_path_compatible(
        path: &Path,
        expectation: &CompatibilityExpectation,
        development_mode: bool,
    ) -> Result<Arc<Self>, StudioError> {
        verify_compatibility(expectation)?;
        std::fs::create_dir_all(path.parent().ok_or_else(path_unavailable)?)
            .map_err(|_| path_unavailable())?;
        Self::open_path(path, development_mode)
    }

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

fn command_timeout(millis: u64, correlation_id: &str) -> Result<Duration, StudioError> {
    if millis == 0 || millis > MAX_COMMAND_DEADLINE_MILLIS {
        return Err(StudioError::Failure {
            code: WireErrorCode::InvalidApplicationState,
            category: WireErrorCategory::Input,
            retryable: false,
            recovery_action: WireRecoveryAction::None,
            correlation_id: Some(correlation_id.to_owned()),
            safe_message: "The command deadline is invalid.".to_owned(),
        });
    }
    Ok(Duration::from_millis(millis))
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
        code: WireErrorCode::StorageUnavailable,
        category: WireErrorCategory::Storage,
        retryable: true,
        recovery_action: WireRecoveryAction::RestartApplication,
        correlation_id: None,
        safe_message: "The application data directory is unavailable.".to_owned(),
    }
}

fn confirmation_expired() -> StudioError {
    StudioError::Failure {
        code: WireErrorCode::InvalidApplicationState,
        category: WireErrorCategory::Lifecycle,
        retryable: false,
        recovery_action: WireRecoveryAction::None,
        correlation_id: None,
        safe_message: "The account removal confirmation is no longer valid.".to_owned(),
    }
}

fn compatibility_mismatch() -> StudioError {
    StudioError::Failure {
        code: WireErrorCode::CompatibilityMismatch,
        category: WireErrorCategory::Compatibility,
        retryable: false,
        recovery_action: WireRecoveryAction::UpdateApplication,
        correlation_id: None,
        safe_message: "The application and native runtime are incompatible.".to_owned(),
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
        ACTOR_MAILBOX_CAPACITY, CompatibilityExpectation, DATABASE_APPLICATION, DATABASE_FILENAME,
        DATABASE_ORGANIZATION, DATABASE_QUALIFIER, FFI_CONTRACT_HASH, FFI_CONTRACT_MAJOR,
        FFI_CONTRACT_MINOR, RequestContextDto, RuntimeCore, StudioAppCore, SystemClock,
        compatibility_descriptor, runtime, verify_compatibility,
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

    #[tokio::test]
    async fn request_context_import_replays_one_committed_receipt() {
        let core = in_memory_core();
        let initial = core.snapshot();
        let context = RequestContextDto {
            request_id: "ffi-test-import-1".to_owned(),
            expected_revision: initial.revision,
            deadline_millis: 5_000,
        };
        let secret = b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
        let first = core
            .import_account_v2(context.clone(), secret.to_vec())
            .await
            .expect("first import");
        let replay = core
            .import_account_v2(context, secret.to_vec())
            .await
            .expect("replayed import");

        assert_eq!(first, replay);
        assert_eq!(first.snapshot.accounts.len(), 1);
        assert_eq!(first.request_id, "ffi-test-import-1");
    }

    #[tokio::test]
    async fn generated_recovery_handle_is_one_use_and_acknowledgement_gated() {
        let core = in_memory_core();
        let initial = core.snapshot();
        let recovery = core
            .begin_generated_account_v2()
            .await
            .expect("begin recovery");

        assert_eq!(core.snapshot(), initial);
        let nsec = recovery.take_recovery_nsec().expect("one-use nsec");
        assert!(nsec.starts_with("nsec1"));
        assert!(recovery.take_recovery_nsec().is_err());
        let committed = core
            .acknowledge_generated_account_v2(Arc::clone(&recovery))
            .await
            .expect("acknowledge");
        assert_eq!(committed.accounts.len(), 1);
        assert!(
            core.acknowledge_generated_account_v2(recovery)
                .await
                .is_err()
        );
    }

    #[test]
    fn compatibility_matrix_rejects_before_storage_mutation() {
        let actual = compatibility_descriptor();
        let compatible = CompatibilityExpectation {
            contract_major: FFI_CONTRACT_MAJOR,
            minimum_contract_minor: FFI_CONTRACT_MINOR,
            contract_hash: FFI_CONTRACT_HASH.to_owned(),
            minimum_schema_version: 5,
            maximum_schema_version: CURRENT_SCHEMA_VERSION,
        };
        verify_compatibility(&compatible).expect("compatible");

        for incompatible in [
            CompatibilityExpectation {
                contract_major: FFI_CONTRACT_MAJOR + 1,
                ..compatible.clone()
            },
            CompatibilityExpectation {
                minimum_contract_minor: FFI_CONTRACT_MINOR + 1,
                ..compatible.clone()
            },
            CompatibilityExpectation {
                contract_hash: "wrong-contract".to_owned(),
                ..compatible.clone()
            },
            CompatibilityExpectation {
                minimum_schema_version: actual.current_schema_version + 1,
                ..compatible.clone()
            },
            CompatibilityExpectation {
                maximum_schema_version: actual.minimum_schema_version - 1,
                ..compatible.clone()
            },
        ] {
            assert!(verify_compatibility(&incompatible).is_err());
        }

        let directory = tempfile::tempdir().expect("directory");
        let rejected = directory.path().join("rejected").join("studio.sqlite3");
        let incompatible = CompatibilityExpectation {
            contract_major: FFI_CONTRACT_MAJOR + 1,
            ..compatible
        };
        assert!(StudioAppCore::open_path_compatible(&rejected, &incompatible, true).is_err());
        assert!(!rejected.parent().expect("parent").exists());
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

    #[test]
    fn superseded_v1_ffi_commands_are_absent() {
        let commands = include_str!("commands.rs");
        let observer = include_str!("observer.rs");
        for forbidden in [
            format!("pub async fn {}_account(", "generate"),
            format!("pub async fn {}_secret_key(", "import"),
            format!("pub fn {}(development_mode", "open"),
            format!("pub async fn {}(", "subscribe"),
            format!("pub fn {}(&self)", "shutdown"),
        ] {
            assert!(!commands.contains(&forbidden));
            assert!(!observer.contains(&forbidden));
        }
    }
}
