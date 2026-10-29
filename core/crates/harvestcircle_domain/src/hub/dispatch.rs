//! Non-reserving preparation and immutable sample identification.
//!
//! These plans do not consume stock, complete work or authorize departure. Native
//! adapters supply current owner/credential checks, complete sources and atomic
//! persistence. Part of a group must first be physically identified by separation.

use super::movement::{PhysicalIdentification, PhysicalStockGroup};
use super::reference::{
    HubRecordId, ReferenceCatalog, ReferenceKind, ReferenceRecord, ReferenceValue,
    SampleHandlingBasis, SourceProvenance, WholePacks,
};
use super::stock::StockPopulation;
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

mod model;
pub use model::*;
mod command;
pub use command::*;
mod result;
pub use result::dispatch_result_digest;

pub const HUB_DISPATCH_CAPACITY: usize = 1_024;
pub const HUB_DISPATCH_LINES: usize = 64;
pub const HUB_IDENTIFICATION_VERSIONS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchError {
    Capacity,
    InvalidSelection,
    InvalidHistory,
    StaleRevision,
    RevisionExhausted,
    PhysicalSeparationRequired,
    InvalidState,
    PreviewChanged,
    NoMaterialChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchIdentities {
    pub operation: HubRecordId,
    pub preparation: HubRecordId,
    pub identification: HubRecordId,
}

impl DispatchIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, DispatchError> {
        let derive = |purpose: &[u8]| {
            let mut hash = Sha256::new();
            hash.update(b"harvestcircle.hub.dispatch.identity.v1\0");
            hash.update(purpose);
            hash.update(operation.as_bytes());
            let digest = hash.finalize();
            HubRecordId::from_bytes(digest[..16].try_into().expect("SHA256 prefix"))
                .map_err(|_| DispatchError::InvalidSelection)
        };
        Ok(Self {
            operation,
            preparation: derive(b"preparation\0")?,
            identification: derive(b"identification\0")?,
        })
    }
}

fn next(revision: HubRevision) -> Result<HubRevision, DispatchError> {
    revision
        .value()
        .checked_add(1)
        .and_then(|value| HubRevision::from_value(value).ok())
        .ok_or(DispatchError::RevisionExhausted)
}

#[cfg(test)]
mod tests;
