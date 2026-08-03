#![doc = "Radroots Studio Nostr account domain types."]

pub mod account;
pub mod error;
pub mod key;
pub mod profile;
pub mod relay;
pub mod time;

pub use account::{
    AccountCreatedAt, AccountIdentity, AccountLabel, AccountSummary, BindingAvailability,
    BindingRepairAction, LocalSignerBinding,
};
pub use error::{SafeError, SafeErrorCode, SafeMessage};
pub use key::{
    MAX_SECRET_KEY_INPUT_BYTES, Npub, Nsec, PublicKey, SecretKeyInput, SecretKeyInputKind,
};
pub use profile::{EventId, Kind0ProfileCandidate, ProfileMetadata, select_latest_kind0};
pub use relay::{RelayUrl, normalize_relay_urls};
pub use time::UnixTimestamp;
