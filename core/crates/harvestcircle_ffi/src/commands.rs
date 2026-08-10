use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use harvestcircle_application::{
    Clock, DurableRequestId, GeneratedKeyRecoveryHandle, RelayConfiguration, RelayRuntimeMode,
    RemovalConfirmationToken, relay_configuration_from_environment,
};
use harvestcircle_domain::{PublicKey, SafeError, SecretKeyInput, UnixTimestamp};
use harvestcircle_nostr::SdkNostrClient;
use harvestcircle_product::{
    DATABASE_APPLICATION, DATABASE_FILENAME, DATABASE_ORGANIZATION, DATABASE_QUALIFIER,
    DEVELOPMENT_DATA_DIR_ENVIRONMENT,
};
use harvestcircle_runtime::{
    RuntimeActorHandle, RuntimeDependencies, UuidInstallationIdentitySource,
};
use harvestcircle_storage::OsKeyringSecretStore;

use crate::{
    AccountDto, AppSnapshotDto, WireErrorCategory, WireErrorCode, WireRecoveryAction,
    contract::{
        DISTRIBUTION_PACKAGE_VERSION, FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR,
        FFI_CONTRACT_MINOR, MINIMUM_SCHEMA_VERSION, PRODUCT_COORDINATE_DIGEST, PRODUCT_VERSION,
        SNAPSHOT_SCHEMA_VERSION, SOURCE_FOUNDATION_BASELINE, SOURCE_PROVENANCE_DIGEST,
    },
    dto::error_policy,
};

