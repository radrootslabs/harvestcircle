use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use directories::BaseDirs;
use harvestcircle_application::{
    Clock, DurableRequestId, GeneratedKeyRecoveryHandle, MAX_CONFIGURED_RELAYS, RelayConfiguration,
    RelayEndpointInput, RelayUrlPolicy, RemovalConfirmationToken, SecretStore,
    relay_configuration_from_endpoints,
};
use harvestcircle_domain::{
    PublicKey, SafeError, SafeErrorCode, SafeMessage, SecretKeyInput, UnixTimestamp,
};
use harvestcircle_nostr::SdkNostrClient;
use harvestcircle_runtime::{
    RuntimeActorHandle, RuntimeDependencies, UuidInstallationIdentitySource,
};
use harvestcircle_storage::OsKeyringSecretStore;
use radroots_runtime_paths::{
    InstanceId, RadrootsHostEnvironment, RadrootsPathProfile, RadrootsPathResolver,
    RadrootsPlatform, RuntimeContext, RuntimeContextBootstrap, RuntimeContextSource, ServiceId,
};
use radroots_service_sqlite::MigrationBuildIdentity;

use crate::{
    AppSnapshotDto, IdentityDto, RelayDestinationDto, RelayEndpointDto, WireErrorCategory,
    WireErrorCode, WireRecoveryAction,
    contract::{
        BUILD_JAVA_TOOLCHAIN, BUILD_KOTLIN_TOOLCHAIN, BUILD_PROVENANCE_DIGEST,
        BUILD_RADROOTS_REVISION, BUILD_RUST_TOOLCHAIN, BUILD_SOURCE_COMMIT,
        BUILD_SOURCE_DATE_EPOCH, BUILD_SOURCE_DIRTY, DISTRIBUTION_PACKAGE_VERSION,
        FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR, FFI_CONTRACT_MINOR,
        MINIMUM_SCHEMA_VERSION, PRODUCT_COORDINATE_DIGEST, PRODUCT_VERSION,
        SNAPSHOT_SCHEMA_VERSION, SOURCE_FOUNDATION_BASELINE, SOURCE_PROVENANCE_DIGEST,
    },
    dto::error_policy,
    host_runtime::HostRuntime,
    keyring_worker::BoundedKeyringWorker,
};

pub(crate) const ACTOR_MAILBOX_CAPACITY: usize = 64;
const MAX_COMMAND_DEADLINE_MILLIS: u64 = 30_000;

#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct RequestContextDto {
    pub request_id: String,
    pub expected_revision: u64,
    pub deadline_millis: u64,
}

impl fmt::Debug for RequestContextDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestContextDto")
            .field("request_id", &"<redacted>")
            .field("expected_revision", &self.expected_revision)
            .field("deadline_millis", &self.deadline_millis)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct RelayBootstrapInputDto {
    pub endpoints: Vec<RelayEndpointDto>,
}

impl fmt::Debug for RelayBootstrapInputDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayBootstrapInputDto")
            .field("endpoint_count", &self.endpoints.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct RuntimeOpenInputDto {
    pub development_mode: bool,
    pub explicit_data_directory: Option<String>,
    pub relay_input: RelayBootstrapInputDto,
}

impl fmt::Debug for RuntimeOpenInputDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeOpenInputDto")
            .field("development_mode", &self.development_mode)
            .field(
                "explicit_data_directory",
                &self.explicit_data_directory.as_ref().map(|_| "<redacted>"),
            )
            .field("relay_endpoint_count", &self.relay_input.endpoints.len())
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct IdentityCommandReceiptDto {
    pub request_id: String,
    pub committed_revision: u64,
    pub snapshot: AppSnapshotDto,
}

impl fmt::Debug for IdentityCommandReceiptDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IdentityCommandReceiptDto")
            .field("request_id", &"<redacted>")
            .field("committed_revision", &self.committed_revision)
            .field("snapshot", &self.snapshot)
            .finish()
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct BuildInfoDto {
    pub source_commit: String,
    pub source_dirty: String,
    pub radroots_revision: String,
    pub rust_toolchain: String,
    pub java_toolchain: String,
    pub kotlin_toolchain: String,
    pub provenance_digest: String,
    pub source_date_epoch: u64,
    pub ffi_contract_id: String,
    pub ffi_contract_hash: String,
    pub snapshot_schema_version: u32,
    pub minimum_storage_schema_version: u32,
    pub current_storage_schema_version: u32,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
#[must_use]
pub fn build_info() -> BuildInfoDto {
    BuildInfoDto {
        source_commit: BUILD_SOURCE_COMMIT.to_owned(),
        source_dirty: BUILD_SOURCE_DIRTY.to_owned(),
        radroots_revision: BUILD_RADROOTS_REVISION.to_owned(),
        rust_toolchain: BUILD_RUST_TOOLCHAIN.to_owned(),
        java_toolchain: BUILD_JAVA_TOOLCHAIN.to_owned(),
        kotlin_toolchain: BUILD_KOTLIN_TOOLCHAIN.to_owned(),
        provenance_digest: BUILD_PROVENANCE_DIGEST.to_owned(),
        source_date_epoch: BUILD_SOURCE_DATE_EPOCH
            .parse()
            .expect("validated build epoch"),
        ffi_contract_id: FFI_CONTRACT_ID.to_owned(),
        ffi_contract_hash: FFI_CONTRACT_HASH.to_owned(),
        snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
        minimum_storage_schema_version: MINIMUM_SCHEMA_VERSION,
        current_storage_schema_version: harvestcircle_storage::CURRENT_SCHEMA_VERSION,
    }
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

impl fmt::Debug for HarvestCircleError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure {
                code,
                category,
                retryable,
                recovery_action,
                correlation_id,
                ..
            } => formatter
                .debug_struct("HarvestCircleError::Failure")
                .field("code", code)
                .field("category", category)
                .field("retryable", retryable)
                .field("recovery_action", recovery_action)
                .field(
                    "correlation_id",
                    &correlation_id.as_ref().map(|_| "<redacted>"),
                )
                .field("safe_message", &"<redacted>")
                .finish(),
        }
    }
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
    fn correlated(error: SafeError, correlation_id: &DurableRequestId) -> Self {
        let (category, retryable, recovery_action) = error_policy(error.code());
        Self::Failure {
            code: error.code().into(),
            category,
            retryable,
            recovery_action,
            correlation_id: Some(correlation_id.as_str().to_owned()),
            safe_message: error.message().as_str().to_owned(),
        }
    }
}

