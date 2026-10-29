#![doc = "HarvestCircle `UniFFI` boundary."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod bootstrap;
mod commands;
mod contract;
mod dto;
mod host_runtime;
mod hub_departure;
mod hub_dispatch;
mod hub_dispatch_input;
mod hub_handling;
mod hub_portions;
mod hub_reference;
mod hub_stock;
mod hub_trace;
mod hub_treatment;
mod hub_treatment_input;
mod keyring_worker;
mod observer;

pub use commands::{
    BuildInfoDto, GeneratedRecoveryRequest, HarvestCircleAppCore, HarvestCircleError,
    HubContextDto, IdentityCommandReceiptDto, RelayBootstrapInputDto, RemovalRequest,
    RequestContextDto, RuntimeOpenInputDto, build_info,
};
pub use contract::{
    DISTRIBUTION_PACKAGE_VERSION, FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR,
    FFI_CONTRACT_MINOR, MINIMUM_SCHEMA_VERSION, PRODUCT_COORDINATE_DIGEST, PRODUCT_VERSION,
    SNAPSHOT_SCHEMA_VERSION,
};
pub use dto::{
    ActiveIdentityDto, AppLifecycleDto, AppSnapshotDto, IdentityDto, NostrReferenceKindDto,
    NostrReferenceParseDto, ProfileDto, ProfileLoadStateDto, RelayConnectionStateDto,
    RelayDestinationDto, RelayEndpointDto, SafeErrorDto, SessionStateDto, SignerAvailabilityDto,
    SignerBindingKindDto, WireErrorCategory, WireErrorCode, WireRecoveryAction,
};
pub use hub_departure::{
    HubDepartureInputDto, HubDepartureLineDto, HubDepartureReceiptDto, HubDepartureReviewDto,
    HubDepartureReviewInputDto, HubDepartureUseDto, HubDepartureWorkChangeDto,
};
pub use hub_dispatch::{
    HubDispatchFilterDto, HubDispatchKindDto, HubDispatchLineDto, HubDispatchMaterialDto,
    HubDispatchPageDto, HubDispatchQueryDto, HubDispatchReceiptDto, HubDispatchRecordDto,
    HubDispatchRowDto, HubDispatchStampDto, HubDispatchStateDto, HubDispatchViewDto,
    HubIdentificationStatusDto, HubIdentificationVersionDto,
};
pub use hub_dispatch_input::{
    HubDispatchActionInputDto, HubDispatchEditInputDto, HubDispatchInputDto,
    HubDispatchLineInputDto, HubDispatchMaterialInputDto,
};
pub use hub_handling::{
    HubActionGroupDto, HubDiscrepancyDto, HubDiscrepancyOpenInputDto, HubDiscrepancyPageDto,
    HubDiscrepancyResolveInputDto, HubDiscrepancyRowDto, HubGroupViewDto, HubGroupWorkDto,
    HubHandlingInputDto, HubHandlingKindDto, HubHandlingQueryDto, HubHandlingReceiptDto,
    HubHandlingResultDto, HubMovementInputDto, HubMovementResultDto, HubNewWorkInputDto,
    HubPhysicalGroupDto, HubPhysicalIdentificationDto, HubReplanInputDto, HubSplitInputDto,
    HubWorkActionResultDto, HubWorkChangeDto, HubWorkOriginDto, HubWorkPageDto,
    HubWorkRequirementDto, HubWorkRowDto,
};
pub use hub_portions::{
    HubGroupCreationKindDto, HubLineagePageDto, HubLineageQueryDto, HubLineageRowDto,
    HubSplitResultDto,
};
pub use hub_reference::{
    HubHandlingBasisDto, HubLocationPurposeDto, HubReferenceEditDto, HubReferenceInputDto,
    HubReferenceKindDto, HubReferencePageDto, HubReferenceQueryDto, HubReferenceReceiptDto,
    HubReferenceRecordDto,
};
pub use hub_stock::{
    HubOccurrenceDto, HubOccurrenceKindDto, HubPopulationOriginDto, HubQuantityWorkDto,
    HubReceiptInputDto, HubSourceCertaintyDto, HubSourceDto, HubSourceInputDto, HubSourceKindDto,
    HubStockPageDto, HubStockPopulationDto, HubStockPositionDto, HubStockQueryDto,
    HubStockReceiptDto, HubStockRowDto,
};
pub use hub_trace::{
    HubLocalOutputDto, HubLocalOutputRequestDto, HubTraceExitDto, HubTraceHistoryEntryDto,
    HubTraceHistoryKindDto, HubTraceHistoryPageDto, HubTraceHistoryQueryDto, HubTracePageDto,
    HubTracePositionDto, HubTraceQueryDto, HubTraceRowDto, HubTraceScopeDto, HubTraceSelectionDto,
    HubTraceSnapshotDto, HubTraceSnapshotQueryDto,
};
pub use hub_treatment::{
    HubAcceptanceDecisionDto, HubAcceptanceStatusDto, HubActionEligibilityDto,
    HubCapturedPhysicalScopeDto, HubGroupTreatmentDto, HubHoldDto, HubHoldRowDto,
    HubPhysicalScopeAnchorDto, HubTreatmentChangeDto, HubTreatmentPageDto, HubTreatmentQueryDto,
    HubTreatmentResultDto, HubTreatmentScopeDto, HubTreatmentSnapshotDto, HubTreatmentStampDto,
    HubTreatmentSummaryDto,
};
pub use hub_treatment_input::{HubAcceptanceInputDto, HubHoldInputDto, HubHoldReleaseInputDto};
pub use observer::{
    HarvestCircleChangeObserver, ObserverSubscription, ShutdownReceiptDto, SnapshotChangeDto,
};

