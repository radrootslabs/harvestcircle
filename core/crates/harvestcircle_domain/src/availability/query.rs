//! Bounded local availability query values and public snapshot continuations.
//!
//! These values retain structural bindings supplied by a caller. They prove no
//! installed-account admission, event signature, physical inventory, network
//! completeness or current database state. The storage adapter must supply its
//! actual validated store identity, source revision and projection snapshot.

use std::cmp::Ordering;
use std::fmt;

pub use radroots_event::envelope::EventTimestamp;
pub use radroots_event::food::availability::FoodAvailabilityStatus;
use sha2::{Digest, Sha256};

use crate::PublicKey;

use super::{AvailabilityEventVersion, PublicPublisher};

pub const AVAILABILITY_PAGE_DEFAULT_ROWS: u16 = 50;
pub const AVAILABILITY_PAGE_MAX_ROWS: u16 = 100;
pub const AVAILABILITY_QUERY_TEXT_MAX_BYTES: usize = 512;
pub const AVAILABILITY_CURSOR_MAX_BYTES: usize = 512;

const QUERY_FINGERPRINT_DOMAIN: &[u8] = b"harvestcircle.availability.query.v1\0";
const CURSOR_ENCODED_BYTES: usize = 151;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Static query-contract failures without untrusted input or dependent errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityQueryError {
    InvalidInput,
    InputTooLarge,
    ScopeMismatch,
    StaleQuery,
    Capacity,
}

impl fmt::Display for AvailabilityQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "The availability query input is invalid.",
            Self::InputTooLarge => "The availability query input exceeds its byte limit.",
            Self::ScopeMismatch => "The availability query scope does not match.",
            Self::StaleQuery => "The availability query is stale.",
            Self::Capacity => "The availability page exceeds its row limit.",
        })
    }
}

impl std::error::Error for AvailabilityQueryError {}

/// A positive row limit bounded by the local page contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailabilityPageLimit(u16);

impl AvailabilityPageLimit {
    /// Admits exactly one through one hundred rows.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for zero or a limit above the maximum.
    pub const fn new(rows: u16) -> Result<Self, AvailabilityQueryError> {
        if rows == 0 || rows > AVAILABILITY_PAGE_MAX_ROWS {
            return Err(AvailabilityQueryError::InvalidInput);
        }
        Ok(Self(rows))
    }

    #[must_use]
    pub const fn rows(self) -> u16 {
        self.0
    }
}

impl Default for AvailabilityPageLimit {
    fn default() -> Self {
        Self(AVAILABILITY_PAGE_DEFAULT_ROWS)
    }
}

/// Exact opaque cached search text, including a present empty value.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilitySearchText(String);

impl AvailabilitySearchText {
    /// Retains exact UTF-8 bytes after checking their bound before copying.
    ///
    /// This applies no trimming, normalization, location policy or SQL wildcard
    /// interpretation and initiates no remote search.
    ///
    /// # Errors
    ///
    /// Returns `InputTooLarge` when the borrowed input exceeds 512 bytes.
    pub fn new(value: &str) -> Result<Self, AvailabilityQueryError> {
        if value.len() > AVAILABILITY_QUERY_TEXT_MAX_BYTES {
            return Err(AvailabilityQueryError::InputTooLarge);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AvailabilitySearchText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilitySearchText")
            .field("bytes", &self.0.len())
            .finish()
    }
}

/// Immutable optional local filters using the selected shared status enum.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityQueryFilters {
    search: Option<AvailabilitySearchText>,
    publisher: Option<PublicPublisher>,
    status: Option<FoodAvailabilityStatus>,
}

impl AvailabilityQueryFilters {
    #[must_use]
    pub fn new(
        search: Option<AvailabilitySearchText>,
        publisher: Option<PublicPublisher>,
        status: Option<FoodAvailabilityStatus>,
    ) -> Self {
        Self {
            search,
            publisher,
            status,
        }
    }

    #[must_use]
    pub const fn search(&self) -> Option<&AvailabilitySearchText> {
        self.search.as_ref()
    }

    #[must_use]
    pub const fn publisher(&self) -> Option<PublicPublisher> {
        self.publisher
    }

    #[must_use]
    pub const fn status(&self) -> Option<FoodAvailabilityStatus> {
        self.status
    }
}

impl fmt::Debug for AvailabilityQueryFilters {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityQueryFilters")
            .field(
                "search_bytes",
                &self.search().map(|value| value.as_str().len()),
            )
            .field("publisher_present", &self.publisher.is_some())
            .field("status", &self.status)
            .finish()
    }
}

/// Caller-supplied selected-context and local snapshot bindings.
///
/// Nonzero opaque identities are structural values, not proof that a context
/// or store is installed, admitted, current or validated by this module.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AvailabilityQueryContext {
    context_id: [u8; 32],
    store_generation: [u8; 32],
    source_revision: u64,
    projection_generation: u64,
}