#[cfg_attr(not(coverage_nightly), derive(uniffi::Object))]
pub struct GeneratedRecoveryRequest {
    handle: GeneratedKeyRecoveryHandle,
    resolution: AtomicU8,
}

const RECOVERY_PENDING: u8 = 0;
const RECOVERY_RESOLVING: u8 = 1;
const RECOVERY_RESOLVED: u8 = 2;

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl GeneratedRecoveryRequest {
    pub fn identity(&self) -> IdentityDto {
        self.handle.view().identity().into()
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
    pub(crate) runtime: tokio::runtime::Handle,
    pub(crate) host_runtime: Option<Arc<HostRuntime>>,
    pub(crate) keyring: Option<Arc<BoundedKeyringWorker>>,
    pub(crate) observers: Mutex<
        BTreeMap<
            harvestcircle_application::ChangeSubscriptionId,
            Option<tokio::task::JoinHandle<()>>,
        >,
    >,
    pub(crate) close_state: AtomicU8,
    pub(crate) close_gate: tokio::sync::Mutex<()>,
    #[cfg(test)]
    pub(crate) _test_directory: Option<Arc<tempfile::TempDir>>,
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
        self.actor.lifecycle()
    }

    pub(crate) fn is_open(&self) -> bool {
        self.close_state.load(Ordering::Acquire) == 0
    }

    pub(crate) fn ensure_open(&self) -> Result<(), HarvestCircleError> {
        if self.is_open() {
            Ok(())
        } else {
            Err(runtime_closed_error())
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
        input: RuntimeOpenInputDto,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        verify_compatibility(&expectation)?;
        let relays = validated_relay_configuration(&input.relay_input)?;
        let context = application_runtime_context(&input)?;
        Self::open_context(&context, relays)
    }

    /// Restores durable public application state.
    ///
    /// # Errors
    ///
    /// Returns a safe storage, recovery, or application-state error.
    pub async fn bootstrap(&self) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
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

    /// Begins the exclusive generated-identity recovery flow without persistence.
    ///
    /// # Errors
    ///
    /// Returns a safe key-generation, conflict, timeout, or lifecycle error.
    pub async fn begin_generated_identity(
        &self,
    ) -> Result<Arc<GeneratedRecoveryRequest>, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .begin_generated_key_stage()
            .await
            .map(|handle| {
                Arc::new(GeneratedRecoveryRequest {
                    handle,
                    resolution: AtomicU8::new(RECOVERY_PENDING),
                })
            })
            .map_err(HarvestCircleError::from)
    }

    /// Acknowledges recovery and commits the generated identity once.
    ///
    /// # Errors
    ///
    /// Returns a terminal safe recovery, credential, persistence, timeout, or lifecycle error.
    /// A failed commit must be recovered by importing the already-saved recovery key.
    pub async fn acknowledge_generated_identity(
        &self,
        context: RequestContextDto,
        request: Arc<GeneratedRecoveryRequest>,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request_id, timeout) = validate_request_context(&context)?;
        if request
            .resolution
            .compare_exchange(
                RECOVERY_PENDING,
                RECOVERY_RESOLVING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return Err(generated_recovery_expired());
        }
        let result = self
            .inner
            .actor
            .acknowledge_generated_key_stage(
                request.handle.id(),
                request_id,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await;
        request
            .resolution
            .store(RECOVERY_RESOLVED, Ordering::Release);
        result
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(generated_commit_failed)
    }

    /// Cancels the exclusive generated-identity recovery flow.
    ///
    /// # Errors
    ///
    /// Returns a safe timeout or lifecycle error.
    pub async fn cancel_generated_identity(
        &self,
        request: Arc<GeneratedRecoveryRequest>,
    ) -> Result<bool, HarvestCircleError> {
        self.inner.ensure_open()?;
        if request
            .resolution
            .compare_exchange(
                RECOVERY_PENDING,
                RECOVERY_RESOLVING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return Ok(false);
        }
        let result = self.inner.actor.cancel_generated_key_stage().await;
        request
            .resolution
            .store(RECOVERY_RESOLVED, Ordering::Release);
        result.map_err(HarvestCircleError::from)
    }

    /// Imports or repairs an identity using a caller-owned idempotency key.
    ///
    /// # Errors
    ///
    /// Returns a correlated validation, conflict, timeout, credential, or storage error.
    pub async fn import_identity(
        &self,
        context: RequestContextDto,
        secret_key: Vec<u8>,
    ) -> Result<IdentityCommandReceiptDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request_id, timeout) = validate_request_context(&context)?;
        let input = SecretKeyInput::parse_bytes(secret_key)
            .map_err(|error| HarvestCircleError::correlated(error, &request_id))?;
        self.inner
            .actor
            .import_secret_key(
                request_id.clone(),
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                input,
                timeout,
            )
            .await
            .map(|_| {
                let snapshot = self.inner.snapshot_dto();
                IdentityCommandReceiptDto {
                    request_id: context.request_id.clone(),
                    committed_revision: snapshot.revision,
                    snapshot,
                }
            })
            .map_err(|error| HarvestCircleError::correlated(error, &request_id))
    }

    /// Selects one saved identity without activating it.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key, identity, or storage error.
    pub async fn select_identity(
        &self,
        public_key_hex: String,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .select_identity(public_key)
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }

    /// Activates one saved identity after validating its credential.
    ///
    /// # Errors
    ///
    /// Returns a safe public-key, credential, identity, or storage error.
    pub async fn activate_identity(
        &self,
        public_key_hex: String,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .activate_identity(public_key)
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(HarvestCircleError::from)
    }

    /// Signs out while retaining identities and credentials.
    ///
    /// # Errors
    ///
    /// Returns a safe application-state error.
    pub async fn sign_out(&self) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
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
        self.inner.ensure_open()?;
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
    /// Returns a safe public-key or identity error.
    pub async fn request_identity_removal(
        &self,
        public_key_hex: String,
    ) -> Result<Arc<RemovalRequest>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let public_key = parse_public_key(&public_key_hex)?;
        self.inner
            .actor
            .request_identity_removal(public_key)
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

    /// Permanently removes the identity represented by a one-time request.
    ///
    /// # Errors
    ///
    /// Returns a safe confirmation, credential, recovery, or storage error.
    pub async fn confirm_identity_removal(
        &self,
        context: RequestContextDto,
        request: Arc<RemovalRequest>,
    ) -> Result<AppSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request_id, timeout) = validate_request_context(&context)?;
        let token = request
            .token
            .lock()
            .map_err(|_| internal_state_unavailable())?
            .take()
            .ok_or_else(confirmation_expired)?;
        self.inner
            .actor
            .confirm_identity_removal(
                token,
                request_id.clone(),
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(|snapshot| self.inner.dto_for(&snapshot))
            .map_err(|error| HarvestCircleError::correlated(error, &request_id))
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
    #[cfg(test)]
    fn open_context_compatible(
        context: &RuntimeContext,
        expectation: &CompatibilityExpectation,
        relay_input: RelayBootstrapInputDto,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        verify_compatibility(expectation)?;
        let relays = validated_relay_configuration(&relay_input)?;
        Self::open_context(context, relays)
    }

    // The concrete product opener binds operating-system paths, keyrings, and
    // SQLite ownership. Platform installation lanes exercise this adapter;
    // deterministic coverage owns the compatibility and runtime policies.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn open_context(
        context: &RuntimeContext,
        relays: RelayConfiguration,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        let runtime = HostRuntime::new().map_err(|()| runtime_unavailable())?;
        let runtime_handle = runtime.handle().clone();
        let keyring = BoundedKeyringWorker::new(OsKeyringSecretStore::default())
            .map_err(HarvestCircleError::from)?;
        let secrets: Arc<dyn SecretStore> = keyring.clone();
        let build = migration_build_identity()?;
        let actor_capacity = actor_mailbox_capacity()?;
        let owned_context = context.clone();
        let actor_runtime = runtime_handle.clone();
        let actor = runtime
            .block_on(async move {
                RuntimeActorHandle::open(
                    &owned_context,
                    relays,
                    RuntimeDependencies::new(
                        secrets,
                        Arc::new(SystemClock),
                        Arc::new(SdkNostrClient::new(Duration::from_secs(5))),
                        Arc::new(UuidInstallationIdentitySource),
                    ),
                    &build,
                    actor_capacity,
                    &actor_runtime,
                )
                .await
            })
            .map_err(|()| runtime_unavailable())??;
        Ok(Arc::new(Self {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: runtime_handle,
                host_runtime: Some(runtime),
                keyring: Some(keyring),
                observers: Mutex::new(BTreeMap::new()),
                close_state: AtomicU8::new(0),
                close_gate: tokio::sync::Mutex::new(()),
                #[cfg(test)]
                _test_directory: None,
            }),
        }))
    }
}

