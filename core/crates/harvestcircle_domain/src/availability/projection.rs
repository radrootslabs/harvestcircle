//! Immutable public availability evidence and retained head views.
//!
//! Shared verification, tolerant admission and deletion evaluation own protocol
//! meaning. These views establish no physical stock, publisher ownership or
//! network authority. Original JSON extras remain unauthenticated observations.

use std::fmt;

use radroots_event::envelope::EventTimestamp;
use radroots_event::envelope::event_head::{
    CurrentEventHead, EventHeadCandidateResult, EventHeadCoordinate,
    event_head_candidate_for_nip01_event,
};
use radroots_event::envelope::kind::KIND_CLASSIFIED_LISTING;
use radroots_event::id::{RADROOTS_NIP01_COORDINATE_MAX_BYTES, RelayUrl};
use radroots_event::listing::classified::ClassifiedListingPartition;
use radroots_event::wire::{DEFAULT_RAW_JSON_MAX_BYTES, Nip01EventWire};
use radroots_event_codec::admission::deletion::{
    RadrootsAdmittedNip09DeletionRequestEvent, RadrootsNip09SuppressionDecision,
    RadrootsNip09SuppressionOutcome, evaluate_nip09_suppression_from_borrowed_requests_v1,
};
use radroots_event_codec::admission::food_availability::{
    RadrootsFoodAvailabilityAdmissionError, RadrootsFoodAvailabilityAdmissionOutcome,
    admit_verified_food_availability_event,
};
use radroots_event_codec::decode::food_availability::RadrootsInboundFoodAvailabilityProjection;
use radroots_event_codec::verify::RadrootsSignatureVerifiedEvent;

use crate::{
    Kind0ProfileCandidate, ProfileMetadata, PublicKey, SafeError, SafeErrorCode, SafeMessage,
    UnixTimestamp,
};

use super::{AvailabilityEventVersion, AvailabilityListingCoordinate, PublicPublisher};

const MAX_OBSERVATION_SOURCE_BYTES: usize = 2048;
const MAX_DELETION_REQUESTS: usize = 4096;
const LISTING_COORDINATE_FRAMING_BYTES: usize = "30402:".len() + 64 + 1;

/// A bounded, exact source reference and local observation time.
///
/// The source records provenance only. Pure shared URL validation neither
/// normalizes endpoint aliases nor authorizes a destination or connection.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityObservation {
    source: RelayUrl,
    observed_at: UnixTimestamp,
}

impl AvailabilityObservation {
    /// Parses a pure source reference after checking its UTF-8 byte bound.
    ///
    /// # Errors
    ///
    /// Returns a static safe error for an oversized or invalid source reference.
    pub fn parse(source: &str, observed_at: UnixTimestamp) -> Result<Self, SafeError> {
        if source.len() > MAX_OBSERVATION_SOURCE_BYTES {
            return Err(invalid_view());
        }
        let source = RelayUrl::parse(source).map_err(|_| invalid_view())?;
        Ok(Self {
            source,
            observed_at,
        })
    }

    #[must_use]
    pub const fn source(&self) -> &RelayUrl {
        &self.source
    }

    #[must_use]
    pub const fn observed_at(&self) -> UnixTimestamp {
        self.observed_at
    }
}

impl fmt::Debug for AvailabilityObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityObservation")
            .field("source_bytes", &self.source.as_str().len())
            .field("observed_at", &self.observed_at)
            .finish()
    }
}

/// The shared reason a verified listing cannot provide focused Food data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityUnsupportedReason {
    Excluded(ClassifiedListingPartition),
    ProjectionRejected(&'static str),
}

#[derive(Clone, Eq, PartialEq)]
enum AvailabilityProjection {
    Focused(Box<RadrootsInboundFoodAvailabilityProjection>),
    Unsupported(AvailabilityUnsupportedReason),
}

/// One exact verified version, its original wire and tolerant admission result.
///
/// The raw head coordinate survives independently of the narrower app reference.
/// Profile metadata, when present, is caller-supplied author-associated display
/// data; it carries no signature typestate or farm-ownership proof.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityVersionView {
    verified_event: RadrootsSignatureVerifiedEvent,
    original_json: String,
    raw_coordinate: EventHeadCoordinate,
    listing_coordinate: Option<AvailabilityListingCoordinate>,
    projection: AvailabilityProjection,
    observation: AvailabilityObservation,
    profile_metadata: Option<ProfileMetadata>,
}

