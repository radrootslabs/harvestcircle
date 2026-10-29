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

use crate::hub_departure::{
    HubDepartureInputDto, HubDepartureReceiptDto, HubDepartureReviewDto, HubDepartureReviewInputDto,
};
use crate::hub_dispatch::{
    HubDispatchPageDto, HubDispatchQueryDto, HubDispatchReceiptDto, HubDispatchViewDto,
};
use crate::hub_dispatch_input::HubDispatchInputDto;
use crate::hub_handling::{
    HubDiscrepancyPageDto, HubGroupViewDto, HubHandlingInputDto, HubHandlingQueryDto,
    HubHandlingReceiptDto, HubWorkPageDto,
};
use crate::hub_portions::{HubLineagePageDto, HubLineageQueryDto};
use crate::hub_reference::{
    HubReferenceEditDto, HubReferencePageDto, HubReferenceQueryDto, HubReferenceReceiptDto,
};
use crate::hub_stock::{HubReceiptInputDto, HubStockPageDto, HubStockQueryDto, HubStockReceiptDto};
use crate::hub_trace::{
    HubLocalOutputDto, HubLocalOutputRequestDto, HubTraceHistoryPageDto, HubTraceHistoryQueryDto,
    HubTracePageDto, HubTraceQueryDto, HubTraceSnapshotDto, HubTraceSnapshotQueryDto,
};
use crate::hub_treatment::{
    HubTreatmentPageDto, HubTreatmentQueryDto, HubTreatmentScopeDto, HubTreatmentSnapshotDto,
};
use crate::{
    AppSnapshotDto, IdentityDto, RelayDestinationDto, RelayEndpointDto, WireErrorCategory,
    WireErrorCode, WireRecoveryAction,
    contract::{
        BUILD_JAVA_TOOLCHAIN, BUILD_KOTLIN_TOOLCHAIN, BUILD_PROVENANCE_DIGEST,
        BUILD_RADROOTS_REVISION, BUILD_RUST_TOOLCHAIN, BUILD_SOURCE_COMMIT,
        BUILD_SOURCE_DATE_EPOCH, BUILD_SOURCE_DIRTY, DISTRIBUTION_PACKAGE_VERSION,
        FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR, FFI_CONTRACT_MINOR,
        MINIMUM_SCHEMA_VERSION, PRODUCT_COORDINATE_DIGEST, PRODUCT_VERSION,
        SNAPSHOT_SCHEMA_VERSION,
    },
    dto::error_policy,
    host_runtime::HostRuntime,
    keyring_worker::BoundedKeyringWorker,
};

pub(crate) const ACTOR_MAILBOX_CAPACITY: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(not(coverage_nightly), derive(uniffi::Record))]
pub struct HubContextDto {
    pub hub_id: String,
    pub owner_public_key: String,
    pub revision: u64,
    pub sample_data: bool,
}

