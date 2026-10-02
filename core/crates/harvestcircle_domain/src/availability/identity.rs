//! Bounded public references for availability publishers, versions and listings.
//!
//! These references identify public data. Their construction proves no event
//! signature, installed account, signing custody, ownership or physical stock.

use radroots_event::envelope::kind::KIND_CLASSIFIED_LISTING;
use radroots_event::id::{Nip01Coordinate, RADROOTS_NIP01_COORDINATE_MAX_BYTES};

use crate::{PublicKey, SafeError, SafeErrorCode, SafeMessage};

const EVENT_ID_HEX_BYTES: usize = 64;
const LISTING_COORDINATE_PREFIX: &str = "30402:";

/// A validated public publisher identity, independent of installed accounts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicPublisher(PublicKey);

impl PublicPublisher {
    #[must_use]
    pub const fn from_public_key(value: PublicKey) -> Self {
        Self(value)
    }

    /// Parses a canonical lowercase hexadecimal public author.
    ///
    /// # Errors
    ///
    /// Returns a safe invalid-public-key error for malformed or invalid keys.
    pub fn from_hex(value: &str) -> Result<Self, SafeError> {
        PublicKey::from_hex(value).map(Self)
    }

    /// Validates an x-only secp256k1 public author using shared identity policy.
    ///
    /// # Errors
    ///
    /// Returns a safe invalid-public-key error for an invalid curve point.
    pub fn from_bytes(value: [u8; 32]) -> Result<Self, SafeError> {
        PublicKey::from_bytes(value).map(Self)
    }

    #[must_use]
    pub const fn public_key(self) -> PublicKey {
        self.0
    }
}

/// An exact event-ID reference, separate from a logical listing coordinate.
///
/// Possessing an ID does not establish the referenced event's signature or its
/// relationship to any listing. Those facts require shared verified evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AvailabilityEventVersion(radroots_event::EventId);

impl AvailabilityEventVersion {
    #[must_use]
    pub const fn from_canonical(value: radroots_event::EventId) -> Self {
        Self(value)
    }

    /// Parses exactly 64 canonical lowercase hexadecimal event-ID bytes.
    ///
    /// # Errors
    ///
    /// Returns a safe public-reference error for malformed or ambiguous input.
    pub fn from_hex(value: &str) -> Result<Self, SafeError> {
        if value.len() != EVENT_ID_HEX_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid_reference());
        }
        radroots_event::EventId::parse(value)
            .map(Self)
            .map_err(|_| invalid_reference())
    }

    #[must_use]
    pub const fn event_id(self) -> radroots_event::EventId {
        self.0
    }
}

/// A nonempty kind-30402 logical listing reference with shared coordinate bounds.
///
/// The identifier remains opaque, including colons, whitespace, controls and
/// Unicode. This reference does not apply strict authored FoodIdentifier policy
/// and must not replace broader raw inbound or event-head admission. Empty or
/// longer raw head identifiers remain evidence at their shared owner boundary.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AvailabilityListingCoordinate(Nip01Coordinate);

impl AvailabilityListingCoordinate {
    /// Parses a canonical listing reference without normalizing its identifier.
    ///
    /// App bounds, framing and author validation precede shared owned copies.
    /// Shared parsing owns coordinate interpretation and canonical construction.
    ///
    /// # Errors
    ///
    /// Returns a safe public-reference error for invalid, noncanonical, empty or
    /// oversized listing references.
    pub fn parse(value: &str) -> Result<Self, SafeError> {
        if value.len() > RADROOTS_NIP01_COORDINATE_MAX_BYTES {
            return Err(invalid_reference());
        }
        let remainder = value
            .strip_prefix(LISTING_COORDINATE_PREFIX)
            .ok_or_else(invalid_reference)?;
        let (author, identifier) = remainder.split_once(':').ok_or_else(invalid_reference)?;
        if identifier.is_empty() {
            return Err(invalid_reference());
        }
        PublicKey::from_hex(author).map_err(|_| invalid_reference())?;
        let coordinate = Nip01Coordinate::parse(value).map_err(|_| invalid_reference())?;
        Self::from_canonical(coordinate)
    }