fn validated_relay_configuration(
    relay_input: &RelayBootstrapInputDto,
) -> Result<RelayConfiguration, HarvestCircleError> {
    if relay_input.endpoints.len() > MAX_CONFIGURED_RELAYS {
        return Err(invalid_relay_configuration());
    }
    let relay_endpoints = relay_input
        .endpoints
        .iter()
        .map(|endpoint| {
            RelayEndpointInput::new(
                endpoint.url.clone(),
                match endpoint.destination {
                    RelayDestinationDto::Local => RelayUrlPolicy::Local,
                    RelayDestinationDto::PrivateNetwork => RelayUrlPolicy::PrivateNetwork,
                    RelayDestinationDto::Public => RelayUrlPolicy::Public,
                },
                endpoint.read,
                endpoint.write,
            )
        })
        .collect::<Vec<_>>();
    relay_configuration_from_endpoints(&relay_endpoints).map_err(HarvestCircleError::from)
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

// BaseDirs is the production host integration boundary. Development roots are
// supplied explicitly by the desktop host and runtime_paths receives only
// validated injected values.
#[cfg_attr(coverage_nightly, coverage(off))]
fn application_runtime_context(
    input: &RuntimeOpenInputDto,
) -> Result<RuntimeContext, HarvestCircleError> {
    let (profile, root, environment, profile_source) =
        if let Some(raw_directory) = input.explicit_data_directory.as_deref() {
            if !input.development_mode || raw_directory.is_empty() {
                return Err(path_unavailable());
            }
            let directory = PathBuf::from(raw_directory);
            if !directory.is_absolute() {
                return Err(path_unavailable());
            }
            let metadata = std::fs::symlink_metadata(&directory).map_err(|_| path_unavailable())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(path_unavailable());
            }
            let canonical = std::fs::canonicalize(&directory).map_err(|_| path_unavailable())?;
            if canonical != directory {
                return Err(path_unavailable());
            }
            (
                RadrootsPathProfile::RepoLocal,
                Some(canonical),
                RadrootsHostEnvironment::default(),
                RuntimeContextSource::BootstrapCli,
            )
        } else {
            let base = BaseDirs::new().ok_or_else(path_unavailable)?;
            (
                RadrootsPathProfile::InteractiveUser,
                None,
                RadrootsHostEnvironment {
                    home_dir: Some(base.home_dir().to_path_buf()),
                    xdg_config_home: Some(base.config_dir().to_path_buf()),
                    xdg_data_home: Some(base.data_dir().to_path_buf()),
                    xdg_state_home: base.state_dir().map(Path::to_path_buf),
                    xdg_cache_home: Some(base.cache_dir().to_path_buf()),
                    xdg_runtime_dir: base.runtime_dir().map(Path::to_path_buf),
                    appdata_dir: None,
                    localappdata_dir: None,
                },
                RuntimeContextSource::SafeDefault,
            )
        };
    let resolver = RadrootsPathResolver::new(RadrootsPlatform::current(), environment);
    let bootstrap = RuntimeContextBootstrap::new(
        profile,
        root,
        profile_source,
        RuntimeContextSource::SafeDefault,
    )
    .map_err(|_| path_unavailable())?;
    RuntimeContext::resolve(
        &resolver,
        bootstrap,
        ServiceId::new("harvestcircle").map_err(|_| path_unavailable())?,
        InstanceId::new("desktop").map_err(|_| path_unavailable())?,
    )
    .map_err(|_| path_unavailable())
}

