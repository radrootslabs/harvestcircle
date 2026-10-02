#![doc = "HarvestCircle Nostr identity domain types."]

pub mod availability;
pub mod error;
pub mod identity;
pub mod key;
pub mod profile;
pub mod time;

pub use availability::{
    AVAILABILITY_CURSOR_MAX_BYTES, AVAILABILITY_PAGE_DEFAULT_ROWS, AVAILABILITY_PAGE_MAX_ROWS,
    AVAILABILITY_QUERY_TEXT_MAX_BYTES, AvailabilityEventVersion, AvailabilityHeadState,
    AvailabilityHeadView, AvailabilityListingCoordinate, AvailabilityObservation,
    AvailabilityOrderKey, AvailabilityPage, AvailabilityPageContinuation, AvailabilityPageCursor,
    AvailabilityPageLimit, AvailabilityQueryContext, AvailabilityQueryError,
    AvailabilityQueryFilters, AvailabilityQueryFingerprint, AvailabilitySearchText,
    AvailabilityUnsupportedReason, AvailabilityVersionView, PublicPublisher,
};
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
pub use time::UnixTimestamp;