impl AvailabilityVersionView {
    /// Binds shared signature evidence to the complete original wire envelope.
    ///
    /// Original formatting and unknown JSON extras are retained exactly. Extras
    /// are outside the signed envelope and do not gain authenticated meaning.
    /// Shared tolerant admission preserves valid exclusions and projection
    /// rejections as unsupported evidence of this version.
    ///
    /// # Errors
    ///
    /// Returns a static safe error for wire limits, structural errors, a wire
    /// mismatch, an unsupported kind or an unavailable shared admission result.
    pub fn from_verified(
        verified_event: RadrootsSignatureVerifiedEvent,
        original_json: &str,
        observation: AvailabilityObservation,
    ) -> Result<Self, SafeError> {
        if original_json.len() > DEFAULT_RAW_JSON_MAX_BYTES
            || verified_event.event().kind_u32() != KIND_CLASSIFIED_LISTING
        {
            return Err(invalid_view());
        }
        let envelope = Nip01EventWire::parse_json_unverified(original_json)
            .map_err(|_| invalid_view())?
            .into_unverified_envelope()
            .map_err(|_| invalid_view())?;
        if &envelope != verified_event.event() {
            return Err(invalid_view());
        }
        let raw_coordinate = match event_head_candidate_for_nip01_event(verified_event.event()) {
            EventHeadCandidateResult::Candidate(candidate) => candidate.coordinate,
            _ => return Err(invalid_view()),
        };
        let listing_coordinate = app_listing_coordinate(&raw_coordinate);
        // Full default-limit wire equality precedes this bounded evidence copy.
        let projection = match admit_verified_food_availability_event(verified_event.clone()) {
            Ok(RadrootsFoodAvailabilityAdmissionOutcome::Admitted(admitted)) => {
                let (_, projection) = (*admitted).into_parts();
                AvailabilityProjection::Focused(Box::new(projection))
            }
            Ok(RadrootsFoodAvailabilityAdmissionOutcome::Excluded(excluded)) => {
                AvailabilityProjection::Unsupported(AvailabilityUnsupportedReason::Excluded(
                    excluded.partition(),
                ))
            }
            Err(RadrootsFoodAvailabilityAdmissionError::Projection(error)) => {
                AvailabilityProjection::Unsupported(
                    AvailabilityUnsupportedReason::ProjectionRejected(error.code()),
                )
            }
            _ => return Err(invalid_view()),
        };
        Ok(Self {
            verified_event,
            original_json: original_json.to_owned(),
            raw_coordinate,
            listing_coordinate,
            projection,
            observation,
            profile_metadata: None,
        })
    }

    #[must_use]
    pub fn version(&self) -> AvailabilityEventVersion {
        AvailabilityEventVersion::from_canonical(*self.verified_event.event().id())
    }

    #[must_use]
    pub fn publisher(&self) -> PublicPublisher {
        PublicPublisher::from_public_key(PublicKey::from_canonical(
            *self.verified_event.event().author(),
        ))
    }

    #[must_use]
    pub fn created_at(&self) -> EventTimestamp {
        self.verified_event.event().created_at()
    }

    #[must_use]
    pub fn original_json(&self) -> &str {
        &self.original_json
    }

    #[must_use]
    pub const fn observation(&self) -> &AvailabilityObservation {
        &self.observation
    }

    #[must_use]
    pub const fn raw_coordinate(&self) -> &EventHeadCoordinate {
        &self.raw_coordinate
    }

    #[must_use]
    pub fn listing_coordinate(&self) -> Option<&AvailabilityListingCoordinate> {
        self.listing_coordinate.as_ref()
    }

    #[must_use]
    pub fn focused(&self) -> Option<&RadrootsInboundFoodAvailabilityProjection> {
        match &self.projection {
            AvailabilityProjection::Focused(projection) => Some(projection),
            AvailabilityProjection::Unsupported(_) => None,
        }
    }

    #[must_use]
    pub const fn unsupported_reason(&self) -> Option<&AvailabilityUnsupportedReason> {
        match &self.projection {
            AvailabilityProjection::Focused(_) => None,
            AvailabilityProjection::Unsupported(reason) => Some(reason),
        }
    }

    #[must_use]
    pub fn profile_metadata(&self) -> Option<&ProfileMetadata> {
        self.profile_metadata.as_ref()
    }

    /// Returns a copy with optional caller-supplied, author-associated metadata.
    ///
    /// Matching authors do not establish a verified profile or ownership.
    /// Rejection leaves this immutable listing view usable.
    ///
    /// # Errors
    ///
    /// Returns a static safe error when the supplied candidate author differs.
    pub fn with_profile(&self, profile: Option<&Kind0ProfileCandidate>) -> Result<Self, SafeError> {
        if profile.is_some_and(|candidate| candidate.author() != self.publisher().public_key()) {
            return Err(invalid_view());
        }
        let mut view = self.clone();
        view.profile_metadata = profile.map(|candidate| candidate.metadata().clone());
        Ok(view)
    }
}

impl fmt::Debug for AvailabilityVersionView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityVersionView")
            .field("created_at", &self.created_at())
            .field("wire_bytes", &self.original_json.len())
            .field("focused", &self.focused().is_some())
            .field("unsupported_reason", &self.unsupported_reason())
            .field("profile_present", &self.profile_metadata.is_some())
            .field("observation", &self.observation)
            .finish()
    }
}

/// The retained selected head's state, without a physical-supply claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityHeadState {
    Missing,
    Focused,
    Unsupported,
    Deleted,
}

