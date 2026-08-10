#![doc = "HarvestCircle `UniFFI` boundary."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod commands;
mod contract;
mod dto;
mod observer;

pub use commands::{
    GeneratedRecoveryRequest, HarvestCircleAppCore, HarvestCircleError, IdentityCommandReceiptDto,
    RemovalRequest, RequestContextDto,
};
pub use contract::{
    DISTRIBUTION_PACKAGE_VERSION, FFI_CONTRACT_HASH, FFI_CONTRACT_ID, FFI_CONTRACT_MAJOR,
    FFI_CONTRACT_MINOR, MINIMUM_SCHEMA_VERSION, PRODUCT_COORDINATE_DIGEST, PRODUCT_VERSION,
    SNAPSHOT_SCHEMA_VERSION, SOURCE_FOUNDATION_BASELINE, SOURCE_PROVENANCE_DIGEST,
};
pub use dto::{
    ActiveIdentityDto, AppLifecycleDto, AppSnapshotDto, IdentityDto, ProfileDto,
    ProfileLoadStateDto, RelayConnectionStateDto, SafeErrorDto, SessionStateDto,
    SignerAvailabilityDto, SignerBindingKindDto, WireErrorCategory, WireErrorCode,
    WireRecoveryAction,
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
        assert_eq!(super::FFI_CONTRACT_MINOR, 0);
        assert_eq!(super::SNAPSHOT_SCHEMA_VERSION, 1);
        assert_eq!(
            super::PRODUCT_COORDINATE_DIGEST,
            harvestcircle_product::PRODUCT_COORDINATE_DIGEST
        );
        assert_eq!(super::DISTRIBUTION_PACKAGE_VERSION, "1.0.0");
        assert_eq!(super::FFI_CONTRACT_HASH.len(), 64);
        assert!(!super::contract::NORMALIZED_CONTRACT_METADATA.is_empty());
    }
}