impl AvailabilityQueryContext {
    /// Retains exact identities and full-width revisions and generations.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` if either opaque identity is all zero.
    pub fn new(
        context_id: [u8; 32],
        store_generation: [u8; 32],
        source_revision: u64,
        projection_generation: u64,
    ) -> Result<Self, AvailabilityQueryError> {
        if context_id == [0; 32] || store_generation == [0; 32] {
            return Err(AvailabilityQueryError::InvalidInput);
        }
        Ok(Self {
            context_id,
            store_generation,
            source_revision,
            projection_generation,
        })
    }

    #[must_use]
    pub const fn context_id(&self) -> &[u8; 32] {
        &self.context_id
    }

    #[must_use]
    pub const fn store_generation(&self) -> &[u8; 32] {
        &self.store_generation
    }

    #[must_use]
    pub const fn source_revision(&self) -> u64 {
        self.source_revision
    }

    #[must_use]
    pub const fn projection_generation(&self) -> u64 {
        self.projection_generation
    }
}

impl fmt::Debug for AvailabilityQueryContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityQueryContext")
            .field("source_revision", &self.source_revision)
            .field("projection_generation", &self.projection_generation)
            .finish_non_exhaustive()
    }
}

/// A page position ordered by descending signed event time and ascending ID.
///
/// Signed event time retains the shared timestamp's complete `u64` range.
/// Lower `Ord` values come earlier in the page; this selects no event head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailabilityOrderKey {
    created_at: EventTimestamp,
    version: AvailabilityEventVersion,
}

impl AvailabilityOrderKey {
    #[must_use]
    pub const fn new(created_at: EventTimestamp, version: AvailabilityEventVersion) -> Self {
        Self {
            created_at,
            version,
        }
    }

    #[must_use]
    pub const fn created_at(self) -> EventTimestamp {
        self.created_at
    }

    #[must_use]
    pub const fn version(self) -> AvailabilityEventVersion {
        self.version
    }

    #[must_use]
    pub fn is_after(self, previous: Self) -> bool {
        self > previous
    }
}

impl Ord for AvailabilityOrderKey {
    fn cmp(&self, other: &Self) -> Ordering {
        other.created_at.cmp(&self.created_at).then_with(|| {
            self.version
                .event_id()
                .as_bytes()
                .cmp(other.version.event_id().as_bytes())
        })
    }
}

impl PartialOrd for AvailabilityOrderKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A versioned public request digest, with no signature or authorization claim.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AvailabilityQueryFingerprint([u8; 32]);

impl AvailabilityQueryFingerprint {
    /// Streams the fixed v1 frame through standard SHA-256.
    ///
    /// The frame binds full owner/context/store bytes, big-endian counters,
    /// exact option discriminants and search byte length, shared status and
    /// row limit. No secret, host entropy or intermediate request string exists.
    #[must_use]
    pub fn new(
        owner: PublicKey,
        context: &AvailabilityQueryContext,
        session_generation: u64,
        filters: &AvailabilityQueryFilters,
        limit: AvailabilityPageLimit,
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(QUERY_FINGERPRINT_DOMAIN);
        digest.update(owner.as_bytes());
        digest.update(context.context_id());
        digest.update(context.store_generation());
        digest.update(context.source_revision().to_be_bytes());
        digest.update(context.projection_generation().to_be_bytes());
        digest.update(session_generation.to_be_bytes());
        match filters.search() {
            None => digest.update([0]),
            Some(search) => {
                digest.update([1]);
                // The validated text type bounds this length to at most 512.
                digest.update((search.as_str().len() as u64).to_be_bytes());
                digest.update(search.as_str().as_bytes());
            }
        }
        match filters.publisher() {
            None => digest.update([0]),
            Some(publisher) => {
                digest.update([1]);
                digest.update(publisher.public_key().as_bytes());
            }
        }
        digest.update([match filters.status() {
            None => 0,
            Some(FoodAvailabilityStatus::Active) => 1,
            Some(FoodAvailabilityStatus::Sold) => 2,
        }]);
        digest.update(limit.rows().to_be_bytes());
        Self(digest.finalize().into())
    }

    #[must_use]
    pub const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for AvailabilityQueryFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityQueryFingerprint")
            .finish_non_exhaustive()
    }
}

/// Canonical public continuation for one exact structural query snapshot.
///
/// A cursor is forgeable by design. Its fingerprint is correspondence, not a
/// permission token; a future consumer must separately admit the local scope.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityPageCursor {
    value: String,
    after: AvailabilityOrderKey,
}