fn migration_build_identity() -> Result<MigrationBuildIdentity, HarvestCircleError> {
    MigrationBuildIdentity::new(
        PRODUCT_VERSION,
        BUILD_SOURCE_COMMIT,
        BUILD_RADROOTS_REVISION,
        BUILD_RUST_TOOLCHAIN,
        format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        "desktop",
        1,
        1,
        1,
        1,
        1,
    )
    .map_err(|_| path_unavailable())
}

fn parse_public_key(value: &str) -> Result<PublicKey, HarvestCircleError> {
    PublicKey::from_hex(value).map_err(HarvestCircleError::from)
}

fn validate_request_context(
    context: &RequestContextDto,
) -> Result<(DurableRequestId, Duration), HarvestCircleError> {
    let request_id =
        DurableRequestId::parse(&context.request_id).map_err(HarvestCircleError::from)?;
    let timeout = command_timeout(context.deadline_millis, &request_id)?;
    Ok((request_id, timeout))
}

fn command_timeout(
    millis: u64,
    correlation_id: &DurableRequestId,
) -> Result<Duration, HarvestCircleError> {
    if millis == 0 || millis > MAX_COMMAND_DEADLINE_MILLIS {
        return Err(HarvestCircleError::Failure {
            code: WireErrorCode::InvalidApplicationState,
            category: WireErrorCategory::Input,
            retryable: false,
            recovery_action: WireRecoveryAction::None,
            correlation_id: Some(correlation_id.as_str().to_owned()),
            safe_message: "The command deadline is invalid.".to_owned(),
        });
    }
    Ok(Duration::from_millis(millis))
}

