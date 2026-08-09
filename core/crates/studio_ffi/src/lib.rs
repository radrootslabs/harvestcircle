#![doc = "Radroots Studio `UniFFI` boundary."]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod commands;
mod contract;
mod dto;
mod observer;

pub use commands::{
    AccountCommandReceiptDto, GeneratedRecoveryRequest, RemovalRequest, RequestContextDto,
    StudioAppCore, StudioError,
};
pub use contract::{
    FFI_CONTRACT_HASH, FFI_CONTRACT_MAJOR, FFI_CONTRACT_MINOR, MINIMUM_SCHEMA_VERSION,
    PRODUCT_VERSION,
};
pub use dto::{
    AccountDto, ActiveAccountDto, AppLifecycleDto, AppSnapshotDto, KeyAvailabilityDto, ProfileDto,
    ProfileLoadStateDto, RelayConnectionStateDto, SafeErrorDto, SessionStateDto, SignerKindDto,
    WireErrorCategory, WireErrorCode, WireRecoveryAction,
};
pub use observer::{
    ObserverSubscription, ShutdownReceiptDto, SnapshotChangeDto, StudioChangeObserver,
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
        assert_eq!(super::FFI_CONTRACT_MAJOR, 3);
        assert_eq!(super::FFI_CONTRACT_HASH.len(), 64);
        assert!(!super::contract::NORMALIZED_CONTRACT_METADATA.is_empty());
    }
}
