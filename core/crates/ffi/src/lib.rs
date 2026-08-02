#![doc = "Radroots Studio `UniFFI` boundary."]

mod dto;

pub use dto::{
    AccountDto, ActiveAccountDto, AppLifecycleDto, AppSnapshotDto, KeyAvailabilityDto, ProfileDto,
    ProfileLoadStateDto, RelayConnectionStateDto, SafeErrorDto, SessionStateDto, SignerKindDto,
};

uniffi::setup_scaffolding!();
