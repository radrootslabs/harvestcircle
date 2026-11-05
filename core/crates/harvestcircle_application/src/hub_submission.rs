//! Read-only original request details for native durable submission admission.
//!
//! This encoder has no decoder into domain commands, persistent effect or native
//! authority. The coordinator must still capture resolved references/material,
//! verify the current session and persist admission before executing a mutation.
use harvestcircle_domain::hub::{
    HubOwnership,
    actuals::{ActualDuplicateLink, ActualNewDepartureCommand, ActualReportCommand},
    corrections::{
        AdditionalQuantityCommand, CorrectionObservationCommand, CountClassificationCommand,
        CountExplanationCommand, FoundCorrectionCommand, RecipientAttributionCommand,
        SourceAttributionCommand,
    },
    counts::{CountObserveCommand, CountPostCommand},
    departure::DepartureCommand,
    dispatch::DispatchCommand,
    disposition::DispositionCommand,
    movement::HandlingCommand,
    reference::ReferenceCommand,
    returns::ReturnCommand,
    stock::{OpeningCommand, ReceiptCommand},
};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{fmt, io};

mod context;
mod inspection;
pub use context::*;
pub use inspection::*;

pub const HUB_SUBMITTED_REQUEST_SCHEMA: u8 = 1;
pub const HUB_SUBMITTED_REQUEST_MAX_BYTES: usize = 64 * 1024;
/// Separately bounded ingress for up to 64 independently identified observations.
/// Existing command archives keep their original limit.
pub const HUB_CORRECTION_OBSERVATION_REQUEST_MAX_BYTES: usize = 256 * 1024;

fn request_limit(kind: &str) -> usize {
    if kind == "correction_observe" {
        HUB_CORRECTION_OBSERVATION_REQUEST_MAX_BYTES
    } else {
        HUB_SUBMITTED_REQUEST_MAX_BYTES
    }
}

