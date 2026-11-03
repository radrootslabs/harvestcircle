//! New returned custody with explicit historical linkage and independent treatment.
//!
//! Pure plans are not execution grants. Native adapters must recapture current
//! ownership, custody, uncertainty, revisions and complete linked-return totals
//! under the protected effect transaction. Nothing here persists or replays an
//! original departure, allocates an ambiguous source, or reverses money.
use super::departure::{DepartureError, DepartureRecord};
use super::dispatch::DispatchLineSnapshot;
use super::movement::{
    PhysicalIdentification, PhysicalStockGroup, WorkAllocation, WorkProgress, WorkRequirement,
};
use super::reference::{
    HubRecordId, HubReferenceError, ReferenceCatalog, ReferenceKind, ReferenceRecord,
    ReferenceValue, SourceAttribution, SourceProvenance, WholePacks,
};
use super::stock::{Occurrence, QuantityWork, StockError, hash_optional_id, hash_source};
use super::treatment::AcceptanceStatus;
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};

mod command;
mod model;
pub use command::{EstablishedReturnTotal, ReturnCommand, ReturnContext};
pub use model::{
    ReturnDepartureClaim, ReturnDepartureHeader, ReturnIntake, ReturnLinkResolution,
    ReturnLinkSnapshot, ReturnPlan, ReturnSourceSnapshot, ReturnTiming, UnresolvedReturnLink,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnError {
    Reference(HubReferenceError),
    Stock(StockError),
    Departure(DepartureError),
    InvalidIdentity,
    InvalidNativeContext,
    EmptyQuantity,
    StaleRevision,
    InvalidPlannedLocation,
    RevisionExhausted,
}
impl From<HubReferenceError> for ReturnError {
    fn from(value: HubReferenceError) -> Self {
        Self::Reference(value)
    }
}
impl From<StockError> for ReturnError {
    fn from(value: StockError) -> Self {
        Self::Stock(value)
    }
}
impl From<DepartureError> for ReturnError {
    fn from(value: DepartureError) -> Self {
        Self::Departure(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReturnIdentities {
    pub operation: HubRecordId,
    pub returned: HubRecordId,
    pub population: HubRecordId,
    pub group: HubRecordId,
    pub source: HubRecordId,
    pub requirement: HubRecordId,
}
impl ReturnIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, ReturnError> {
        let derive = |purpose: &[u8]| {
            let mut hash = Sha256::new();
            hash.update(b"harvestcircle.hub.return.identity.v1\0");
            hash.update(purpose);
            hash.update(operation.as_bytes());
            let digest = hash.finalize();
            HubRecordId::from_bytes(digest[..16].try_into().expect("SHA256 prefix"))
                .map_err(ReturnError::from)
        };
        Ok(Self {
            operation,
            returned: derive(b"return\0")?,
            population: derive(b"population\0")?,
            group: derive(b"group\0")?,
            source: derive(b"source\0")?,
            requirement: derive(b"work\0")?,
        })
    }
    fn validate(self) -> Result<(), ReturnError> {
        if self != Self::from_operation_bytes(self.operation)? {
            return Err(ReturnError::InvalidIdentity);
        }
        let all = [
            self.operation,
            self.returned,
            self.population,
            self.group,
            self.source,
            self.requirement,
        ];
        if all.iter().enumerate().any(|(i, id)| all[..i].contains(id)) {
            return Err(ReturnError::InvalidIdentity);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

mod result;
pub use result::{return_population_digest, return_result_digest};
