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
