#![doc = "HarvestCircle `UniFFI` boundary."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod commands;
mod contract;
mod dto;
mod observer;

pub use commands::{
    BuildInfoDto, GeneratedRecoveryRequest, HarvestCircleAppCore, HarvestCircleError,
    IdentityCommandReceiptDto, RelayBootstrapInputDto, RemovalRequest, RequestContextDto,
    RuntimeOpenInputDto, build_info,
};
pub use contract::{
    DISTRIBUTION_PACKAGE_VERSION, FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR,
    FFI_CONTRACT_MINOR, MINIMUM_SCHEMA_VERSION, PRODUCT_COORDINATE_DIGEST, PRODUCT_VERSION,
    SNAPSHOT_SCHEMA_VERSION, SOURCE_FOUNDATION_BASELINE, SOURCE_PROVENANCE_DIGEST,
};
pub use dto::{
    ActiveIdentityDto, AppLifecycleDto, AppSnapshotDto, IdentityDto, ProfileDto,
    ProfileLoadStateDto, RelayConnectionStateDto, RelayDestinationDto, RelayEndpointDto,
    SafeErrorDto, SessionStateDto, SignerAvailabilityDto, SignerBindingKindDto, WireErrorCategory,
    WireErrorCode, WireRecoveryAction,
};
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #[test]
    fn native_runtime_reports_the_product_version_independently() {
        assert_eq!(super::native_runtime_version(), "0.1.0-alpha");
        assert_eq!(super::PRODUCT_VERSION, "0.1.0-alpha");
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.1.0-alpha");
        assert_eq!(super::FFI_CONTRACT_ID, "harvestcircle-desktop-ffi-v4");
        assert_eq!(super::FFI_CONTRACT_MAJOR, 4);
        assert_eq!(super::FFI_CONTRACT_MINOR, 2);
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