pub(crate) const ACTOR_MAILBOX_CAPACITY: usize = 64;
const MAX_COMMAND_DEADLINE_MILLIS: u64 = 30_000;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct RequestContextDto {
    pub request_id: String,
    pub expected_revision: u64,
    pub deadline_millis: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct AccountCommandReceiptDto {
    pub request_id: String,
    pub committed_revision: u64,
    pub snapshot: AppSnapshotDto,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct CompatibilityDescriptor {
    pub contract_id: String,
    pub product_version: String,
    pub cargo_package_version: String,
    pub distribution_package_version: String,
    pub contract_major: u16,
    pub contract_minor: u16,
    pub contract_hash: String,
    pub product_coordinate_digest: String,
    pub snapshot_schema_version: u32,
    pub minimum_schema_version: u32,
    pub current_schema_version: u32,
    pub source_provenance_digest: String,
    pub source_foundation_baseline: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct CompatibilityExpectation {
    pub contract_id: String,
    pub contract_major: u16,
    pub minimum_contract_minor: u16,
    pub contract_hash: String,
    pub product_coordinate_digest: String,
    pub snapshot_schema_version: u32,
    pub minimum_schema_version: u32,
    pub maximum_schema_version: u32,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn compatibility_descriptor() -> CompatibilityDescriptor {
    CompatibilityDescriptor {
        contract_id: FFI_CONTRACT_ID.to_owned(),
        product_version: PRODUCT_VERSION.to_owned(),
        cargo_package_version: env!("CARGO_PKG_VERSION").to_owned(),
        distribution_package_version: DISTRIBUTION_PACKAGE_VERSION.to_owned(),
        contract_major: FFI_CONTRACT_MAJOR,
        contract_minor: FFI_CONTRACT_MINOR,
        contract_hash: FFI_CONTRACT_HASH.to_owned(),
        product_coordinate_digest: PRODUCT_COORDINATE_DIGEST.to_owned(),
        snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
        minimum_schema_version: MINIMUM_SCHEMA_VERSION,
        current_schema_version: harvestcircle_storage::CURRENT_SCHEMA_VERSION,
        source_provenance_digest: SOURCE_PROVENANCE_DIGEST.to_owned(),
        source_foundation_baseline: SOURCE_FOUNDATION_BASELINE.to_owned(),
    }
}

#[derive(Debug)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Error))]
pub enum HarvestCircleError {
    Failure {
        code: WireErrorCode,
        category: WireErrorCategory,
        retryable: bool,
        recovery_action: WireRecoveryAction,
        correlation_id: Option<String>,
        safe_message: String,
    },
}

impl Display for HarvestCircleError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure { safe_message, .. } => formatter.write_str(safe_message),
        }
    }
}

impl std::error::Error for HarvestCircleError {}

impl From<SafeError> for HarvestCircleError {
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

impl HarvestCircleError {
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

#[cfg_attr(not(coverage_nightly), derive(uniffi::Object))]
pub struct GeneratedRecoveryRequest {
    handle: GeneratedKeyRecoveryHandle,
    resolved: AtomicBool,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
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
    pub fn take_recovery_nsec(&self) -> Result<String, HarvestCircleError> {
        self.handle
            .take_recovery_nsec()
            .map(|nsec| nsec.with_exposed_secret(str::to_owned))
            .map_err(HarvestCircleError::from)
    }
}

#[cfg_attr(not(coverage_nightly), derive(uniffi::Object))]
pub struct RemovalRequest {
    public_key_hex: String,
    deletes_local_credential: bool,
    signs_out: bool,
    expires_at_seconds: i64,
    token: Mutex<Option<RemovalConfirmationToken>>,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
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
        BTreeMap<
            harvestcircle_application::ChangeSubscriptionId,
            Option<tokio::task::JoinHandle<()>>,
        >,
    >,
    pub(crate) closed: AtomicBool,
    pub(crate) startup_relay_problem: Option<SafeError>,
}

impl RuntimeCore {
    pub(crate) fn snapshot_dto(&self) -> AppSnapshotDto {
        AppSnapshotDto::from_runtime(&self.actor.snapshot(), self.effective_lifecycle())
    }

    pub(crate) fn dto_for(
        &self,
        snapshot: &harvestcircle_application::AppSnapshot,
    ) -> AppSnapshotDto {
        AppSnapshotDto::from_runtime(snapshot, self.effective_lifecycle())
    }

    pub(crate) fn effective_lifecycle(&self) -> harvestcircle_application::RuntimeLifecycle {
        let lifecycle = self.actor.lifecycle();
        match (lifecycle, self.startup_relay_problem) {
            (harvestcircle_application::RuntimeLifecycle::Ready, Some(problem)) => {
                harvestcircle_application::RuntimeLifecycle::Degraded(problem)
            }
            _ => lifecycle,
        }
    }
}

#[cfg_attr(not(coverage_nightly), derive(uniffi::Object))]
pub struct HarvestCircleAppCore {
    pub(crate) inner: Arc<RuntimeCore>,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl HarvestCircleAppCore {
    /// Verifies the static contract before touching the application data path.
    ///
    /// # Errors
    ///
    /// Returns a safe compatibility error without opening or migrating storage.
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    #[allow(clippy::needless_pass_by_value)]
    pub fn open_compatible(
        expectation: CompatibilityExpectation,
        development_mode: bool,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        let path = application_database_path(development_mode)?;
        Self::open_path_compatible(&path, &expectation, development_mode)
    }

    /// Restores durable public application state.
    ///
    /// # Errors
    ///
    /// Returns a safe storage, recovery, or application-state error.
    pub async fn bootstrap(&self) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner
            .actor
            .bootstrap()
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
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
    ) -> Result<Arc<GeneratedRecoveryRequest>, HarvestCircleError> {
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
            .map_err(HarvestCircleError::from)
    }

    /// Acknowledges recovery and commits the generated account once.
    ///
    /// # Errors
    ///
    /// Returns a terminal safe recovery, credential, persistence, timeout, or lifecycle error.
    /// A failed commit must be recovered by importing the already-saved recovery key.
    pub async fn acknowledge_generated_account_v2(
        &self,
        context: RequestContextDto,
        request: Arc<GeneratedRecoveryRequest>,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        if request.resolved.swap(true, Ordering::AcqRel) {
            return Err(generated_recovery_expired());
        }
        let request_id = DurableRequestId::parse(context.request_id.clone())
            .map_err(|error| HarvestCircleError::correlated(error, &context.request_id))?;
        let timeout = command_timeout(context.deadline_millis, &context.request_id)?;
        self.inner
            .actor
            .acknowledge_generated_key_stage(
                request.handle.id(),
                request_id,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(generated_commit_failed)
    }

    /// Cancels the exclusive generated-account recovery flow.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or lifecycle error.
    pub async fn cancel_generated_account_v2(
        &self,
        request: Arc<GeneratedRecoveryRequest>,
    ) -> Result<bool, HarvestCircleError> {
        if request.resolved.swap(true, Ordering::AcqRel) {
            return Ok(false);
        }
        self.inner
            .actor
            .cancel_generated_key_stage()
            .await
            .map_err(HarvestCircleError::from)
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
    ) -> Result<AccountCommandReceiptDto, HarvestCircleError> {
        let request_id = DurableRequestId::parse(context.request_id.clone())
            .map_err(|error| HarvestCircleError::correlated(error, &context.request_id))?;
        let timeout = command_timeout(context.deadline_millis, &context.request_id)?;
        let input = SecretKeyInput::parse_bytes(secret_key)
            .map_err(|error| HarvestCircleError::correlated(error, &context.request_id))?;
        self.inner
            .actor
            .import_secret_key(
                request_id,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
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
            .map_err(|error| HarvestCircleError::correlated(error, &context.request_id))
    }

    /// Selects one saved account without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key, account, or storage error.
    pub async fn select_account(
        &self,
        public_key_hex: String,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .select_account(public_key)
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }

    /// Activates one saved account after validating its credential.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key, credential, account, or storage error.
    pub async fn activate_account(
        &self,
        public_key_hex: String,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .activate_account(public_key)
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }

    /// Signs out while retaining accounts and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error.
    pub async fn sign_out(&self) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner
            .actor
            .sign_out()
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }

    /// Refreshes the active Nostr profile from configured relays.
    ///
    /// # Errors
    ///
    /// Returns a safe storage or application-state error.
    pub async fn refresh_active_profile(&self) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner
            .actor
            .refresh_active_profile()
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }

    /// Issues a revision-bound removal confirmation object.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key or account error.
    pub async fn request_account_removal(
        &self,
        public_key_hex: String,
    ) -> Result<Arc<RemovalRequest>, HarvestCircleError> {
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
            .map_err(HarvestCircleError::from)
    }

    /// Permanently removes the account represented by a one-time request.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, recovery, or storage error.
    pub async fn confirm_account_removal(
        &self,
        context: RequestContextDto,
        request: Arc<RemovalRequest>,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        let token = request
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(confirmation_expired)?;
        let request_id = DurableRequestId::parse(context.request_id.clone())
            .map_err(|error| HarvestCircleError::correlated(error, &context.request_id))?;
        let timeout = command_timeout(context.deadline_millis, &context.request_id)?;
        self.inner
            .actor
            .confirm_account_removal(
                token,
                request_id,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }
}

fn verify_compatibility(expectation: &CompatibilityExpectation) -> Result<(), HarvestCircleError> {
    let actual = compatibility_descriptor();
    if expectation.contract_id != actual.contract_id
        || expectation.contract_major != actual.contract_major
        || expectation.minimum_contract_minor > actual.contract_minor
        || expectation.contract_hash != actual.contract_hash
        || expectation.product_coordinate_digest != actual.product_coordinate_digest
        || expectation.snapshot_schema_version != actual.snapshot_schema_version
        || expectation.minimum_schema_version > actual.current_schema_version
        || expectation.maximum_schema_version < actual.minimum_schema_version
    {
        return Err(compatibility_mismatch());
    }
    Ok(())
}

impl HarvestCircleAppCore {
    fn open_path_compatible(
        path: &Path,
        expectation: &CompatibilityExpectation,
        development_mode: bool,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        verify_compatibility(expectation)?;
        std::fs::create_dir_all(path.parent().ok_or_else(path_unavailable)?)
            .map_err(|_| path_unavailable())?;
        Self::open_path(path, development_mode)
    }

    // The concrete product opener binds operating-system paths, keyrings, and
    // SQLite ownership. Platform installation lanes exercise this adapter;
    // deterministic coverage owns the compatibility and runtime policies.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn open_path(path: &Path, development_mode: bool) -> Result<Arc<Self>, HarvestCircleError> {
        let mode = if development_mode {
            RelayRuntimeMode::Development
        } else {
            RelayRuntimeMode::Packaged
        };
        let (relays, startup_relay_problem) =
            local_first_relay_configuration(relay_configuration_from_environment(mode));
        let runtime = runtime()?;
        let actor = runtime.block_on(RuntimeActorHandle::open(
            path,
            relays,
            RuntimeDependencies::new(
                Arc::new(OsKeyringSecretStore::default()),
                Arc::new(SystemClock),
                Arc::new(SdkNostrClient::new(Duration::from_secs(5))),
                Arc::new(UuidInstallationIdentitySource),
            ),
            actor_mailbox_capacity()?,
            runtime.handle(),
        ))?;
        Ok(Arc::new(Self {
            inner: Arc::new(RuntimeCore {
                actor,
                observers: Mutex::new(BTreeMap::new()),
                closed: AtomicBool::new(false),
                startup_relay_problem,
            }),
        }))
    }
}

fn local_first_relay_configuration(
    configured: Result<RelayConfiguration, SafeError>,
) -> (RelayConfiguration, Option<SafeError>) {
    match configured {
        Ok(relays) => (relays, None),
        Err(problem) => (RelayConfiguration::default(), Some(problem)),
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
        UnixTimestamp::from_seconds(seconds).unwrap_or(UnixTimestamp::UNIX_EPOCH)
    }
}

// ProjectDirs and the process environment are host integration boundaries.
#[cfg_attr(coverage_nightly, coverage(off))]
fn application_database_path(development_mode: bool) -> Result<PathBuf, HarvestCircleError> {
    if development_mode && let Some(directory) = std::env::var_os(DEVELOPMENT_DATA_DIR_ENVIRONMENT)
    {
        return Ok(PathBuf::from(directory).join(DATABASE_FILENAME));
    }
    ProjectDirs::from(
        DATABASE_QUALIFIER,
        DATABASE_ORGANIZATION,
        DATABASE_APPLICATION,
    )
    .map(|project| project.data_dir().join(DATABASE_FILENAME))
    .ok_or_else(path_unavailable)
}

fn parse_public_key(value: &str) -> Result<PublicKey, HarvestCircleError> {
    PublicKey::from_hex(value).map_err(HarvestCircleError::from)
}

fn command_timeout(millis: u64, correlation_id: &str) -> Result<Duration, HarvestCircleError> {
    if millis == 0 || millis > MAX_COMMAND_DEADLINE_MILLIS {
        return Err(HarvestCircleError::Failure {
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

pub(crate) fn runtime() -> Result<&'static tokio::runtime::Runtime, HarvestCircleError> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, ()>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("harvestcircle-core")
                .build()
                .map_err(|_| ())
        })
        .as_ref()
        .map_err(|()| runtime_unavailable())
}

fn actor_mailbox_capacity() -> Result<NonZeroUsize, HarvestCircleError> {
    NonZeroUsize::new(ACTOR_MAILBOX_CAPACITY).ok_or_else(runtime_unavailable)
}

fn runtime_unavailable() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::InvalidApplicationState,
        category: WireErrorCategory::Lifecycle,
        retryable: true,
        recovery_action: WireRecoveryAction::RestartApplication,
        correlation_id: None,
        safe_message: "The application runtime is unavailable.".to_owned(),
    }
}

fn path_unavailable() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::StorageUnavailable,
        category: WireErrorCategory::Storage,
        retryable: true,
        recovery_action: WireRecoveryAction::RestartApplication,
        correlation_id: None,
        safe_message: "The application data directory is unavailable.".to_owned(),
    }
}