impl From<harvestcircle_domain::hub::HubOwnership> for HubContextDto {
    fn from(hub: harvestcircle_domain::hub::HubOwnership) -> Self {
        Self {
            hub_id: hub
                .id()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            owner_public_key: hub.owner().to_hex(),
            revision: hub.revision().value(),
            sample_data: true,
        }
    }
}
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
    fn with_correlation(mut self, request: &DurableRequestId) -> Self {
        let Self::Failure { correlation_id, .. } = &mut self;
        *correlation_id = Some(request.as_str().to_owned());
        self
    }

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
            Arc<crate::observer::ObserverTask>,
        >,
    >,
    pub(crate) retired_observers: Mutex<
        BTreeMap<
            harvestcircle_application::ChangeSubscriptionId,
            Arc<crate::observer::ObserverTask>,
        >,
    >,
    pub(crate) observer_admission: Arc<tokio::sync::Semaphore>,
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
        crate::bootstrap::prepare(&context).map_err(|()| HarvestCircleError::Failure {
            code: WireErrorCode::StorageUnavailable, category: WireErrorCategory::Storage,
            retryable: false, recovery_action: WireRecoveryAction::CheckConfiguration,
            correlation_id: None, safe_message: "HarvestCircle cannot prepare its local data location. Check its availability and permissions, then try again.".to_owned(),
        })?;
        Self::open_context(&context, relays)
    }

    /// Opens an explicitly selected isolated sample-data profile, offline.
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub fn open_demo_compatible(
        expectation: CompatibilityExpectation,
        run_name: String,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        verify_compatibility(&expectation)?;
        crate::bootstrap::validate_demo_run(&run_name).map_err(|()| path_unavailable())?;
        let interactive = application_runtime_context(&RuntimeOpenInputDto {
            development_mode: false,
            explicit_data_directory: None,
            relay_input: RelayBootstrapInputDto {
                endpoints: Vec::new(),
            },
        })?;
        crate::bootstrap::prepare(&interactive).map_err(|()| path_unavailable())?;
        let anchor = interactive
            .paths()
            .state()
            .ancestors()
            .nth(3)
            .ok_or_else(path_unavailable)?;
        let root = crate::bootstrap::prepare_demo_under(anchor, &run_name)
            .map_err(|()| path_unavailable())?;
        let context = application_runtime_context(&RuntimeOpenInputDto {
            development_mode: true,
            explicit_data_directory: Some(root.to_str().ok_or_else(path_unavailable)?.to_owned()),
            relay_input: RelayBootstrapInputDto {
                endpoints: Vec::new(),
            },
        })?;
        Self::open_context_with_mode(&context, RelayConfiguration::default(), true)
    }

    /// Private hub reads require the current native owner session.
    pub async fn hub_context(&self) -> Result<Option<HubContextDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_context()
            .await
            .map(|hub| hub.map(HubContextDto::from))
            .map_err(HarvestCircleError::from)
    }

    /// The owner is derived from the native session, never supplied by a client.
    pub async fn initialize_hub(
        &self,
        context: RequestContextDto,
    ) -> Result<HubContextDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request, timeout) = validate_request_context(&context)?;
        self.inner
            .actor
            .initialize_hub(
                request.clone(),
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(HubContextDto::from)
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    pub async fn hub_stock(
        &self,
        query: HubStockQueryDto,
    ) -> Result<HubStockPageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_stock(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_group(&self, group_id: String) -> Result<HubGroupViewDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let id = harvestcircle_domain::hub::reference::HubRecordId::from_hex(&group_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        self.inner
            .actor
            .hub_group(id)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_lineage(
        &self,
        query: HubLineageQueryDto,
    ) -> Result<HubLineagePageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_lineage(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_treatment(
        &self,
        scope: HubTreatmentScopeDto,
    ) -> Result<HubTreatmentSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_treatment(scope.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }
    pub async fn hub_treatment_history(
        &self,
        query: HubTreatmentQueryDto,
    ) -> Result<HubTreatmentPageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_treatment_history(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_trace_page(
        &self,
        query: HubTraceQueryDto,
    ) -> Result<HubTracePageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_trace_page(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }
    pub async fn hub_trace_snapshot(
        &self,
        query: HubTraceSnapshotQueryDto,
    ) -> Result<HubTraceSnapshotDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_trace_snapshot(query.decode()?)
            .await
            .map_err(HarvestCircleError::from)?
            .try_into()
    }
    pub async fn hub_trace_history(
        &self,
        query: HubTraceHistoryQueryDto,
    ) -> Result<HubTraceHistoryPageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_trace_history(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }
    pub async fn hub_local_output(
        &self,
        request: HubLocalOutputRequestDto,
    ) -> Result<HubLocalOutputDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_local_output(request.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_departure_review(
        &self,
        input: HubDepartureReviewInputDto,
    ) -> Result<HubDepartureReviewDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_departure_review(input.decode()?)
            .await
            .map_err(HarvestCircleError::from)?
            .try_into()
    }

    pub async fn apply_hub_departure(
        &self,
        context: RequestContextDto,
        input: HubDepartureInputDto,
    ) -> Result<HubDepartureReceiptDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request, timeout) = validate_request_context(&context)?;
        let command = input.command()?;
        let receipt = self
            .inner
            .actor
            .apply_hub_departure(
                request.clone(),
                command,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map_err(|error| HarvestCircleError::correlated(error, &request))?;
        HubDepartureReceiptDto::try_from(receipt).map_err(|error| error.with_correlation(&request))
    }
    /// Reads the original result without submitting another physical effect.
    pub async fn hub_departure_operation(
        &self,
        operation_id: String,
    ) -> Result<Option<HubDepartureReceiptDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let request = DurableRequestId::parse(operation_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        self.inner
            .actor
            .hub_departure_operation(request.clone())
            .await
            .map_err(|error| HarvestCircleError::correlated(error, &request))?
            .map(TryInto::try_into)
            .transpose()
            .map_err(|error: HarvestCircleError| error.with_correlation(&request))
    }

    pub async fn hub_dispatch_page(
        &self,
        query: HubDispatchQueryDto,
    ) -> Result<HubDispatchPageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_dispatch_page(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_dispatch_record(
        &self,
        preparation_id: String,
    ) -> Result<HubDispatchViewDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_dispatch_record(crate::hub_treatment_input::id(&preparation_id)?)
            .await
            .map_err(HarvestCircleError::from)?
            .try_into()
    }

    pub async fn apply_hub_dispatch(
        &self,
        context: RequestContextDto,
        input: HubDispatchInputDto,
    ) -> Result<HubDispatchReceiptDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request, timeout) = validate_request_context(&context)?;
        let command = input.command()?;
        let receipt = self
            .inner
            .actor
            .apply_hub_dispatch(
                request.clone(),
                command,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map_err(|error| HarvestCircleError::correlated(error, &request))?;
        HubDispatchReceiptDto::try_from(receipt).map_err(|error| error.with_correlation(&request))
    }

    /// Reads only the original result; absence does not permit another submission.
    pub async fn hub_dispatch_operation(
        &self,
        operation_id: String,
    ) -> Result<Option<HubDispatchReceiptDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let request = DurableRequestId::parse(operation_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        let receipt = self
            .inner
            .actor
            .hub_dispatch_operation(request.clone())
            .await
            .map_err(|error| HarvestCircleError::correlated(error, &request))?;
        receipt
            .map(TryInto::try_into)
            .transpose()
            .map_err(|error: HarvestCircleError| error.with_correlation(&request))
    }

    pub async fn hub_work(
        &self,
        query: HubHandlingQueryDto,
    ) -> Result<HubWorkPageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_work(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn hub_discrepancies(
        &self,
        query: HubHandlingQueryDto,
    ) -> Result<HubDiscrepancyPageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        self.inner
            .actor
            .hub_discrepancies(query.decode()?)
            .await
            .map(Into::into)
            .map_err(HarvestCircleError::from)
    }

    pub async fn apply_hub_handling(
        &self,
        context: RequestContextDto,
        input: HubHandlingInputDto,
    ) -> Result<HubHandlingReceiptDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request, timeout) = validate_request_context(&context)?;
        let command = input.command()?;
        self.inner
            .actor
            .apply_hub_handling(
                request.clone(),
                command,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(Into::into)
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    /// Reads only the original result; absence does not permit another submission.
    pub async fn hub_handling_operation(
        &self,
        operation_id: String,
    ) -> Result<Option<HubHandlingReceiptDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let request = DurableRequestId::parse(operation_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        self.inner
            .actor
            .hub_handling_operation(request.clone())
            .await
            .map(|value| value.map(Into::into))
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    pub async fn receive_hub_stock(
        &self,
        context: RequestContextDto,
        input: HubReceiptInputDto,
    ) -> Result<HubStockReceiptDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request, timeout) = validate_request_context(&context)?;
        let command = input.command(&request)?;
        self.inner
            .actor
            .receive_hub_stock(
                request.clone(),
                command,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(Into::into)
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    /// Reads the immutable original; an absent receipt never authorizes replay.
    pub async fn hub_receipt_operation(
        &self,
        operation_id: String,
    ) -> Result<Option<HubStockReceiptDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let request = DurableRequestId::parse(operation_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        self.inner
            .actor
            .hub_receipt_operation(request.clone())
            .await
            .map(|value| value.map(Into::into))
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    /// Reads one bounded, revision-pinned page under current native authority.
    pub async fn hub_references(
        &self,
        query: HubReferenceQueryDto,
    ) -> Result<HubReferencePageDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        query.validate()?;
        let catalog = self
            .inner
            .actor
            .hub_references()
            .await
            .map_err(HarvestCircleError::from)?;
        query.page(&catalog)
    }

    pub async fn apply_hub_reference(
        &self,
        context: RequestContextDto,
        edit: HubReferenceEditDto,
    ) -> Result<HubReferenceReceiptDto, HarvestCircleError> {
        self.inner.ensure_open()?;
        let (request, timeout) = validate_request_context(&context)?;
        let command = edit.command(&request)?;
        self.inner
            .actor
            .apply_hub_reference(
                request.clone(),
                command,
                harvestcircle_application::SnapshotRevision::from_value(context.expected_revision),
                timeout,
            )
            .await
            .map(Into::into)
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    /// Read the original result; absence never causes implicit replay.
    pub async fn hub_reference_operation(
        &self,
        operation_id: String,
    ) -> Result<Option<HubReferenceReceiptDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let request = DurableRequestId::parse(operation_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        self.inner
            .actor
            .hub_reference_operation(request.clone())
            .await
            .map(|receipt| receipt.map(Into::into))
            .map_err(|error| HarvestCircleError::correlated(error, &request))
    }

    pub async fn hub_reference_history(
        &self,
        record_id: String,
        after_revision: u64,
        limit: u32,
    ) -> Result<Vec<HubReferenceReceiptDto>, HarvestCircleError> {
        self.inner.ensure_open()?;
        let id = harvestcircle_domain::hub::reference::HubRecordId::from_hex(&record_id)
            .map_err(|_| crate::hub_reference::invalid_input())?;
        self.inner
            .actor
            .hub_reference_history(id, after_revision, limit)
            .await
            .map(|history| history.into_iter().map(Into::into).collect())
            .map_err(HarvestCircleError::from)
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
        Self::open_context_with_mode(context, relays, false)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn open_context_with_mode(
        context: &RuntimeContext,
        relays: RelayConfiguration,
        demo_profile: bool,
    ) -> Result<Arc<Self>, HarvestCircleError> {
        let build = migration_build_identity()?;
        let actor_capacity = actor_mailbox_capacity()?;
        let runtime = HostRuntime::new().map_err(|()| runtime_unavailable())?;
        let runtime_handle = runtime.handle().clone();
        let keyring = match BoundedKeyringWorker::new(OsKeyringSecretStore::default()) {
            Ok(keyring) => keyring,
            Err(error) => {
                finish_failed_open(&runtime, None)?;
                return Err(HarvestCircleError::from(error));
            }
        };
        let secrets: Arc<dyn SecretStore> = keyring.clone();
        let owned_context = context.clone();
        let actor_runtime = runtime_handle.clone();
        let actor_result = runtime
            .block_on(async move {
                RuntimeActorHandle::open(
                    &owned_context,
                    relays,
                    {
                        let dependencies = RuntimeDependencies::new(
                            secrets,
                            Arc::new(SystemClock),
                            Arc::new(SdkNostrClient::new(Duration::from_secs(5))),
                            Arc::new(UuidInstallationIdentitySource),
                        );
                        if demo_profile {
                            dependencies.with_demo_profile()
                        } else {
                            dependencies
                        }
                    },
                    &build,
                    actor_capacity,
                    &actor_runtime,
                )
                .await
            })
            .map_err(|()| runtime_unavailable())
            .and_then(|result| result.map_err(HarvestCircleError::from));
        let actor = match actor_result {
            Ok(actor) => actor,
            Err(error) => {
                finish_failed_open(&runtime, Some(&keyring))?;
                return Err(error);
            }
        };
        Ok(Arc::new(Self {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: runtime_handle,
                host_runtime: Some(runtime),
                keyring: Some(keyring),
                observers: Mutex::new(BTreeMap::new()),
                retired_observers: Mutex::new(BTreeMap::new()),
                observer_admission: Arc::new(tokio::sync::Semaphore::new(
                    crate::observer::MAX_OBSERVERS,
                )),
                close_state: AtomicU8::new(0),
                close_gate: tokio::sync::Mutex::new(()),
                #[cfg(test)]
                _test_directory: None,
            }),
        }))
    }
}

// A failed constructor has no public handle through which callers could await
// shutdown. Join its workers before returning a retryable storage/configuration
// error. Use a separate executor so this sync constructor also works from an
// existing Tokio caller; never ask the host runtime to join itself.
fn finish_failed_open(
    runtime: &Arc<HostRuntime>,
    keyring: Option<&Arc<BoundedKeyringWorker>>,
) -> Result<(), HarvestCircleError> {
    let completed = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let executor = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| ())?;
                executor.block_on(async {
                    let keyring_closed = if let Some(keyring) = keyring {
                        let keyring = Arc::clone(keyring);
                        matches!(
                            runtime.run(async move { keyring.close().await }).await,
                            Ok(Ok(()))
                        )
                    } else {
                        true
                    };
                    let runtime_closed = runtime.shutdown().await.is_ok();
                    if keyring_closed && runtime_closed {
                        Ok(())
                    } else {
                        Err(())
                    }
                })
            })
            .join()
            .unwrap_or(Err(()))
    });
    completed.map_err(|()| HarvestCircleError::Failure {
        code: WireErrorCode::Internal,
        category: WireErrorCategory::Lifecycle,
        retryable: false,
        recovery_action: WireRecoveryAction::RestartApplication,
        correlation_id: None,
        safe_message:
            "Startup cleanup could not complete. Close HarvestCircle before opening it again."
                .to_owned(),
    })
}

