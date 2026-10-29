//! Read-only original request details for native durable submission admission.
//!
//! This encoder has no decoder into domain commands, persistent effect or native
//! authority. The coordinator must still capture resolved references/material,
//! verify the current session and persist admission before executing a mutation.
use harvestcircle_domain::hub::{
    HubOwnership,
    counts::{CountObserveCommand, CountPostCommand},
    departure::DepartureCommand,
    dispatch::DispatchCommand,
    movement::HandlingCommand,
    reference::ReferenceCommand,
    stock::{OpeningCommand, ReceiptCommand},
};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{fmt, io};

pub const HUB_SUBMITTED_REQUEST_SCHEMA: u8 = 1;
pub const HUB_SUBMITTED_REQUEST_MAX_BYTES: usize = 64 * 1024;

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
            limit: HUB_SUBMITTED_REQUEST_MAX_BYTES,
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
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubRequestArchiveError {
    Capacity,
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
