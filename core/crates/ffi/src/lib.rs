#![doc = "Radroots Studio `UniFFI` boundary."]

mod commands;
mod dto;

pub use commands::{GeneratedAccountDto, RemovalRequest, StudioAppCore, StudioError};
pub use dto::{
    AccountDto, ActiveAccountDto, AppLifecycleDto, AppSnapshotDto, KeyAvailabilityDto, ProfileDto,
    ProfileLoadStateDto, RelayConnectionStateDto, SafeErrorDto, SessionStateDto, SignerKindDto,
};

uniffi::setup_scaffolding!();