fn confirmation_expired() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::InvalidApplicationState,
        category: WireErrorCategory::Lifecycle,
        retryable: false,
        recovery_action: WireRecoveryAction::None,
        correlation_id: None,
        safe_message: "The account removal confirmation is no longer valid.".to_owned(),
    }
}

fn generated_recovery_expired() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::InvalidApplicationState,
        category: WireErrorCategory::Lifecycle,
        retryable: false,
        recovery_action: WireRecoveryAction::None,
        correlation_id: None,
        safe_message: "The generated-key recovery step is no longer valid.".to_owned(),
    }
}

fn generated_commit_failed(error: SafeError) -> HarvestCircleError {
    let (category, _, _) = error_policy(error.code());
    HarvestCircleError::Failure {
        code: error.code().into(),
        category,
        retryable: false,
        recovery_action: WireRecoveryAction::None,
        correlation_id: None,
        safe_message:
            "The generated account could not be saved. Import the recovery key you saved to try again."
                .to_owned(),
    }
}

fn compatibility_mismatch() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::CompatibilityMismatch,
        category: WireErrorCategory::Compatibility,
        retryable: false,
        recovery_action: WireRecoveryAction::UpdateApplication,
        correlation_id: None,
        safe_message: "The application and native runtime are incompatible.".to_owned(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;

    use harvestcircle_application::{InMemorySecretStore, RelayConfiguration};
    use harvestcircle_domain::SafeError;
    use harvestcircle_nostr::SdkNostrClient;
    use harvestcircle_runtime::{
        RuntimeActorHandle, RuntimeDependencies, UuidInstallationIdentitySource,
    };

    use harvestcircle_storage::{CREDENTIAL_SERVICE, CURRENT_SCHEMA_VERSION};

    use super::{
        ACTOR_MAILBOX_CAPACITY, CompatibilityExpectation, DATABASE_APPLICATION, DATABASE_FILENAME,
        DATABASE_ORGANIZATION, DATABASE_QUALIFIER, FFI_CONTRACT_HASH, FFI_CONTRACT_ID,
        FFI_CONTRACT_MAJOR, FFI_CONTRACT_MINOR, HarvestCircleAppCore, HarvestCircleError,
        PRODUCT_COORDINATE_DIGEST, ProjectDirs, RequestContextDto, RuntimeCore,
        SNAPSHOT_SCHEMA_VERSION, SystemClock, WireErrorCategory, WireErrorCode, WireRecoveryAction,
        actor_mailbox_capacity, compatibility_descriptor, confirmation_expired,
        generated_commit_failed, local_first_relay_configuration, path_unavailable, runtime,
        runtime_unavailable, verify_compatibility,
    };

    async fn in_memory_core() -> Arc<HarvestCircleAppCore> {
        let actor = RuntimeActorHandle::in_memory(
            RelayConfiguration::default(),
            RuntimeDependencies::new(
                Arc::new(InMemorySecretStore::default()),
                Arc::new(SystemClock),
                Arc::new(SdkNostrClient::new(std::time::Duration::from_millis(10))),
                Arc::new(UuidInstallationIdentitySource),
            ),
            NonZeroUsize::new(ACTOR_MAILBOX_CAPACITY).expect("capacity"),
            runtime().expect("runtime").handle(),
        )
        .await
        .expect("in-memory actor");
        Arc::new(HarvestCircleAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                observers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                closed: std::sync::atomic::AtomicBool::new(false),
                startup_relay_problem: None,
            }),
        })
    }

    #[tokio::test]
    async fn exported_bootstrap_and_snapshot_are_revisioned() {
        let core = in_memory_core().await;
        let bootstrapped = core.bootstrap().await.expect("bootstrap");
        let current = core.snapshot();

        assert_eq!(bootstrapped, current);
        assert_eq!(current.revision, 1);
    }

    #[tokio::test]
    async fn request_context_import_replays_one_committed_receipt() {
        let core = in_memory_core().await;
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
        let core = in_memory_core().await;
        let initial = core.snapshot();
        let recovery = core
            .begin_generated_account_v2()
            .await
            .expect("begin recovery");

        assert_eq!(core.snapshot(), initial);
        let nsec = recovery.take_recovery_nsec().expect("one-use nsec");
        assert!(nsec.starts_with("nsec1"));
        assert!(recovery.take_recovery_nsec().is_err());
        let context = RequestContextDto {
            request_id: "ffi-test-generate-1".to_owned(),
            expected_revision: initial.revision,
            deadline_millis: 5_000,
        };
        let committed = core
            .acknowledge_generated_account_v2(context.clone(), Arc::clone(&recovery))
            .await
            .expect("acknowledge");
        assert_eq!(committed.accounts.len(), 1);
        let repeated = core
            .acknowledge_generated_account_v2(context, recovery)
            .await
            .expect_err("repeated acknowledgement");
        assert!(matches!(
            repeated,
            HarvestCircleError::Failure { safe_message, .. }
                if safe_message == "The generated-key recovery step is no longer valid."
        ));
    }

    #[tokio::test]
    async fn account_lifecycle_and_one_use_removal_are_exercised_through_the_ffi_boundary() {
        let core = in_memory_core().await;
        let initial = core.bootstrap().await.expect("bootstrap");
        let imported = core
            .import_account_v2(
                RequestContextDto {
                    request_id: "ffi-lifecycle-import".to_owned(),
                    expected_revision: initial.revision,
                    deadline_millis: 5_000,
                },
                b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_vec(),
            )
            .await
            .expect("import account");
        let public_key = imported.snapshot.accounts[0].public_key_hex.clone();

        let selected = core
            .select_account(public_key.clone())
            .await
            .expect("select account");
        let active = core
            .activate_account(public_key.clone())
            .await
            .expect("activate account");
        assert!(active.revision > selected.revision);
        let signed_out = core.sign_out().await.expect("sign out");
        assert!(signed_out.revision > active.revision);
        let refreshed = core
            .refresh_active_profile()
            .await
            .expect("signed-out refresh is a stable no-op");
        assert_eq!(refreshed.revision, signed_out.revision);

        let removal = core
            .request_account_removal(public_key.clone())
            .await
            .expect("request removal");
        assert_eq!(removal.public_key_hex(), public_key);
        assert!(removal.deletes_local_credential());
        assert!(!removal.signs_out());
        assert!(removal.expires_at_seconds() > 0);
        let removed = core
            .confirm_account_removal(
                RequestContextDto {
                    request_id: "ffi-lifecycle-remove".to_owned(),
                    expected_revision: signed_out.revision,
                    deadline_millis: 5_000,
                },
                Arc::clone(&removal),
            )
            .await
            .expect("confirm removal");
        assert!(removed.accounts.is_empty());
        assert!(
            core.confirm_account_removal(
                RequestContextDto {
                    request_id: "ffi-lifecycle-remove-repeated".to_owned(),
                    expected_revision: removed.revision,
                    deadline_millis: 5_000,
                },
                removal,
            )
            .await
            .is_err()
        );
        assert!(
            core.select_account("not-a-public-key".to_owned())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn generated_recovery_cancellation_and_request_validation_fail_closed() {
        let core = in_memory_core().await;
        let recovery = core
            .begin_generated_account_v2()
            .await
            .expect("begin generated account");
        assert_eq!(recovery.account().public_key_hex.len(), 64);
        assert!(recovery.expires_at_seconds() > 0);
        assert!(
            core.cancel_generated_account_v2(Arc::clone(&recovery))
                .await
                .expect("first cancellation")
        );
        assert!(
            !core
                .cancel_generated_account_v2(recovery)
                .await
                .expect("second cancellation")
        );

        for context in [
            RequestContextDto {
                request_id: String::new(),
                expected_revision: 0,
                deadline_millis: 5_000,
            },
            RequestContextDto {
                request_id: "ffi-zero-deadline".to_owned(),
                expected_revision: 0,
                deadline_millis: 0,
            },
            RequestContextDto {
                request_id: "ffi-long-deadline".to_owned(),
                expected_revision: 0,
                deadline_millis: 30_001,
            },
        ] {
            assert!(core.import_account_v2(context, vec![0; 32]).await.is_err());
        }
        assert!(
            core.import_account_v2(
                RequestContextDto {
                    request_id: "ffi-invalid-secret".to_owned(),
                    expected_revision: 0,
                    deadline_millis: 5_000,
                },
                vec![0; 31],
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn boundary_failures_remain_typed_and_secret_safe() {
        assert_eq!(actor_mailbox_capacity().expect("capacity").get(), 64);
        for (error, code, category, retryable, recovery, message) in [
            (
                runtime_unavailable(),
                WireErrorCode::InvalidApplicationState,
                WireErrorCategory::Lifecycle,
                true,
                WireRecoveryAction::RestartApplication,
                "The application runtime is unavailable.",
            ),
            (
                path_unavailable(),
                WireErrorCode::StorageUnavailable,
                WireErrorCategory::Storage,
                true,
                WireRecoveryAction::RestartApplication,
                "The application data directory is unavailable.",
            ),
            (
                confirmation_expired(),
                WireErrorCode::InvalidApplicationState,
                WireErrorCategory::Lifecycle,
                false,
                WireRecoveryAction::None,
                "The account removal confirmation is no longer valid.",
            ),
            (
                generated_commit_failed(SafeError::new(
                    harvestcircle_domain::SafeErrorCode::StorageUnavailable,
                    harvestcircle_domain::SafeMessage::new("internal detail"),
                )),
                WireErrorCode::StorageUnavailable,
                WireErrorCategory::Storage,
                false,
                WireRecoveryAction::None,
                "The generated account could not be saved. Import the recovery key you saved to try again.",
            ),
        ] {
            assert_eq!(error.to_string(), message);
            assert!(matches!(
                error,
                HarvestCircleError::Failure {
                    code: actual_code,
                    category: actual_category,
                    retryable: actual_retryable,
                    recovery_action: actual_recovery,
                    correlation_id: None,
                    safe_message,
                } if actual_code == code
                    && actual_category == category
                    && actual_retryable == retryable
                    && actual_recovery == recovery
                    && safe_message == message
            ));
        }
    }

    #[test]
    fn compatibility_matrix_rejects_before_storage_mutation() {
        let actual = compatibility_descriptor();
        let compatible = CompatibilityExpectation {
            contract_id: FFI_CONTRACT_ID.to_owned(),
            contract_major: FFI_CONTRACT_MAJOR,
            minimum_contract_minor: FFI_CONTRACT_MINOR,
            contract_hash: FFI_CONTRACT_HASH.to_owned(),
            product_coordinate_digest: PRODUCT_COORDINATE_DIGEST.to_owned(),
            snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
            minimum_schema_version: 5,
            maximum_schema_version: CURRENT_SCHEMA_VERSION,
        };
        verify_compatibility(&compatible).expect("compatible");

        for incompatible in [
            CompatibilityExpectation {
                contract_id: "wrong-contract".to_owned(),
                ..compatible.clone()
            },
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
                product_coordinate_digest: "wrong-coordinates".to_owned(),
                ..compatible.clone()
            },
            CompatibilityExpectation {
                snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION + 1,
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
        let rejected = directory
            .path()
            .join("rejected")
            .join("harvestcircle.sqlite3");
        let incompatible = CompatibilityExpectation {
            contract_major: FFI_CONTRACT_MAJOR + 1,
            ..compatible
        };
        assert!(
            HarvestCircleAppCore::open_path_compatible(&rejected, &incompatible, true).is_err()
        );
        assert!(!rejected.parent().expect("parent").exists());
    }

    #[test]
    fn final_product_coordinates_do_not_adopt_the_temporary_namespace() {
        assert_eq!(DATABASE_QUALIFIER, "org");
        assert_eq!(DATABASE_ORGANIZATION, "harvestcircle");
        assert_eq!(DATABASE_APPLICATION, "desktop");
        assert_eq!(DATABASE_FILENAME, "harvestcircle.sqlite3");
        assert_eq!(CREDENTIAL_SERVICE, "org.harvestcircle.desktop.nostr");

        let current = ProjectDirs::from(
            DATABASE_QUALIFIER,
            DATABASE_ORGANIZATION,
            DATABASE_APPLICATION,
        )
        .expect("current product coordinates");
        let temporary =
            ProjectDirs::from("org", "radroots", "harvestcircle").expect("temporary coordinates");
        assert_ne!(current.data_dir(), temporary.data_dir());
        assert_eq!(CURRENT_SCHEMA_VERSION, 10);
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

    #[test]
    fn invalid_relay_configuration_preserves_local_startup_as_degraded() {
        let problem = SafeError::new(
            harvestcircle_domain::SafeErrorCode::InvalidRelayConfiguration,
            harvestcircle_domain::SafeMessage::new("The Nostr relay configuration is invalid."),
        );
        let (relays, degraded) = local_first_relay_configuration(Err(problem));

        assert!(relays.relays().is_empty());
        assert_eq!(degraded, Some(problem));
    }
}