#[cfg(test)]
pub(crate) async fn test_actor(
    relays: RelayConfiguration,
) -> (RuntimeActorHandle, Arc<tempfile::TempDir>) {
    let directory = Arc::new(tempfile::tempdir().expect("temporary runtime root"));
    let context = application_runtime_context(&RuntimeOpenInputDto {
        development_mode: true,
        explicit_data_directory: Some(
            directory
                .path()
                .canonicalize()
                .expect("canonical runtime root")
                .to_string_lossy()
                .into_owned(),
        ),
        relay_input: RelayBootstrapInputDto {
            endpoints: Vec::new(),
        },
    })
    .expect("runtime context");
    let build = migration_build_identity().expect("migration build identity");
    let actor = RuntimeActorHandle::open(
        &context,
        relays,
        RuntimeDependencies::new(
            Arc::new(harvestcircle_application::InMemorySecretStore::default()),
            Arc::new(SystemClock),
            Arc::new(SdkNostrClient::new(Duration::from_millis(10))),
            Arc::new(UuidInstallationIdentitySource),
        ),
        &build,
        actor_mailbox_capacity().expect("capacity"),
        &tokio::runtime::Handle::current(),
    )
    .await
    .expect("test actor");
    (actor, directory)
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

pub(crate) fn internal_state_unavailable() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::Internal,
        category: WireErrorCategory::Internal,
        retryable: false,
        recovery_action: WireRecoveryAction::RestartApplication,
        correlation_id: None,
        safe_message: "The application state is unavailable.".to_owned(),
    }
}

pub(crate) fn runtime_closed_error() -> HarvestCircleError {
    HarvestCircleError::Failure {
        code: WireErrorCode::InvalidApplicationState,
        category: WireErrorCategory::Lifecycle,
        retryable: false,
        recovery_action: WireRecoveryAction::None,
        correlation_id: None,
        safe_message: "The application runtime is closed.".to_owned(),
    }
}