uniffi::setup_scaffolding!();

#[cfg_attr(not(coverage_nightly), uniffi::export)]
#[must_use]
pub fn native_runtime_version() -> String {
    PRODUCT_VERSION.to_owned()
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
#[must_use]
pub fn generate_operation_id_v7() -> String {
    harvestcircle_application::DurableRequestId::new_v7()
        .as_str()
        .to_owned()
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
#[must_use]
pub fn classify_nostr_reference(raw: String) -> NostrReferenceParseDto {
    harvestcircle_nostr::classify_reference(&raw).into()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #[test]
    fn native_runtime_reports_the_product_version_independently() {
        assert_eq!(super::native_runtime_version(), "0.1.0-alpha");
        assert_eq!(super::PRODUCT_VERSION, "0.1.0-alpha");
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0-alpha");
        assert_eq!(super::FFI_CONTRACT_ID, "harvestcircle-desktop-ffi-v5");
        assert_eq!(super::FFI_CONTRACT_MAJOR, 5);
        assert_eq!(super::FFI_CONTRACT_MINOR, 13);
        assert_eq!(super::SNAPSHOT_SCHEMA_VERSION, 1);
        assert_eq!(
            super::PRODUCT_COORDINATE_DIGEST,
            harvestcircle_product::PRODUCT_COORDINATE_DIGEST
        );
        assert_eq!(super::DISTRIBUTION_PACKAGE_VERSION, "1.0.0");
        assert_eq!(super::FFI_CONTRACT_HASH.len(), 64);
        assert!(!super::contract::NORMALIZED_CONTRACT_METADATA.is_empty());
        let operation_id = super::generate_operation_id_v7();
        assert!(harvestcircle_application::DurableRequestId::parse(operation_id).is_ok());
        let build = super::build_info();
        assert_eq!(build.ffi_contract_id, super::FFI_CONTRACT_ID);
        assert_eq!(build.ffi_contract_hash, super::FFI_CONTRACT_HASH);
        assert_eq!(build.provenance_digest.len(), 64);
        assert_eq!(
            build.current_storage_schema_version,
            harvestcircle_storage::CURRENT_SCHEMA_VERSION
        );
        for value in [
            build.source_commit,
            build.radroots_revision,
            build.rust_toolchain,
            build.java_toolchain,
            build.kotlin_toolchain,
        ] {
            assert!(!value.is_empty());
            assert!(!value.contains(['\n', '\r']));
        }
    }
}