impl AvailabilityPageCursor {
    #[must_use]
    pub fn encode(fingerprint: AvailabilityQueryFingerprint, after: AvailabilityOrderKey) -> Self {
        let mut value = String::with_capacity(CURSOR_ENCODED_BYTES);
        value.push_str("hcq1:");
        append_hex(&mut value, fingerprint.bytes());
        value.push(':');
        append_hex(&mut value, &after.created_at().as_u64().to_be_bytes());
        value.push(':');
        append_hex(&mut value, after.version().event_id().as_bytes());
        Self { value, after }
    }

    /// Validates the complete borrowed cursor before allocating its owned text.
    ///
    /// # Errors
    ///
    /// Returns `InputTooLarge` first for input above 512 bytes, `InvalidInput`
    /// for noncanonical version/framing/hex, or `StaleQuery` for a different
    /// complete request fingerprint.
    pub fn parse(
        value: &str,
        expected: AvailabilityQueryFingerprint,
    ) -> Result<Self, AvailabilityQueryError> {
        if value.len() > AVAILABILITY_CURSOR_MAX_BYTES {
            return Err(AvailabilityQueryError::InputTooLarge);
        }
        if value.len() != CURSOR_ENCODED_BYTES || !value.is_ascii() {
            return Err(AvailabilityQueryError::InvalidInput);
        }
        let bytes = value.as_bytes();
        if &bytes[..5] != b"hcq1:" || bytes[69] != b':' || bytes[86] != b':' {
            return Err(AvailabilityQueryError::InvalidInput);
        }
        let fingerprint = decode_hex::<32>(&bytes[5..69])?;
        let timestamp = u64::from_be_bytes(decode_hex::<8>(&bytes[70..86])?);
        let event_id = decode_hex::<32>(&bytes[87..151])?;
        if &fingerprint != expected.bytes() {
            return Err(AvailabilityQueryError::StaleQuery);
        }
        let after = AvailabilityOrderKey::new(
            EventTimestamp::new(timestamp),
            AvailabilityEventVersion::from_canonical(radroots_event::EventId::from_bytes(event_id)),
        );
        Ok(Self {
            value: value.to_owned(),
            after,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn after(&self) -> AvailabilityOrderKey {
        self.after
    }
}

impl fmt::Debug for AvailabilityPageCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityPageCursor")
            .field("bytes", &self.value.len())
            .finish()
    }
}

fn append_hex(value: &mut String, bytes: &[u8]) {
    for byte in bytes {
        value.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
}

fn decode_hex<const N: usize>(input: &[u8]) -> Result<[u8; N], AvailabilityQueryError> {
    if input.len() != N * 2 {
        return Err(AvailabilityQueryError::InvalidInput);
    }
    let mut result = [0; N];
    for (output, pair) in result.iter_mut().zip(input.chunks_exact(2)) {
        *output = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Ok(result)
}

const fn hex_digit(byte: u8) -> Result<u8, AvailabilityQueryError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(AvailabilityQueryError::InvalidInput),
    }
}

/// End or continuation of this local query snapshot only.
///
/// `End` proves no exhaustive network/deletion knowledge, fresh inventory or
/// absence of food outside the rows supplied by the local query adapter.
#[derive(Clone, Eq, PartialEq)]
pub enum AvailabilityPageContinuation {
    End,
    More(AvailabilityPageCursor),
}

impl fmt::Debug for AvailabilityPageContinuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::End => "End",
            Self::More(_) => "More",
        })
    }
}

/// A bounded-row owned result without a database or serialization-cap claim.
///
/// The storage adapter remains responsible for bounded scanning, actual
/// snapshot selection, response bytes and deadlines.
pub struct AvailabilityPage<T> {
    items: Vec<T>,
    continuation: AvailabilityPageContinuation,
    projection_generation: u64,
}

impl<T> AvailabilityPage<T> {
    /// Moves the supplied vector after checking its configured row bound.
    ///
    /// No item is cloned and no additional vector allocation is made.
    ///
    /// # Errors
    ///
    /// Returns `Capacity` when the owned row count exceeds the limit.
    pub fn new(
        limit: AvailabilityPageLimit,
        items: Vec<T>,
        continuation: AvailabilityPageContinuation,
        projection_generation: u64,
    ) -> Result<Self, AvailabilityQueryError> {
        if items.len() > usize::from(limit.rows()) {
            return Err(AvailabilityQueryError::Capacity);
        }
        Ok(Self {
            items,
            continuation,
            projection_generation,
        })
    }

    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }

    #[must_use]
    pub const fn continuation(&self) -> &AvailabilityPageContinuation {
        &self.continuation
    }

    #[must_use]
    pub const fn projection_generation(&self) -> u64 {
        self.projection_generation
    }

    #[must_use]
    pub fn into_items(self) -> Vec<T> {
        self.items
    }
}

impl<T> fmt::Debug for AvailabilityPage<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityPage")
            .field("item_count", &self.items.len())
            .field("projection_generation", &self.projection_generation)
            .field("continuation", &self.continuation)
            .finish()
    }
}