#[derive(Clone, Eq, PartialEq)]
enum AvailabilityHeadEvidence {
    Missing(AvailabilityListingCoordinate),
    Selected {
        version: Box<AvailabilityVersionView>,
        suppression: RadrootsNip09SuppressionDecision,
    },
}

/// Absence of retained knowledge or an exact selected version and shared decision.
///
/// Deleted versions remain available as historical evidence. Neither missing
/// knowledge nor listing query completion establishes exhaustive deletion data.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityHeadView {
    evidence: AvailabilityHeadEvidence,
}

impl AvailabilityHeadView {
    #[must_use]
    pub const fn missing(coordinate: AvailabilityListingCoordinate) -> Self {
        Self {
            evidence: AvailabilityHeadEvidence::Missing(coordinate),
        }
    }

    /// Validates exact correspondence to the supplied shared selected head and
    /// delegates suppression to the shared evaluator over borrowed requests.
    ///
    /// # Errors
    ///
    /// Returns a static safe error when coordinate, event ID or signed time
    /// differs, or more than4096 admitted deletion requests are supplied.
    pub fn from_selected(
        selected: CurrentEventHead,
        version: AvailabilityVersionView,
        requests: &[RadrootsAdmittedNip09DeletionRequestEvent],
    ) -> Result<Self, SafeError> {
        if requests.len() > MAX_DELETION_REQUESTS
            || &selected.coordinate != version.raw_coordinate()
            || selected.event_id != version.version().event_id()
            || selected.created_at != version.created_at().as_u64()
        {
            return Err(invalid_view());
        }
        let suppression = evaluate_nip09_suppression_from_borrowed_requests_v1(
            &version.verified_event,
            requests.iter(),
        );
        Ok(Self {
            evidence: AvailabilityHeadEvidence::Selected {
                version: Box::new(version),
                suppression,
            },
        })
    }

    #[must_use]
    pub fn state(&self) -> AvailabilityHeadState {
        match &self.evidence {
            AvailabilityHeadEvidence::Missing(_) => AvailabilityHeadState::Missing,
            AvailabilityHeadEvidence::Selected {
                version,
                suppression,
            } => {
                if suppression.outcome() == RadrootsNip09SuppressionOutcome::Suppressed {
                    AvailabilityHeadState::Deleted
                } else if version.focused().is_some() {
                    AvailabilityHeadState::Focused
                } else {
                    AvailabilityHeadState::Unsupported
                }
            }
        }
    }

    /// Returns the retained version, including historical deleted evidence.
    #[must_use]
    pub fn version(&self) -> Option<&AvailabilityVersionView> {
        match &self.evidence {
            AvailabilityHeadEvidence::Missing(_) => None,
            AvailabilityHeadEvidence::Selected { version, .. } => Some(version),
        }
    }

    #[must_use]
    pub const fn suppression(&self) -> Option<&RadrootsNip09SuppressionDecision> {
        match &self.evidence {
            AvailabilityHeadEvidence::Missing(_) => None,
            AvailabilityHeadEvidence::Selected { suppression, .. } => Some(suppression),
        }
    }

    /// Returns focused data only when the selected version remains visible.
    #[must_use]
    pub fn focused(&self) -> Option<&RadrootsInboundFoodAvailabilityProjection> {
        if self.state() == AvailabilityHeadState::Focused {
            self.version().and_then(AvailabilityVersionView::focused)
        } else {
            None
        }
    }
}

impl fmt::Debug for AvailabilityHeadView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut summary = formatter.debug_struct("AvailabilityHeadView");
        summary.field("state", &self.state());
        match &self.evidence {
            AvailabilityHeadEvidence::Missing(coordinate) => {
                summary.field("coordinate_bytes", &coordinate.as_str().len());
            }
            AvailabilityHeadEvidence::Selected {
                version,
                suppression,
            } => {
                summary
                    .field("version", version)
                    .field("suppression_reason", &suppression.reason());
            }
        }
        summary.finish()
    }
}

fn app_listing_coordinate(raw: &EventHeadCoordinate) -> Option<AvailabilityListingCoordinate> {
    let EventHeadCoordinate::Addressable {
        kind,
        pubkey,
        d_tag,
    } = raw
    else {
        return None;
    };
    if *kind != KIND_CLASSIFIED_LISTING
        || d_tag.is_empty()
        || d_tag.len() > RADROOTS_NIP01_COORDINATE_MAX_BYTES - LISTING_COORDINATE_FRAMING_BYTES
    {
        return None;
    }
    // Borrowed framing bounds precede author encoding and coordinate allocation.
    AvailabilityListingCoordinate::parse(&format!("30402:{}:{d_tag}", pubkey.to_hex())).ok()
}

const fn invalid_view() -> SafeError {
    SafeError::new(
        SafeErrorCode::InvalidProfileMetadata,
        SafeMessage::new("The public availability view is invalid."),
    )
}