fn invalid_relay_configuration() -> HarvestCircleError {
    HarvestCircleError::from(SafeError::new(
        SafeErrorCode::InvalidRelayConfiguration,
        SafeMessage::new("The Nostr relay configuration is invalid."),
    ))
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
        safe_message: "The identity removal confirmation is no longer valid.".to_owned(),
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
            "The generated identity could not be saved. Import the recovery key you saved to try again."
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
    use std::error::Error as _;
    use std::sync::Arc;

    use harvestcircle_application::{
        RelayConfiguration, RelayEndpointInput, RelayUrlPolicy, relay_configuration_from_endpoints,
    };
    use harvestcircle_domain::SafeError;
    use harvestcircle_storage::{
        CREDENTIAL_SERVICE, CURRENT_SCHEMA_VERSION, HarvestCircleStorageContract,
    };

    use super::{
        CompatibilityExpectation, FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR,
        FFI_CONTRACT_MINOR, HarvestCircleAppCore, HarvestCircleError, MAX_CONFIGURED_RELAYS,
        PRODUCT_COORDINATE_DIGEST, RelayBootstrapInputDto, RelayDestinationDto, RelayEndpointDto,
        RequestContextDto, RuntimeCore, RuntimeOpenInputDto, SNAPSHOT_SCHEMA_VERSION,
        WireErrorCategory, WireErrorCode, WireRecoveryAction, actor_mailbox_capacity,
        application_runtime_context, compatibility_descriptor, confirmation_expired,
        generated_commit_failed, path_unavailable, runtime_unavailable, test_actor,
        verify_compatibility,
    };

    async fn in_memory_core() -> Arc<HarvestCircleAppCore> {
        let (actor, directory) = test_actor(RelayConfiguration::default()).await;
        Arc::new(HarvestCircleAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: tokio::runtime::Handle::current(),
                host_runtime: None,
                keyring: None,
                observers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                close_state: std::sync::atomic::AtomicU8::new(0),
                close_gate: tokio::sync::Mutex::new(()),
                _test_directory: Some(directory),
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
            request_id: "01890f3e-7b1c-7000-8000-000000000041".to_owned(),
            expected_revision: initial.revision,
            deadline_millis: 5_000,
        };
        let secret = b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7";
        let first = core
            .import_identity(context.clone(), secret.to_vec())
            .await
            .expect("first import");
        let replay = core
            .import_identity(context, secret.to_vec())
            .await
            .expect("replayed import");

        assert_eq!(first, replay);
        assert_eq!(first.snapshot.identities.len(), 1);
        assert_eq!(first.request_id, "01890f3e-7b1c-7000-8000-000000000041");
    }

    #[tokio::test]
    async fn generated_recovery_handle_is_one_use_and_acknowledgement_gated() {
        let core = in_memory_core().await;
        let initial = core.snapshot();
        let recovery = core
            .begin_generated_identity()
            .await
            .expect("begin recovery");

        assert_eq!(core.snapshot(), initial);
        let nsec = recovery.take_recovery_nsec().expect("one-use nsec");
        assert!(nsec.starts_with("nsec1"));
        assert!(recovery.take_recovery_nsec().is_err());
        let context = RequestContextDto {
            request_id: "01890f3e-7b1c-7000-8000-000000000042".to_owned(),
            expected_revision: initial.revision,
            deadline_millis: 5_000,
        };
        let committed = core
            .acknowledge_generated_identity(context.clone(), Arc::clone(&recovery))
            .await
            .expect("acknowledge");
        assert_eq!(committed.identities.len(), 1);
        let repeated = core
            .acknowledge_generated_identity(context, recovery)
            .await
            .expect_err("repeated acknowledgement");
        assert!(matches!(
            repeated,
            HarvestCircleError::Failure { safe_message, .. }
                if safe_message == "The generated-key recovery step is no longer valid."
        ));
    }

    #[tokio::test]
    async fn identity_lifecycle_and_one_use_removal_are_exercised_through_the_ffi_boundary() {
        let core = in_memory_core().await;
        let initial = core.bootstrap().await.expect("bootstrap");
        let imported = core
            .import_identity(
                RequestContextDto {
                    request_id: "01890f3e-7b1c-7000-8000-000000000043".to_owned(),
                    expected_revision: initial.revision,
                    deadline_millis: 5_000,
                },
                b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_vec(),
            )
            .await
            .expect("import identity");
        let public_key = imported.snapshot.identities[0].public_key_hex.clone();

        let selected = core
            .select_identity(public_key.clone())
            .await
            .expect("select identity");
        let active = core
            .activate_identity(public_key.clone())
            .await
            .expect("activate identity");
        assert!(active.revision > selected.revision);
        let signed_out = core.sign_out().await.expect("sign out");
        assert!(signed_out.revision > active.revision);
        let refreshed = core
            .refresh_active_profile()
            .await
            .expect("signed-out refresh is a stable no-op");
        assert_eq!(refreshed.revision, signed_out.revision);

        let removal = core
            .request_identity_removal(public_key.clone())
            .await
            .expect("request removal");
        assert_eq!(removal.public_key_hex(), public_key);
        assert!(removal.deletes_local_credential());
        assert!(!removal.signs_out());
        assert!(removal.expires_at_seconds() > 0);
        let invalid_confirmation = core
            .confirm_identity_removal(
                RequestContextDto {
                    request_id: "secret-invalid-request".to_owned(),
                    expected_revision: signed_out.revision,
                    deadline_millis: 5_000,
                },
                Arc::clone(&removal),
            )
            .await
            .expect_err("invalid request context");
        assert!(matches!(
            invalid_confirmation,
            HarvestCircleError::Failure {
                correlation_id: None,
                ..
            }
        ));
        assert_eq!(core.snapshot().identities.len(), 1);
        let removed = core
            .confirm_identity_removal(
                RequestContextDto {
                    request_id: "01890f3e-7b1c-7000-8000-000000000044".to_owned(),
                    expected_revision: signed_out.revision,
                    deadline_millis: 5_000,
                },
                Arc::clone(&removal),
            )
            .await
            .expect("confirm removal");
        assert!(removed.identities.is_empty());
        assert!(
            core.confirm_identity_removal(
                RequestContextDto {
                    request_id: "01890f3e-7b1c-7000-8000-000000000045".to_owned(),
                    expected_revision: removed.revision,
                    deadline_millis: 5_000,
                },
                removal,
            )
            .await
            .is_err()
        );
        assert!(
            core.select_identity("not-a-public-key".to_owned())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn generated_recovery_cancellation_and_request_validation_fail_closed() {
        let core = in_memory_core().await;
        let recovery = core
            .begin_generated_identity()
            .await
            .expect("begin generated identity");
        assert_eq!(recovery.identity().public_key_hex.len(), 64);
        assert!(recovery.expires_at_seconds() > 0);
        assert!(
            core.cancel_generated_identity(Arc::clone(&recovery))
                .await
                .expect("first cancellation")
        );
        assert!(
            !core
                .cancel_generated_identity(recovery)
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
                request_id: "01890f3e-7b1c-7000-8000-000000000046".to_owned(),
                expected_revision: 0,
                deadline_millis: 0,
            },
            RequestContextDto {
                request_id: "01890f3e-7b1c-7000-8000-000000000047".to_owned(),
                expected_revision: 0,
                deadline_millis: 30_001,
            },
        ] {
            assert!(core.import_identity(context, vec![0; 32]).await.is_err());
        }
        assert!(
            core.import_identity(
                RequestContextDto {
                    request_id: "01890f3e-7b1c-7000-8000-000000000048".to_owned(),
                    expected_revision: 0,
                    deadline_millis: 5_000,
                },
                vec![0; 31],
            )
            .await
            .is_err()
        );

        let recovery = core
            .begin_generated_identity()
            .await
            .expect("begin after validation failures");
        let invalid = core
            .acknowledge_generated_identity(
                RequestContextDto {
                    request_id: "not-a-valid-request-id".to_owned(),
                    expected_revision: core.snapshot().revision,
                    deadline_millis: 5_000,
                },
                Arc::clone(&recovery),
            )
            .await
            .expect_err("invalid request");
        assert!(matches!(
            invalid,
            HarvestCircleError::Failure {
                correlation_id: None,
                ..
            }
        ));
        core.acknowledge_generated_identity(
            RequestContextDto {
                request_id: "01890f3e-7b1c-7000-8000-000000000049".to_owned(),
                expected_revision: core.snapshot().revision,
                deadline_millis: 5_000,
            },
            recovery,
        )
        .await
        .expect("valid retry retains one-shot recovery");
    }

    #[test]
    fn input_and_error_debug_are_type_safe_and_redacted() {
        let request_secret = "01890f3e-7b1c-7000-8000-00000000dead";
        let path_secret = "/Users/private/secret-data";
        let relay_secret = "wss://user:secret@example.invalid/private";
        let request = RequestContextDto {
            request_id: request_secret.to_owned(),
            expected_revision: 9,
            deadline_millis: 1_000,
        };
        let relay = crate::RelayEndpointDto {
            url: relay_secret.to_owned(),
            destination: crate::RelayDestinationDto::PrivateNetwork,
            read: true,
            write: true,
        };
        let open = RuntimeOpenInputDto {
            development_mode: true,
            explicit_data_directory: Some(path_secret.to_owned()),
            relay_input: RelayBootstrapInputDto {
                endpoints: vec![relay.clone()],
            },
        };
        let error = HarvestCircleError::Failure {
            code: WireErrorCode::Internal,
            category: WireErrorCategory::Internal,
            retryable: false,
            recovery_action: WireRecoveryAction::RestartApplication,
            correlation_id: Some(request_secret.to_owned()),
            safe_message: "A safe public message.".to_owned(),
        };
        let rendered = format!("{request:?} {relay:?} {open:?} {error:?}");
        for secret in [
            request_secret,
            path_secret,
            relay_secret,
            "A safe public message.",
        ] {
            assert!(!rendered.contains(secret));
        }
        assert!(error.source().is_none());
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
                "The identity removal confirmation is no longer valid.",
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
                "The generated identity could not be saved. Import the recovery key you saved to try again.",
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
            minimum_schema_version: 1,
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

        let oversized_relay_error = HarvestCircleAppCore::open_compatible(
            compatible.clone(),
            RuntimeOpenInputDto {
                development_mode: true,
                explicit_data_directory: Some("relative/path-must-not-be-read".to_owned()),
                relay_input: RelayBootstrapInputDto {
                    endpoints: (0..=MAX_CONFIGURED_RELAYS)
                        .map(|index| RelayEndpointDto {
                            url: format!("wss://relay-{index}.example"),
                            destination: RelayDestinationDto::Public,
                            read: true,
                            write: true,
                        })
                        .collect(),
                },
            },
        )
        .err()
        .expect("relay bound must reject before path inspection");
        assert!(matches!(
            oversized_relay_error,
            HarvestCircleError::Failure {
                code: WireErrorCode::InvalidRelayConfiguration,
                ..
            }
        ));

        let directory = tempfile::tempdir().expect("directory");
        let canonical = directory
            .path()
            .canonicalize()
            .expect("canonical directory");
        let input = RuntimeOpenInputDto {
            development_mode: true,
            explicit_data_directory: Some(canonical.to_string_lossy().into_owned()),
            relay_input: RelayBootstrapInputDto {
                endpoints: Vec::new(),
            },
        };
        let context = application_runtime_context(&input).expect("context");
        let rejected = HarvestCircleStorageContract::from_runtime_context(&context)
            .expect("storage contract")
            .paths()
            .state_database()
            .to_path_buf();
        let incompatible = CompatibilityExpectation {
            contract_major: FFI_CONTRACT_MAJOR + 1,
            ..compatible
        };
        assert!(
            HarvestCircleAppCore::open_context_compatible(
                &context,
                &incompatible,
                input.relay_input,
            )
            .is_err()
        );
        assert!(!rejected.exists());
    }

    #[test]
    fn final_product_coordinates_do_not_adopt_the_temporary_namespace() {
        assert_eq!(CREDENTIAL_SERVICE, "org.harvestcircle.desktop.nostr");
        let temporary = tempfile::tempdir().expect("directory");
        let canonical = temporary
            .path()
            .canonicalize()
            .expect("canonical directory");
        let context = application_runtime_context(&RuntimeOpenInputDto {
            development_mode: true,
            explicit_data_directory: Some(canonical.to_string_lossy().into_owned()),
            relay_input: RelayBootstrapInputDto {
                endpoints: Vec::new(),
            },
        })
        .expect("context");
        assert_eq!(context.service().as_str(), "harvestcircle");
        assert_eq!(context.instance().as_str(), "desktop");
        let database = HarvestCircleStorageContract::from_runtime_context(&context)
            .expect("storage contract")
            .paths()
            .state_database()
            .to_path_buf();
        assert!(database.ends_with("data/services/harvestcircle/desktop/state.sqlite"));
        assert!(!database.to_string_lossy().contains("harvestcircle.sqlite3"));
        assert_eq!(CURRENT_SCHEMA_VERSION, 1);
    }

    #[test]
    fn explicit_development_data_directory_is_exact_and_fail_closed() {
        let temporary = tempfile::tempdir().expect("directory");
        let canonical = temporary
            .path()
            .canonicalize()
            .expect("canonical directory");
        let relay_input = RelayBootstrapInputDto {
            endpoints: Vec::new(),
        };
        let explicit = RuntimeOpenInputDto {
            development_mode: true,
            explicit_data_directory: Some(canonical.to_string_lossy().into_owned()),
            relay_input: relay_input.clone(),
        };
        let context = application_runtime_context(&explicit).expect("explicit context");
        assert_eq!(context.repo_local_root(), Some(canonical.as_path()));
        assert!(
            HarvestCircleStorageContract::from_runtime_context(&context)
                .expect("storage contract")
                .paths()
                .state_database()
                .ends_with("data/services/harvestcircle/desktop/state.sqlite")
        );

        for rejected in [
            RuntimeOpenInputDto {
                development_mode: false,
                explicit_data_directory: explicit.explicit_data_directory.clone(),
                relay_input: relay_input.clone(),
            },
            RuntimeOpenInputDto {
                development_mode: true,
                explicit_data_directory: Some("relative/data".to_owned()),
                relay_input: relay_input.clone(),
            },
            RuntimeOpenInputDto {
                development_mode: true,
                explicit_data_directory: Some(
                    canonical.join("missing").to_string_lossy().into_owned(),
                ),
                relay_input,
            },
        ] {
            assert!(application_runtime_context(&rejected).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_development_data_directory_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().expect("directory");
        let target = temporary.path().join("target");
        std::fs::create_dir(&target).expect("target");
        let link = temporary.path().join("link");
        symlink(&target, &link).expect("link");
        let input = RuntimeOpenInputDto {
            development_mode: true,
            explicit_data_directory: Some(link.to_string_lossy().into_owned()),
            relay_input: RelayBootstrapInputDto {
                endpoints: Vec::new(),
            },
        };

        assert!(application_runtime_context(&input).is_err());
    }

    #[test]
    fn superseded_v1_ffi_commands_are_absent() {
        let commands = include_str!("commands.rs");
        let observer = include_str!("observer.rs");
        for forbidden in [
            format!("pub async fn {}_identity(", "generate"),
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
    fn invalid_relay_configuration_fails_before_runtime_mutation() {
        assert!(relay_configuration_from_endpoints(&[]).is_err());
    }

    #[test]
    fn injected_relay_input_is_explicit_profile_bound_and_fail_closed() {
        let local = relay_configuration_from_endpoints(&[
            RelayEndpointInput::new(
                "ws://localhost:8080".to_owned(),
                RelayUrlPolicy::Local,
                true,
                true,
            ),
            RelayEndpointInput::new(
                "ws://127.0.0.1:8081".to_owned(),
                RelayUrlPolicy::Local,
                true,
                true,
            ),
        ])
        .expect("explicit local profile");
        assert_eq!(local.relays()[0].url().as_str(), "ws://localhost:8080");
        assert_eq!(local.relays()[1].url().as_str(), "ws://127.0.0.1:8081");

        for input in [
            Vec::new(),
            vec![RelayEndpointInput::new(
                "https://not-a-relay.example".to_owned(),
                RelayUrlPolicy::Public,
                true,
                true,
            )],
            vec![
                RelayEndpointInput::new(
                    "ws://localhost:8080".to_owned(),
                    RelayUrlPolicy::Local,
                    true,
                    true,
                ),
                RelayEndpointInput::new(
                    "wss://relay.example".to_owned(),
                    RelayUrlPolicy::Public,
                    true,
                    true,
                ),
            ],
        ] {
            assert_eq!(
                relay_configuration_from_endpoints(&input)
                    .expect_err("invalid profile")
                    .code(),
                harvestcircle_domain::SafeErrorCode::InvalidRelayConfiguration
            );
        }
    }
}
