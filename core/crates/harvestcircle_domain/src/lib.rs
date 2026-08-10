#![doc = "HarvestCircle Nostr identity domain types."]

pub mod error;
pub mod identity;
pub mod key;
pub mod profile;
pub mod relay;
pub mod time;

pub use error::{SafeError, SafeErrorCode, SafeMessage};
pub use identity::{
    IdentityCreatedAt, IdentityLabel, LocalKeyringBinding, NostrIdentity, NostrIdentityReference,
    SignerAvailability, SignerBinding, SignerRepairAction,
};
pub use key::{
    MAX_SECRET_KEY_INPUT_BYTES, Npub, Nsec, PersistedPublicKeyClassification, PublicKey,
    SecretKeyInput, SecretKeyInputKind, classify_persisted_public_key,
};
pub use profile::{EventId, Kind0ProfileCandidate, ProfileMetadata, select_latest_kind0};
pub use relay::{RelayDestinationPolicy, RelayUrl, normalize_relay_urls};
pub use time::UnixTimestamp;