/// Only product hub inputs can be archived here; identity/secret commands are absent.
pub enum HubSubmittedRequest<'a> {
    Initialize(&'a HubOwnership),
    Reference(&'a ReferenceCommand),
    Receive(&'a ReceiptCommand),
    Opening(&'a OpeningCommand),
    Handling(&'a HandlingCommand),
    Dispatch(&'a DispatchCommand),
    Departure(&'a DepartureCommand),
    CountObservation(&'a CountObserveCommand),
    CountPosting(&'a CountPostCommand),
    Return(&'a ReturnCommand),
    Disposition(&'a DispositionCommand),
    ActualReport(&'a ActualReportCommand),
    ActualReview(&'a ActualNewDepartureCommand),
    ActualLink(&'a ActualDuplicateLink),
    CorrectionObservation(&'a CorrectionObservationCommand),
    FoundCorrection(&'a FoundCorrectionCommand),
    AdditionalQuantity(&'a AdditionalQuantityCommand),
    SourceAttribution(&'a SourceAttributionCommand),
    RecipientAttribution(&'a RecipientAttributionCommand),
    CountExplanation(&'a CountExplanationCommand),
    CountClassification(&'a CountClassificationCommand),
}

/// An owned native request can move through admission without decoding an archive.
/// It carries intent only; ownership, session and effect checks remain native.
pub enum HubMutation {
    Initialize(Box<HubOwnership>),
    Reference(Box<ReferenceCommand>),
    Receive(Box<ReceiptCommand>),
    Opening(Box<OpeningCommand>),
    Handling(Box<HandlingCommand>),
    Dispatch(Box<DispatchCommand>),
    Departure(Box<DepartureCommand>),
    CountObservation(Box<CountObserveCommand>),
    CountPosting(Box<CountPostCommand>),
    Return(Box<ReturnCommand>),
    Disposition(Box<DispositionCommand>),
    ActualReport(Box<ActualReportCommand>),
    ActualReview(Box<ActualNewDepartureCommand>),
    ActualLink(Box<ActualDuplicateLink>),
    CorrectionObservation(Box<CorrectionObservationCommand>),
    FoundCorrection(Box<FoundCorrectionCommand>),
    AdditionalQuantity(Box<AdditionalQuantityCommand>),
    SourceAttribution(Box<SourceAttributionCommand>),
    RecipientAttribution(Box<RecipientAttributionCommand>),
    CountExplanation(Box<CountExplanationCommand>),
    CountClassification(Box<CountClassificationCommand>),
}

impl HubMutation {
    #[must_use]
    pub fn submitted_request(&self) -> HubSubmittedRequest<'_> {
        match self {
            Self::Initialize(value) => HubSubmittedRequest::Initialize(value),
            Self::Reference(value) => HubSubmittedRequest::Reference(value),
            Self::Receive(value) => HubSubmittedRequest::Receive(value),
            Self::Opening(value) => HubSubmittedRequest::Opening(value),
            Self::Handling(value) => HubSubmittedRequest::Handling(value),
            Self::Dispatch(value) => HubSubmittedRequest::Dispatch(value),
            Self::Departure(value) => HubSubmittedRequest::Departure(value),
            Self::CountObservation(value) => HubSubmittedRequest::CountObservation(value),
            Self::CountPosting(value) => HubSubmittedRequest::CountPosting(value),
            Self::Return(value) => HubSubmittedRequest::Return(value),
            Self::Disposition(value) => HubSubmittedRequest::Disposition(value),
            Self::ActualReport(value) => HubSubmittedRequest::ActualReport(value),
            Self::ActualReview(value) => HubSubmittedRequest::ActualReview(value),
            Self::ActualLink(value) => HubSubmittedRequest::ActualLink(value),
            Self::CorrectionObservation(value) => HubSubmittedRequest::CorrectionObservation(value),
            Self::FoundCorrection(value) => HubSubmittedRequest::FoundCorrection(value),
            Self::AdditionalQuantity(value) => HubSubmittedRequest::AdditionalQuantity(value),
            Self::SourceAttribution(value) => HubSubmittedRequest::SourceAttribution(value),
            Self::RecipientAttribution(value) => HubSubmittedRequest::RecipientAttribution(value),
            Self::CountExplanation(value) => HubSubmittedRequest::CountExplanation(value),
            Self::CountClassification(value) => HubSubmittedRequest::CountClassification(value),
        }
    }
}

impl HubSubmittedRequest<'_> {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Initialize(_) => "initialize",
            Self::Reference(_) => "reference",
            Self::Receive(_) => "receipt",
            Self::Opening(_) => "opening",
            Self::Handling(command) => command.kind(),
            Self::Dispatch(command) => command.kind(),
            Self::Departure(_) => "departure",
            Self::CountObservation(_) => "count_observe",
            Self::CountPosting(_) => "count_post",
            Self::Return(_) => "return",
            Self::Disposition(_) => "disposition",
            Self::ActualReport(_) => "actual_report",
            Self::ActualReview(_) | Self::ActualLink(_) => "actual_review",
            Self::CorrectionObservation(_) => "correction_observe",
            Self::FoundCorrection(_) => "correction_found",
            Self::AdditionalQuantity(_) => "correction_additional",
            Self::SourceAttribution(_) => "correction_source",
            Self::RecipientAttribution(_) => "correction_recipient",
            Self::CountExplanation(_) => "correction_count_explain",
            Self::CountClassification(_) => "correction_count_classify",
        }
    }

    pub fn archive(&self) -> Result<ArchivedHubRequest, HubRequestArchiveError> {
        #[derive(Serialize)]
        struct Envelope<'a, 'b> {
            schema: u8,
            kind: &'static str,
            input: &'a HubSubmittedRequest<'b>,
        }
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            limit: request_limit(self.kind()),
        };
        serde_json::to_writer(
            &mut writer,
            &Envelope {
                schema: HUB_SUBMITTED_REQUEST_SCHEMA,
                kind: self.kind(),
                input: self,
            },
        )
        .map_err(|_| HubRequestArchiveError::Capacity)?;
        let digest = archive_digest(&writer.bytes);
        Ok(ArchivedHubRequest {
            kind: self.kind(),
            bytes: writer.bytes,
            digest,
        })
    }
}

impl Serialize for HubSubmittedRequest<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Initialize(command) => command.serialize(serializer),
            Self::Reference(command) => command.serialize(serializer),
            Self::Receive(command) => command.serialize(serializer),
            Self::Opening(command) => command.serialize(serializer),
            Self::Handling(command) => command.serialize(serializer),
            Self::Dispatch(command) => command.serialize(serializer),
            Self::Departure(command) => command.serialize(serializer),
            Self::CountObservation(command) => command.serialize(serializer),
            Self::CountPosting(command) => command.serialize(serializer),
            Self::Return(command) => command.serialize(serializer),
            Self::Disposition(command) => command.serialize(serializer),
            Self::ActualReport(command) => command.serialize(serializer),
            Self::CorrectionObservation(command) => command.serialize(serializer),
            Self::FoundCorrection(command) => command.serialize(serializer),
            Self::AdditionalQuantity(command) => command.serialize(serializer),
            Self::SourceAttribution(command) => command.serialize(serializer),
            Self::RecipientAttribution(command) => command.serialize(serializer),
            Self::CountExplanation(command) => command.serialize(serializer),
            Self::CountClassification(command) => command.serialize(serializer),
            Self::ActualReview(command) => {
                #[derive(Serialize)]
                struct Input<'a> {
                    review_kind: &'static str,
                    details: &'a ActualNewDepartureCommand,
                }
                Input {
                    review_kind: "departure",
                    details: command,
                }
                .serialize(serializer)
            }
            Self::ActualLink(command) => {
                #[derive(Serialize)]
                struct Input<'a> {
                    review_kind: &'static str,
                    details: &'a ActualDuplicateLink,
                }
                Input {
                    review_kind: "link",
                    details: command,
                }
                .serialize(serializer)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubRequestArchiveError {
    Capacity,
    InvalidContext,
}

/// Exact bounded request bytes, not a replay command, signature or permission.
#[derive(Clone, Eq, PartialEq)]
pub struct ArchivedHubRequest {
    kind: &'static str,
    bytes: Vec<u8>,
    digest: [u8; 32],
}
impl ArchivedHubRequest {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}
impl fmt::Debug for ArchivedHubRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArchivedHubRequest")
            .field("kind", &self.kind)
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

fn archive_digest(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.submitted_request.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .expect("bounded request bytes")
            .to_be_bytes(),
    );
    hash.update(bytes);
    hash.finalize().into()
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl io::Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("request capacity"))?;
        if end > self.limit {
            return Err(io::Error::other("request capacity"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