fn validated_relay_configuration(
    relay_input: &RelayBootstrapInputDto,
) -> Result<RelayConfiguration, HarvestCircleError> {
    // No configured endpoint means offline startup, never an ambient network
    // fallback. The explicit-endpoint parser below remains strict.
    if relay_input.endpoints.is_empty() {
        return Ok(RelayConfiguration::default());
    }
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
    test_actor_with_nostr_timeout(relays, Duration::from_millis(10)).await
}

#[cfg(test)]
pub(crate) async fn test_actor_with_nostr_timeout(
    relays: RelayConfiguration,
    nostr_timeout: Duration,
) -> (RuntimeActorHandle, Arc<tempfile::TempDir>) {
    test_actor_mode(relays, nostr_timeout, false).await
}

#[cfg(test)]
async fn test_actor_mode(
    relays: RelayConfiguration,
    nostr_timeout: Duration,
    demo_profile: bool,
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
    std::fs::create_dir_all(directory.path().join("data")).expect("state root");
    let build = migration_build_identity().expect("migration build identity");
    let actor = RuntimeActorHandle::open(
        &context,
        relays,
        {
            let deps = RuntimeDependencies::new(
                Arc::new(harvestcircle_application::InMemorySecretStore::default()),
                Arc::new(SystemClock),
                Arc::new(SdkNostrClient::new(nostr_timeout)),
                Arc::new(UuidInstallationIdentitySource),
            );
            if demo_profile {
                deps.with_demo_profile()
            } else {
                deps
            }
        },
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
        generated_commit_failed, path_unavailable, runtime_unavailable, verify_compatibility,
    };

    async fn in_memory_core() -> Arc<HarvestCircleAppCore> {
        in_memory_core_with_mode(false).await
    }

    async fn in_memory_core_with_mode(demo_profile: bool) -> Arc<HarvestCircleAppCore> {
        let (actor, directory) = super::test_actor_mode(
            RelayConfiguration::default(),
            std::time::Duration::from_millis(10),
            demo_profile,
        )
        .await;
        Arc::new(HarvestCircleAppCore {
            inner: Arc::new(RuntimeCore {
                actor,
                runtime: tokio::runtime::Handle::current(),
                host_runtime: None,
                keyring: None,
                observers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                retired_observers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                observer_admission: Arc::new(tokio::sync::Semaphore::new(
                    crate::observer::MAX_OBSERVERS,
                )),
                close_state: std::sync::atomic::AtomicU8::new(0),
                close_gate: tokio::sync::Mutex::new(()),
                _test_directory: Some(directory),
            }),
        })
    }

    #[tokio::test]
    async fn demo_hub_ffi_derives_owner_and_replays_original_setup() {
        let core = in_memory_core_with_mode(true).await;
        let initial = core.bootstrap().await.unwrap();
        assert!(core.hub_context().await.is_err());
        let import = core
            .import_identity(
                RequestContextDto {
                    request_id: harvestcircle_application::DurableRequestId::new_v7()
                        .as_str()
                        .to_owned(),
                    expected_revision: initial.revision,
                    deadline_millis: 5000,
                },
                b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_vec(),
            )
            .await
            .unwrap();
        let key = import.snapshot.identities[0].public_key_hex.clone();
        let active = core.activate_identity(key.clone()).await.unwrap();
        assert!(core.hub_context().await.unwrap().is_none());
        let context = RequestContextDto {
            request_id: harvestcircle_application::DurableRequestId::new_v7()
                .as_str()
                .to_owned(),
            expected_revision: active.revision,
            deadline_millis: 5000,
        };
        let hub = core.initialize_hub(context.clone()).await.unwrap();
        assert!(hub.sample_data);
        assert_eq!(hub.owner_public_key, key);
        assert_eq!(hub.revision, 1);
        assert_eq!(core.initialize_hub(context).await.unwrap(), hub);
        assert_eq!(core.hub_context().await.unwrap(), Some(hub));
        use crate::{
            HubReferenceEditDto, HubReferenceInputDto, HubReferenceKindDto, HubReferenceQueryDto,
        };
        let query = HubReferenceQueryDto {
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
            kind: None,
            text: String::new(),
        };
        let empty = core.hub_references(query.clone()).await.unwrap();
        assert_eq!(empty.total, 0);
        let reference_context = RequestContextDto {
            request_id: harvestcircle_application::DurableRequestId::new_v7()
                .as_str()
                .to_owned(),
            expected_revision: core.snapshot().revision,
            deadline_millis: 5000,
        };
        let edit = HubReferenceEditDto {
            expected_catalog_revision: empty.catalog_revision,
            record_id: None,
            expected_record_revision: None,
            value: HubReferenceInputDto {
                kind: HubReferenceKindDto::Party,
                label: "Fictional orchard".to_owned(),
                product_id: None,
                nominal_grams: None,
                location_purpose: None,
            },
        };
        let receipt = core
            .apply_hub_reference(reference_context.clone(), edit.clone())
            .await
            .unwrap();
        assert_eq!(receipt.actor_public_key, key);
        assert_eq!(receipt.operation_id, reference_context.request_id);
        assert_eq!(
            core.apply_hub_reference(reference_context.clone(), edit)
                .await
                .unwrap(),
            receipt
        );
        assert_eq!(
            core.hub_reference_operation(reference_context.request_id.clone())
                .await
                .unwrap(),
            Some(receipt.clone())
        );
        assert_eq!(
            core.hub_reference_history(receipt.current.id.clone(), 0, 64)
                .await
                .unwrap(),
            vec![receipt]
        );
        assert_eq!(core.hub_references(query.clone()).await.unwrap().total, 1);
        core.sign_out().await.unwrap();
        assert!(core.hub_context().await.is_err());
        assert!(core.hub_references(query).await.is_err());
        assert!(
            core.hub_reference_operation(reference_context.request_id)
                .await
                .is_err()
        );
        core.shutdown_v2().await.unwrap();
    }

    #[tokio::test]
    async fn hub_receiving_and_handling_ffi_preserve_authority_and_original_physical_results() {
        use crate::HubReferenceEditDto;
        use crate::{
            HubLocationPurposeDto, HubOccurrenceDto, HubOccurrenceKindDto, HubReceiptInputDto,
            HubReferenceInputDto, HubReferenceKindDto, HubSourceCertaintyDto, HubSourceInputDto,
            HubStockQueryDto,
        };
        use harvestcircle_application::DurableRequestId;
        let core = in_memory_core_with_mode(true).await;
        let initial = core.bootstrap().await.unwrap();
        let context = |revision| RequestContextDto {
            request_id: DurableRequestId::new_v7().as_str().to_owned(),
            expected_revision: revision,
            deadline_millis: 5000,
        };
        let imported = core
            .import_identity(
                context(initial.revision),
                b"7e7e9c42a91bfef19fa7ea99d52d8afdb67d893a8fefba1f5cb9793f2107f6d7".to_vec(),
            )
            .await
            .unwrap();
        let key = imported.snapshot.identities[0].public_key_hex.clone();
        core.activate_identity(key.clone()).await.unwrap();
        core.initialize_hub(context(core.snapshot().revision))
            .await
            .unwrap();
        let mut ids: Vec<String> = Vec::new();
        let mut catalog_revision = 1;
        for (kind, label, purpose) in [
            (HubReferenceKindDto::Product, "Fictional apples", None),
            (HubReferenceKindDto::Pack, "Original crate", None),
            (
                HubReferenceKindDto::Location,
                "Receiving",
                Some(HubLocationPurposeDto::Receiving),
            ),
            (
                HubReferenceKindDto::Location,
                "A3",
                Some(HubLocationPurposeDto::Storage),
            ),
        ] {
            let product_id = if kind == HubReferenceKindDto::Pack {
                Some(ids[0].clone())
            } else {
                None
            };
            let saved = core
                .apply_hub_reference(
                    context(core.snapshot().revision),
                    HubReferenceEditDto {
                        expected_catalog_revision: catalog_revision,
                        record_id: None,
                        expected_record_revision: None,
                        value: HubReferenceInputDto {
                            kind,
                            label: label.to_owned(),
                            product_id,
                            nominal_grams: None,
                            location_purpose: purpose,
                        },
                    },
                )
                .await
                .unwrap();
            catalog_revision = saved.catalog_revision;
            ids.push(saved.current.id);
        }
        let query = HubStockQueryDto {
            expected_catalog_revision: None,
            expected_stock_revision: None,
            offset: 0,
            limit: 64,
            location_id: None,
            trace_id: None,
            text: String::new(),
        };
        let empty = core.hub_stock(query.clone()).await.unwrap();
        assert_eq!(empty.total, 0);
        let input = HubReceiptInputDto {
            expected_stock_revision: empty.stock_revision,
            expected_catalog_revision: catalog_revision,
            variant_id: ids[1].clone(),
            supplier_id: None,
            producer_id: None,
            source: HubSourceInputDto {
                existing_source_id: None,
                issuer_id: None,
                kind: None,
                code: None,
                provenance: None,
                certainty: HubSourceCertaintyDto::Unknown,
            },
            whole_packs: 20,
            receiving_id: ids[2].clone(),
            planned_location_id: Some(ids[3].clone()),
            occurrence: HubOccurrenceDto {
                kind: HubOccurrenceKindDto::Unknown,
                start_unix_s: None,
                end_unix_s: None,
            },
            observation: "Manually counted twenty original crates".into(),
        };
        let original_context = context(core.snapshot().revision);
        let recorded = core
            .receive_hub_stock(original_context.clone(), input.clone())
            .await
            .unwrap();
        assert_eq!(recorded.operation_id, original_context.request_id);
        assert_eq!(recorded.trace.actor_public_key, key);
        assert_eq!(recorded.initial_position.whole_packs, 20);
        assert_eq!(recorded.initial_position.location_id, ids[2]);
        assert_eq!(recorded.initial_put_away.remaining, 20);
        assert_eq!(recorded.initial_put_away.completed, 0);
        assert!(recorded.trace.source_needs_review);
        assert_eq!(
            recorded.trace.source.certainty,
            HubSourceCertaintyDto::Unknown
        );
        assert_eq!(
            core.hub_stock(HubStockQueryDto {
                location_id: Some(ids[3].clone()),
                ..query.clone()
            })
            .await
            .unwrap()
            .total,
            0
        );
        assert_eq!(
            core.receive_hub_stock(original_context.clone(), input.clone())
                .await
                .unwrap(),
            recorded
        );
        let changed = HubReceiptInputDto {
            whole_packs: 21,
            ..input
        };
        assert!(matches!(
            core.receive_hub_stock(original_context.clone(), changed)
                .await
                .unwrap_err(),
            HarvestCircleError::Failure {
                code: WireErrorCode::HubConflict,
                ..
            }
        ));
        assert_eq!(
            core.hub_stock(query.clone()).await.unwrap().rows[0]
                .position
                .whole_packs,
            20
        );
        assert_eq!(
            core.hub_receipt_operation(original_context.request_id.clone())
                .await
                .unwrap(),
            Some(recorded.clone())
        );
        let work_query = crate::HubHandlingQueryDto {
            expected_stock_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
            trace_id: None,
            open_only: false,
            text: String::new(),
        };
        let group = core
            .hub_group(recorded.initial_position.group_id.clone())
            .await
            .unwrap();
        let move_context = context(core.snapshot().revision);
        let move_input = crate::HubHandlingInputDto::Move {
            input: crate::HubMovementInputDto {
                expected_stock_revision: group.stock_revision,
                expected_catalog_revision: group.catalog_revision,
                group_id: group.group.id.clone(),
                expected_group_revision: group.group.revision,
                to_location_id: ids[3].clone(),
                whole_packs: 8,
                separated_portion: Some(crate::HubPhysicalIdentificationDto {
                    label: "Blue tagged stack".into(),
                    observation: "Eight separately tagged crates".into(),
                }),
                occurrence: crate::HubOccurrenceDto {
                    kind: crate::HubOccurrenceKindDto::Unknown,
                    start_unix_s: None,
                    end_unix_s: None,
                },
                observation: "Eight tagged crates arrived at A3".into(),
            },
        };
        let moved = core
            .apply_hub_handling(move_context.clone(), move_input.clone())
            .await
            .unwrap();
        assert_eq!(moved.actor_public_key, key);
        let crate::HubHandlingResultDto::Moved { value: movement } = &moved.result else {
            panic!("expected a physical movement result");
        };
        assert_eq!(
            (movement.whole_packs, movement.remaining_whole_packs),
            (8, 12)
        );
        assert_eq!(
            core.hub_stock(query.clone())
                .await
                .unwrap()
                .rows
                .iter()
                .map(|row| row.position.whole_packs)
                .sum::<u32>(),
            20
        );
        let work = core.hub_work(work_query.clone()).await.unwrap();
        assert_eq!(work.total, 1);
        assert_eq!(work.rows[0].work.quantity.completed, 8);
        assert_eq!(work.rows[0].work.quantity.remaining, 12);
        assert_eq!(
            core.apply_hub_handling(move_context.clone(), move_input.clone())
                .await
                .unwrap(),
            moved
        );
        assert_eq!(
            core.hub_handling_operation(move_context.request_id.clone())
                .await
                .unwrap(),
            Some(moved.clone())
        );
        assert!(
            core.apply_hub_handling(context(core.snapshot().revision), move_input)
                .await
                .is_err()
        );
        let original_open = core
            .apply_hub_handling(
                context(core.snapshot().revision),
                crate::HubHandlingInputDto::OpenDiscrepancy {
                    input: crate::HubDiscrepancyOpenInputDto {
                        expected_stock_revision: moved.stock_revision,
                        expected_catalog_revision: moved.catalog_revision,
                        trace_id: moved.trace_id.clone(),
                        group_id: Some(movement.moved_group_id.clone()),
                        observation: "Inspect damaged outer packaging".into(),
                    },
                },
            )
            .await
            .unwrap();
        let crate::HubHandlingResultDto::DiscrepancyChanged { value: issue } =
            &original_open.result
        else {
            panic!("expected an original discrepancy");
        };
        let closed = core
            .apply_hub_handling(
                context(core.snapshot().revision),
                crate::HubHandlingInputDto::ResolveDiscrepancy {
                    input: crate::HubDiscrepancyResolveInputDto {
                        expected_stock_revision: original_open.stock_revision,
                        expected_catalog_revision: original_open.catalog_revision,
                        discrepancy_id: issue.id.clone(),
                        expected_revision: issue.revision,
                        resolution: "Inspection recorded; original observation retained".into(),
                    },
                },
            )
            .await
            .unwrap();
        assert!(closed.stock_revision > original_open.stock_revision);
        let selected = core
            .hub_group(movement.moved_group_id.clone())
            .await
            .unwrap();
        let split_context = context(core.snapshot().revision);
        let separated = core
            .apply_hub_handling(
                split_context.clone(),
                crate::HubHandlingInputDto::Split {
                    input: crate::HubSplitInputDto {
                        expected_stock_revision: selected.stock_revision,
                        expected_catalog_revision: selected.catalog_revision,
                        group_id: selected.group.id.clone(),
                        expected_group_revision: selected.group.revision,
                        whole_packs: 5,
                        identification: crate::HubPhysicalIdentificationDto {
                            label: "Green five".into(),
                            observation: "Five physically tagged crates".into(),
                        },
                        occurrence: crate::HubOccurrenceDto {
                            kind: crate::HubOccurrenceKindDto::Unknown,
                            start_unix_s: None,
                            end_unix_s: None,
                        },
                        observation: "Separated without moving or completing work".into(),
                    },
                },
            )
            .await
            .unwrap();
        let crate::HubHandlingResultDto::Separated { value: portion } = &separated.result else {
            panic!("original separation")
        };
        assert_eq!((portion.whole_packs, portion.remaining_packs), (5, 3));
        let lineage_query = crate::HubLineageQueryDto {
            trace_id: recorded.trace.id.clone(),
            expected_stock_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
        };
        let lineage = core.hub_lineage(lineage_query.clone()).await.unwrap();
        assert_eq!(lineage.total, 3);
        assert_eq!(
            lineage.rows.iter().map(|row| row.whole_packs).sum::<u32>(),
            20
        );
        assert_eq!(
            core.hub_handling_operation(split_context.request_id)
                .await
                .unwrap(),
            Some(separated)
        );

        let occurrence = || crate::HubOccurrenceDto {
            kind: crate::HubOccurrenceKindDto::Unknown,
            start_unix_s: None,
            end_unix_s: None,
        };
        let treatment_scope = crate::HubTreatmentScopeDto {
            trace_id: recorded.trace.id.clone(),
            expected_stock_revision: None,
            expected_catalog_revision: None,
        };
        let initial = core.hub_treatment(treatment_scope.clone()).await.unwrap();
        assert_eq!(initial.summary.review_required, 20);
        assert_eq!(initial.summary.departure_eligible, 0);
        let accepted = core
            .apply_hub_handling(
                context(core.snapshot().revision),
                crate::HubHandlingInputDto::SetAcceptance {
                    input: crate::HubAcceptanceInputDto {
                        expected_stock_revision: initial.stock_revision,
                        expected_catalog_revision: initial.catalog_revision,
                        group_ids: lineage.rows.iter().map(|row| row.id.clone()).collect(),
                        status: crate::HubAcceptanceStatusDto::Accepted,
                        occurrence: occurrence(),
                        observation: "Explicit manually reviewed sample acceptance".into(),
                    },
                },
            )
            .await
            .unwrap();
        let five_id = lineage
            .rows
            .iter()
            .find(|row| {
                row.identification
                    .as_ref()
                    .is_some_and(|identity| identity.label == "Green five")
            })
            .unwrap()
            .id
            .clone();
        let current = core.hub_treatment(treatment_scope.clone()).await.unwrap();
        assert_eq!(current.summary.departure_eligible, 20);
        let held_a = core
            .apply_hub_handling(
                context(core.snapshot().revision),
                crate::HubHandlingInputDto::PlaceHold {
                    input: crate::HubHoldInputDto {
                        expected_stock_revision: current.stock_revision,
                        expected_catalog_revision: current.catalog_revision,
                        group_ids: vec![five_id.clone()],
                        occurrence: occurrence(),
                        reason: "Sample damage review on the five tagged crates".into(),
                    },
                },
            )
            .await
            .unwrap();
        let crate::HubHandlingResultDto::Treatment { value: a } = &held_a.result else {
            panic!("treatment")
        };
        let crate::HubTreatmentChangeDto::HoldPlaced { value: a } = &a.change else {
            panic!("hold A")
        };
        let hold_a = a.stamp.id.clone();
        let current = core.hub_treatment(treatment_scope.clone()).await.unwrap();
        let held_b = core
            .apply_hub_handling(
                context(core.snapshot().revision),
                crate::HubHandlingInputDto::PlaceHold {
                    input: crate::HubHoldInputDto {
                        expected_stock_revision: current.stock_revision,
                        expected_catalog_revision: current.catalog_revision,
                        group_ids: vec![five_id.clone(), movement.moved_group_id.clone()],
                        occurrence: occurrence(),
                        reason: "Independent source review on all eight tagged crates".into(),
                    },
                },
            )
            .await
            .unwrap();
        let crate::HubHandlingResultDto::Treatment { value: b } = &held_b.result else {
            panic!("treatment")
        };
        let crate::HubTreatmentChangeDto::HoldPlaced { value: b } = &b.change else {
            panic!("hold B")
        };
        let hold_b = b.stamp.id.clone();
        let current = core.hub_treatment(treatment_scope.clone()).await.unwrap();
        assert_eq!(
            (
                current.summary.restricted,
                current.summary.departure_eligible
            ),
            (8, 12)
        );
        let five = current
            .groups
            .iter()
            .find(|group| group.group_id == five_id)
            .unwrap();
        assert!(five.actions.internal_relocation);
        assert!(!five.actions.normal_departure);
        for (index, hold) in [hold_a, hold_b].into_iter().enumerate() {
            let current = core.hub_treatment(treatment_scope.clone()).await.unwrap();
            core.apply_hub_handling(
                context(core.snapshot().revision),
                crate::HubHandlingInputDto::ReleaseHold {
                    input: crate::HubHoldReleaseInputDto {
                        expected_stock_revision: current.stock_revision,
                        expected_catalog_revision: current.catalog_revision,
                        hold_id: hold,
                        expected_hold_revision: 1,
                        occurrence: occurrence(),
                        resolution: "Explicitly reviewed only this selected hold".into(),
                    },
                },
            )
            .await
            .unwrap();
            let current = core.hub_treatment(treatment_scope.clone()).await.unwrap();
            assert_eq!(
                current.summary.departure_eligible,
                if index == 0 { 12 } else { 20 }
            );
        }
        assert_eq!(
            core.hub_handling_operation(held_a.operation_id.clone())
                .await
                .unwrap(),
            Some(held_a)
        );
        assert_eq!(
            core.hub_handling_operation(accepted.operation_id.clone())
                .await
                .unwrap(),
            Some(accepted)
        );
        assert_eq!(
            core.hub_treatment_history(crate::HubTreatmentQueryDto {
                trace_id: recorded.trace.id.clone(),
                expected_stock_revision: None,
                expected_catalog_revision: None,
                offset: 0,
                limit: 64
            })
            .await
            .unwrap()
            .total,
            5
        );

        assert_eq!(
            core.hub_handling_operation(original_open.operation_id.clone())
                .await
                .unwrap(),
            Some(original_open)
        );
        assert_eq!(
            core.hub_work(work_query.clone()).await.unwrap().rows[0]
                .work
                .quantity,
            work.rows[0].work.quantity
        );
        assert_eq!(
            core.hub_discrepancies(crate::HubHandlingQueryDto {
                open_only: true,
                ..work_query.clone()
            })
            .await
            .unwrap()
            .total,
            0
        );
        let saved = core
            .apply_hub_reference(
                context(core.snapshot().revision),
                HubReferenceEditDto {
                    expected_catalog_revision: core
                        .hub_stock(query.clone())
                        .await
                        .unwrap()
                        .catalog_revision,
                    record_id: None,
                    expected_record_revision: None,
                    value: HubReferenceInputDto {
                        kind: HubReferenceKindDto::Party,
                        label: "Fictional recipient A".into(),
                        product_id: None,
                        nominal_grams: None,
                        location_purpose: None,
                    },
                },
            )
            .await
            .unwrap();
        let selected = core.hub_group(five_id).await.unwrap();
        let before = core.hub_stock(query.clone()).await.unwrap().rows;
        let work_before = core.hub_work(work_query.clone()).await.unwrap().rows;
        let document_context = context(core.snapshot().revision);
        let preparation = crate::HubDispatchInputDto::Prepare {
            value: crate::HubDispatchMaterialInputDto {
                expected_stock_revision: selected.stock_revision,
                expected_catalog_revision: selected.catalog_revision,
                recipient_id: saved.current.id,
                lines: vec![crate::HubDispatchLineInputDto {
                    group_id: selected.group.id,
                    expected_group_revision: selected.group.revision,
                    whole_packs: selected.group.whole_packs,
                }],
                observation: "Reviewed five fictional crates for identification".into(),
            },
        };
        let prepared = core
            .apply_hub_dispatch(document_context.clone(), preparation.clone())
            .await
            .unwrap();
        assert_eq!(prepared.record.state, crate::HubDispatchStateDto::Draft);
        assert_eq!(prepared.record.draft.as_ref().unwrap().whole_packs, 5);
        assert_eq!(
            core.apply_hub_dispatch(document_context, preparation)
                .await
                .unwrap(),
            prepared
        );
        let current = core
            .hub_dispatch_record(prepared.record.id.clone())
            .await
            .unwrap();
        let issued = core
            .apply_hub_dispatch(
                context(core.snapshot().revision),
                crate::HubDispatchInputDto::Issue {
                    value: crate::HubDispatchActionInputDto {
                        expected_stock_revision: current.stock_revision,
                        expected_catalog_revision: current.catalog_revision,
                        preparation_id: current.record.id,
                        expected_preparation_revision: current.record.revision,
                        observation: "Explicit sample issuance".into(),
                    },
                },
            )
            .await
            .unwrap();
        assert_eq!(issued.record.state, crate::HubDispatchStateDto::Issued);
        assert_eq!(issued.record.versions[0].material.whole_packs, 5);
        let issued_output_request = crate::HubLocalOutputRequestDto::Identification {
            preparation_id: issued.record.id.clone(),
            identification_id: issued.record.versions[0].id.clone(),
        };
        let issued_output = core
            .hub_local_output(issued_output_request.clone())
            .await
            .unwrap();
        assert_eq!(
            issued.record.versions[0].issued.operation_id,
            issued.operation_id
        );
        assert_eq!(
            core.hub_dispatch_operation(prepared.operation_id.clone())
                .await
                .unwrap(),
            Some(prepared)
        );
        assert_eq!(
            core.hub_dispatch_operation(issued.operation_id.clone())
                .await
                .unwrap(),
            Some(issued.clone())
        );
        assert_eq!(core.hub_stock(query.clone()).await.unwrap().rows, before);
        assert_eq!(
            core.hub_work(work_query.clone()).await.unwrap().rows,
            work_before
        );
        let current = core
            .hub_dispatch_record(issued.record.id.clone())
            .await
            .unwrap();
        let review_input = crate::HubDepartureReviewInputDto {
            preparation_id: issued.record.id.clone(),
            expected_preparation_revision: issued.record.revision,
            identification_id: issued.record.versions[0].id.clone(),
        };
        let before_review = core.hub_stock(query.clone()).await.unwrap();
        let before_review_work = core.hub_work(work_query.clone()).await.unwrap();
        let review = core
            .hub_departure_review(review_input.clone())
            .await
            .unwrap();
        assert!(review.ready);
        assert!(review.block.is_none());
        assert!(review.proposed_work_changes.is_empty());
        assert_eq!(review.view.record, issued.record);
        assert_eq!(before_review, core.hub_stock(query.clone()).await.unwrap());
        assert_eq!(
            before_review_work,
            core.hub_work(work_query.clone()).await.unwrap()
        );
        let departure_context = context(core.snapshot().revision);
        let departure_input = crate::HubDepartureInputDto {
            expected_stock_revision: current.stock_revision,
            expected_catalog_revision: current.catalog_revision,
            preparation_id: issued.record.id.clone(),
            expected_preparation_revision: issued.record.revision,
            identification_id: issued.record.versions[0].id.clone(),
            occurrence: crate::HubOccurrenceDto {
                kind: crate::HubOccurrenceKindDto::Unknown,
                start_unix_s: None,
                end_unix_s: None,
            },
            observation: "Explicitly confirmed five original crates actually left for recipient A"
                .into(),
        };
        let departed = core
            .apply_hub_departure(departure_context.clone(), departure_input.clone())
            .await
            .unwrap();
        assert_eq!(departed.paper, issued.record);
        assert_eq!(departed.lines.len(), 1);
        assert_eq!(departed.lines[0].whole_packs, 5);
        assert_eq!(
            core.apply_hub_departure(departure_context, departure_input.clone())
                .await
                .unwrap(),
            departed
        );
        assert_eq!(
            core.hub_departure_operation(departed.operation_id.clone())
                .await
                .unwrap(),
            Some(departed.clone())
        );
        assert_eq!(
            core.hub_stock(query.clone())
                .await
                .unwrap()
                .rows
                .iter()
                .map(|row| row.position.whole_packs)
                .sum::<u32>(),
            15
        );
        let quantities = core.hub_work(work_query.clone()).await.unwrap().rows[0]
            .work
            .quantity
            .clone();
        assert_eq!(
            (
                quantities.required,
                quantities.completed,
                quantities.no_longer_required,
                quantities.remaining
            ),
            // This fixture separates five from the eight already completed at
            // A3. Exit preserves all completion and cannot retire pending scope.
            (20, 8, 0, 12)
        );
        assert!(departed.work_changes.is_empty());
        let used = core
            .hub_dispatch_record(issued.record.id.clone())
            .await
            .unwrap();
        assert_eq!(used.state, crate::HubDispatchStateDto::Departed);
        assert_eq!(used.record, issued.record);
        assert_eq!(used.departure.unwrap().id, departed.id);
        let review = core
            .hub_departure_review(review_input.clone())
            .await
            .unwrap();
        assert!(!review.ready);
        assert_eq!(review.view.state, crate::HubDispatchStateDto::Departed);
        assert_eq!(review.view.departure.unwrap().id, departed.id);
        let trace_query = crate::HubTraceQueryDto {
            selection: crate::HubTraceSelectionDto {
                scope: crate::HubTraceScopeDto::All,
                receipt_id: None,
                source_id: None,
                text: String::new(),
            },
            expected_stock_revision: None,
            expected_catalog_revision: None,
            offset: 0,
            limit: 64,
        };
        let trace_page = core.hub_trace_page(trace_query.clone()).await.unwrap();
        assert_eq!(trace_page.total, 1);
        assert_eq!(trace_page.rows[0].on_hand_whole_packs, 15);
        assert_eq!(trace_page.rows[0].departed_whole_packs, 5);
        let trace_snapshot_query = crate::HubTraceSnapshotQueryDto {
            trace_id: recorded.trace.id.clone(),
            expected_stock_revision: Some(trace_page.stock_revision),
            expected_catalog_revision: Some(trace_page.catalog_revision),
        };
        let trace_snapshot = core
            .hub_trace_snapshot(trace_snapshot_query.clone())
            .await
            .unwrap();
        assert_eq!(trace_snapshot.trace.original_whole_packs, 20);
        assert_eq!(trace_snapshot.exits.len(), 1);
        assert_eq!(trace_snapshot.exits[0].operation_id, departed.operation_id);
        let history_query = crate::HubTraceHistoryQueryDto {
            trace_id: recorded.trace.id.clone(),
            expected_stock_revision: Some(trace_page.stock_revision),
            expected_catalog_revision: Some(trace_page.catalog_revision),
            offset: 0,
            limit: 64,
        };
        let history = core.hub_trace_history(history_query.clone()).await.unwrap();
        assert!(
            history
                .rows
                .iter()
                .any(|r| r.operation_id == departed.operation_id
                    && r.kind == crate::HubTraceHistoryKindDto::Departure)
        );
        assert_eq!(
            issued_output,
            core.hub_local_output(issued_output_request.clone())
                .await
                .unwrap()
        );
        let output_request = crate::HubLocalOutputRequestDto::Trace {
            trace_id: recorded.trace.id.clone(),
            expected_stock_revision: trace_page.stock_revision,
            expected_catalog_revision: trace_page.catalog_revision,
        };
        let output = core.hub_local_output(output_request.clone()).await.unwrap();
        assert!(output.html.contains("On hand:</strong> 15"));
        assert!(output.html.contains("Actual departed:</strong> 5"));
        assert_eq!(
            output,
            core.hub_local_output(output_request.clone()).await.unwrap()
        );
        assert_eq!(
            history,
            core.hub_trace_history(history_query.clone()).await.unwrap()
        );
        assert_eq!(
            trace_snapshot,
            core.hub_trace_snapshot(trace_snapshot_query.clone())
                .await
                .unwrap()
        );

        assert!(
            core.apply_hub_departure(context(core.snapshot().revision), departure_input.clone())
                .await
                .is_err()
        );
        core.sign_out().await.unwrap();
        assert!(core.hub_trace_page(trace_query).await.is_err());
        assert!(core.hub_trace_snapshot(trace_snapshot_query).await.is_err());
        assert!(core.hub_trace_history(history_query).await.is_err());
        assert!(core.hub_local_output(output_request).await.is_err());
        assert!(core.hub_local_output(issued_output_request).await.is_err());
        assert!(
            core.apply_hub_departure(context(core.snapshot().revision), departure_input)
                .await
                .is_err()
        );
        assert!(
            core.hub_departure_operation(departed.operation_id)
                .await
                .is_err()
        );
        assert!(core.hub_departure_review(review_input).await.is_err());
        assert!(core.hub_group(group.group.id).await.is_err());
        assert!(
            core.hub_handling_operation(move_context.request_id)
                .await
                .is_err()
        );
        assert!(core.hub_stock(query).await.is_err());
        assert!(
            core.hub_receipt_operation(original_context.request_id)
                .await
                .is_err()
        );
        assert!(core.hub_dispatch_record(issued.record.id).await.is_err());
        assert!(
            core.hub_dispatch_operation(issued.operation_id)
                .await
                .is_err()
        );
        core.shutdown_v2().await.unwrap();
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
        assert_eq!(CURRENT_SCHEMA_VERSION, 14);
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
    fn absent_relay_input_opens_offline_without_accepting_invalid_endpoints() {
        let offline =
            super::validated_relay_configuration(&RelayBootstrapInputDto { endpoints: vec![] })
                .unwrap();
        assert!(offline.relays().is_empty());
        assert!(
            super::validated_relay_configuration(&RelayBootstrapInputDto {
                endpoints: vec![RelayEndpointDto {
                    url: "not-a-relay".to_owned(),
                    destination: RelayDestinationDto::Public,
                    read: true,
                    write: true,
                }],
            })
            .is_err()
        );
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
