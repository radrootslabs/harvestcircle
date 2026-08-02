#![doc = "Radroots Studio `UniFFI` boundary."]

mod commands;
mod dto;
mod observer;

pub use commands::{GeneratedAccountDto, RemovalRequest, StudioAppCore, StudioError};
pub use dto::{
    AccountDto, ActiveAccountDto, AppLifecycleDto, AppSnapshotDto, KeyAvailabilityDto, ProfileDto,
    ProfileLoadStateDto, RelayConnectionStateDto, SafeErrorDto, SessionStateDto, SignerKindDto,
};
pub use observer::{ObserverSubscription, StudioObserver};

uniffi::setup_scaffolding!();

#[uniffi::export]
#[must_use]
pub fn native_runtime_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_runtime_reports_the_crate_version() {
        assert_eq!(super::native_runtime_version(), "0.1.0-alpha");
    }
}