    /// Moves a shared coordinate after checking the app's listing invariants.
    ///
    /// # Errors
    ///
    /// Returns a safe public-reference error for another kind, an empty
    /// identifier or a coordinate exceeding the selected shared byte bound.
    pub fn from_canonical(value: Nip01Coordinate) -> Result<Self, SafeError> {
        if value.kind() != KIND_CLASSIFIED_LISTING
            || value.identifier().is_empty()
            || value.as_str().len() > RADROOTS_NIP01_COORDINATE_MAX_BYTES
        {
            return Err(invalid_reference());
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn canonical(&self) -> &Nip01Coordinate {
        &self.0
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    #[must_use]
    pub fn publisher(&self) -> PublicPublisher {
        PublicPublisher::from_public_key(PublicKey::from_canonical(*self.0.pubkey()))
    }

    #[must_use]
    pub const fn kind(&self) -> u32 {
        self.0.kind()
    }

    #[must_use]
    pub fn identifier(&self) -> &str {
        self.0.identifier()
    }
}

const fn invalid_reference() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidProfileMetadata,
        SafeMessage::new("The public availability reference is invalid."),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::error::Error;

    use radroots_event::food::availability::FoodIdentifier;
    use radroots_event::id::{Nip01Coordinate, RADROOTS_NIP01_COORDINATE_MAX_BYTES};

    use super::{AvailabilityEventVersion, AvailabilityListingCoordinate, PublicPublisher};
    use crate::{PublicKey, SafeError, SafeErrorCode};

    const AUTHOR_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

    fn author() -> PublicKey {
        PublicKey::from_hex(AUTHOR_HEX).expect("valid public author")
    }

    fn coordinate_text(identifier: &str) -> String {
        format!("30402:{AUTHOR_HEX}:{identifier}")
    }

    fn coordinate(identifier: &str) -> AvailabilityListingCoordinate {
        AvailabilityListingCoordinate::parse(&coordinate_text(identifier))
            .expect("valid listing reference")
    }

    fn identifier_at_byte_limit(unit: &str, maximum: usize) -> String {
        unit.repeat(maximum / unit.len()) + &"x".repeat(maximum % unit.len())
    }

    fn assert_invalid_reference(error: SafeError) {
        assert_eq!(error.code(), SafeErrorCode::InvalidProfileMetadata);
    }

    #[test]
    fn public_publisher_uses_shared_identity_without_installed_account() {
        let shared =
            radroots_identity::PublicKey::from_hex(AUTHOR_HEX).expect("valid shared public author");
        let publisher = PublicPublisher::from_public_key(PublicKey::from_canonical(shared));

        assert_eq!(publisher.public_key().canonical(), shared);
        assert_eq!(
            PublicPublisher::from_hex(AUTHOR_HEX).expect("public author"),
            publisher
        );
        assert_eq!(
            PublicPublisher::from_bytes(*shared.as_bytes()).expect("public author bytes"),
            publisher
        );
        assert_eq!(HashSet::from([publisher, publisher]).len(), 1);
    }

    #[test]
    fn public_publisher_rejects_invalid_curve_and_noncanonical_author() {
        let invalid_curve = "ff".repeat(32);
        let uppercase = AUTHOR_HEX.to_ascii_uppercase();
        let short = &AUTHOR_HEX[..63];
        let padded = format!(" {AUTHOR_HEX}");
        let invalid_character = "g".repeat(64);

        for invalid in [
            "",
            invalid_curve.as_str(),
            uppercase.as_str(),
            short,
            padded.as_str(),
            invalid_character.as_str(),
            "npub1invalid",
        ] {
            assert_eq!(
                PublicPublisher::from_hex(invalid)
                    .expect_err("invalid public author")
                    .code(),
                SafeErrorCode::InvalidPublicKey
            );
        }
        assert_eq!(
            PublicPublisher::from_bytes([u8::MAX; 32])
                .expect_err("invalid x-only curve point")
                .code(),
            SafeErrorCode::InvalidPublicKey
        );
    }

    #[test]
    fn event_version_retains_exact_shared_event_id() {
        let shared = radroots_event::EventId::from_bytes([0xab; 32]);
        let version = AvailabilityEventVersion::from_canonical(shared);

        assert_eq!(version.event_id(), shared);
        assert_eq!(version.event_id().as_bytes(), &[0xab; 32]);
        assert_eq!(
            AvailabilityEventVersion::from_hex(&"ab".repeat(32)).expect("exact event ID"),
            version
        );
        assert_eq!(HashSet::from([version, version]).len(), 1);
        assert_ne!(
            version,
            AvailabilityEventVersion::from_canonical(radroots_event::EventId::from_bytes(
                [0xcd; 32]
            ))
        );
    }

    #[test]
    fn event_version_rejects_malformed_and_ambiguous_hex() {
        let uppercase = "AB".repeat(32);
        let short = "a".repeat(63);
        let long = "a".repeat(65);
        let invalid_character = "g".repeat(64);
        let leading_space = format!(" {}", "ab".repeat(32));
        let prefixed = format!("0x{}", "ab".repeat(32));
        let multibyte = "é".repeat(32);

        for invalid in [
            "",
            uppercase.as_str(),
            short.as_str(),
            long.as_str(),
            invalid_character.as_str(),
            leading_space.as_str(),
            prefixed.as_str(),
            multibyte.as_str(),
        ] {
            assert_invalid_reference(
                AvailabilityEventVersion::from_hex(invalid).expect_err("invalid exact event ID"),
            );
        }
    }

    #[test]
    fn listing_coordinate_delegates_to_shared_canonical_coordinate() {
        let text = coordinate_text("farm:lot-1");
        let shared = Nip01Coordinate::parse(&text).expect("shared coordinate");
        let reference = AvailabilityListingCoordinate::parse(&text).expect("listing coordinate");

        assert_eq!(reference.canonical(), &shared);
        assert_eq!(reference.as_str(), shared.as_str());
        assert_eq!(reference.kind(), 30_402);
        assert_eq!(reference.publisher().public_key(), author());
        assert_eq!(reference.identifier(), "farm:lot-1");
        assert_eq!(
            AvailabilityListingCoordinate::from_canonical(shared).expect("typed coordinate"),
            reference
        );
    }

    #[test]
    fn listing_coordinate_rejects_wrong_kind_and_empty_identifier() {
        for kind in [0, 3, 5, 10_000, 30_000, 30_401, 30_403, 39_999, 40_000] {
            assert_invalid_reference(
                AvailabilityListingCoordinate::parse(&format!("{kind}:{AUTHOR_HEX}:lot"))
                    .expect_err("wrong listing kind"),
            );
        }
        for invalid in [
            String::new(),
            coordinate_text(""),
            format!("30402:{AUTHOR_HEX}"),
            "30402:bad:lot".to_owned(),
            format!("30402:{}:lot", "ff".repeat(32)),
        ] {
            assert_invalid_reference(
                AvailabilityListingCoordinate::parse(&invalid)
                    .expect_err("invalid listing coordinate"),
            );
        }
        assert!(
            Nip01Coordinate::parse(coordinate_text("")).is_ok(),
            "raw shared coordinate admission remains broader than this listing reference"
        );
    }

    #[test]
    fn listing_coordinate_rejects_noncanonical_kind_and_author_aliases() {
        for kind in ["+30402", "030402", "+030402", " 30402", "30402 "] {
            assert_invalid_reference(
                AvailabilityListingCoordinate::parse(&format!("{kind}:{AUTHOR_HEX}:lot"))
                    .expect_err("noncanonical kind encoding"),
            );
        }
        for invalid in [
            format!("30402:{}:lot", AUTHOR_HEX.to_ascii_uppercase()),
            format!("30402: {AUTHOR_HEX}:lot"),
            format!("30402:{AUTHOR_HEX} :lot"),
            format!("\u{2003}30402:{AUTHOR_HEX}:lot"),
        ] {
            assert_invalid_reference(
                AvailabilityListingCoordinate::parse(&invalid)
                    .expect_err("noncanonical author or coordinate framing"),
            );
        }
    }

    #[test]
    fn listing_coordinate_preserves_opaque_identifier_after_second_colon() {
        for identifier in [
            ":",
            "::",
            "farm:lot:2026",
            "  victoria:\0seed:\u{2603}\n",
            "a\u{200d}b",
            "e\u{301}",
            "a\r\tb",
            " trailing ",
        ] {
            let reference = coordinate(identifier);
            let shared = Nip01Coordinate::parse(coordinate_text(identifier))
                .expect("shared opaque coordinate");

            assert_eq!(reference.identifier().as_bytes(), identifier.as_bytes());
            assert_eq!(
                reference.as_str().as_bytes(),
                coordinate_text(identifier).as_bytes()
            );
            assert_eq!(reference.canonical(), &shared);
        }
    }

    #[test]
    fn listing_reference_identifier_policy_is_distinct_from_authored_food_identifier() {
        let oversized_authored = "x".repeat(513);
        for identifier in [oversized_authored.as_str(), " farm ", "a\0b", "a\u{200d}b"] {
            assert_eq!(coordinate(identifier).identifier(), identifier);
            assert!(
                FoodIdentifier::parse(identifier).is_err(),
                "opaque public references cannot inherit strict authored identifier policy"
            );
        }
        let strict = FoodIdentifier::parse("farm:lot-1").expect("strict colon identifier");
        assert_eq!(coordinate(strict.as_str()).identifier(), strict.as_str());
    }

    #[test]
    fn listing_coordinate_accepts_exact_ascii_byte_limit_and_rejects_overflow() {
        assert_eq!(RADROOTS_NIP01_COORDINATE_MAX_BYTES, 4_096);
        let prefix = coordinate_text("");
        assert_eq!(prefix.len(), 71);
        let identifier = "x".repeat(RADROOTS_NIP01_COORDINATE_MAX_BYTES - prefix.len());
        let exact = coordinate_text(&identifier);

        assert_eq!(identifier.len(), 4_025);
        assert_eq!(exact.len(), RADROOTS_NIP01_COORDINATE_MAX_BYTES);
        assert_eq!(
            AvailabilityListingCoordinate::parse(&exact)
                .expect("exact full coordinate limit")
                .identifier(),
            identifier
        );
        assert_invalid_reference(
            AvailabilityListingCoordinate::parse(&(exact + "x"))
                .expect_err("one byte above full coordinate limit"),
        );
    }

    #[test]
    fn listing_coordinate_enforces_multibyte_and_combining_byte_limits() {
        let maximum = RADROOTS_NIP01_COORDINATE_MAX_BYTES - coordinate_text("").len();
        for unit in ["é", "🥕", "e\u{301}"] {
            let identifier = identifier_at_byte_limit(unit, maximum);
            let exact = coordinate_text(&identifier);

            assert_eq!(identifier.len(), maximum);
            assert_eq!(exact.len(), RADROOTS_NIP01_COORDINATE_MAX_BYTES);
            assert_eq!(
                AvailabilityListingCoordinate::parse(&exact)
                    .expect("exact UTF-8 coordinate limit")
                    .identifier()
                    .as_bytes(),
                identifier.as_bytes()
            );
            assert_invalid_reference(
                AvailabilityListingCoordinate::parse(&(exact + "x"))
                    .expect_err("one UTF-8 byte above coordinate limit"),
            );
        }
        assert_ne!(coordinate("é"), coordinate("e\u{301}"));
    }

    #[test]
    fn typed_coordinate_constructor_cannot_bypass_listing_invariants() {
        for text in [
            format!("30000:{AUTHOR_HEX}:lot"),
            format!("0:{AUTHOR_HEX}:"),
            coordinate_text(""),
        ] {
            let shared = Nip01Coordinate::parse(text).expect("valid broader shared coordinate");
            assert_invalid_reference(
                AvailabilityListingCoordinate::from_canonical(shared)
                    .expect_err("shared coordinate is not a nonempty listing reference"),
            );
        }
        let canonicalized = Nip01Coordinate::parse(format!(
            "030402:{}:farm:lot",
            AUTHOR_HEX.to_ascii_uppercase()
        ))
        .expect("shared canonicalization");
        let reference = AvailabilityListingCoordinate::from_canonical(canonicalized)
            .expect("already canonical shared listing reference");
        assert_eq!(reference.as_str(), coordinate_text("farm:lot"));
    }

    #[test]
    fn event_versions_and_listing_coordinates_preserve_distinct_identity() {
        let first_id = radroots_event::EventId::from_bytes([0xab; 32]);
        let first = AvailabilityEventVersion::from_canonical(first_id);
        let repeated = AvailabilityEventVersion::from_hex(&first_id.to_hex())
            .expect("repeated exact event version");
        let revision = AvailabilityEventVersion::from_canonical(
            radroots_event::EventId::from_bytes([0xcd; 32]),
        );
        let listing = coordinate("lot-1");
        let same_listing = AvailabilityListingCoordinate::from_canonical(
            Nip01Coordinate::parse(coordinate_text("lot-1")).expect("same shared coordinate"),
        )
        .expect("same listing coordinate");
        let different_identifier = coordinate("lot-2");
        let other_author = PublicKey::from_bytes([7; 32]).expect("another public author");
        let different_publisher =
            AvailabilityListingCoordinate::parse(&format!("30402:{}:lot-1", other_author.to_hex()))
                .expect("distinct publisher coordinate");

        assert_eq!(first, repeated);
        assert_ne!(first, revision);
        assert_eq!(listing, same_listing);
        assert_ne!(listing, different_identifier);
        assert_ne!(listing, different_publisher);
        assert_eq!(HashSet::from([first, repeated, revision]).len(), 2);
        assert_eq!(
            HashSet::from([
                listing,
                same_listing,
                different_identifier,
                different_publisher
            ])
            .len(),
            3
        );
    }

    #[test]
    fn availability_reference_errors_contain_only_static_safe_messages() {
        const INPUT_MARKER: &str = "raw_availability_input_diagnostic_marker";
        let publisher_error =
            PublicPublisher::from_hex(INPUT_MARKER).expect_err("malformed public author");
        let version_error =
            AvailabilityEventVersion::from_hex(INPUT_MARKER).expect_err("malformed exact event ID");
        let coordinate_error =
            AvailabilityListingCoordinate::parse(&format!("30403:{AUTHOR_HEX}:{INPUT_MARKER}"))
                .expect_err("wrong kind with opaque input marker");

        assert_eq!(publisher_error.code(), SafeErrorCode::InvalidPublicKey);
        assert_invalid_reference(version_error);
        assert_invalid_reference(coordinate_error);
        assert_eq!(
            publisher_error.message(),
            PublicPublisher::from_hex("")
                .expect_err("empty public author")
                .message()
        );
        assert_eq!(
            version_error.message(),
            AvailabilityEventVersion::from_hex("")
                .expect_err("empty event ID")
                .message()
        );
        assert_eq!(
            coordinate_error.message(),
            AvailabilityListingCoordinate::parse("")
                .expect_err("empty coordinate")
                .message()
        );
        for error in [publisher_error, version_error, coordinate_error] {
            assert!(!error.message().as_str().contains(INPUT_MARKER));
            assert!(!error.to_string().contains(INPUT_MARKER));
            assert!(!format!("{error:?}").contains(INPUT_MARKER));
            assert!(error.source().is_none());
        }
    }
}
