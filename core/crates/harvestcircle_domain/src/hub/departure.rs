//! Atomic normal departure of exactly identified whole groups.
//!
//! Native adapters supply complete current sources/treatment and current owner,
//! credential, usage and journal-budget checks in one transaction. A plan is not
//! authority and does not perform persistence or external effects.
use super::dispatch::{DispatchError, DispatchRecord, DispatchSource};
use super::movement::{MovementError, PhysicalStockGroup, WorkProgress, WorkRequirement};
use super::portions::PhysicalLineage;
use super::reference::{HubRecordId, ReferenceCatalog, SourceProvenance, WholePacks};
use super::stock::{Occurrence, QuantityWork};
use super::treatment::{TreatmentError, TreatmentState};
use super::{HubId, HubRevision};
use crate::{PublicKey, UnixTimestamp};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

mod command;
mod model;
pub use command::{DepartureCommand, DepartureContext, DepartureTreatment};
pub use model::{ConsumedStockGroup, DepartureRecord, DepartureWorkChange};

pub const HUB_DEPARTURE_WORK_CHANGES: usize =
    super::dispatch::HUB_DISPATCH_LINES * super::movement::HUB_GROUP_REQUIREMENTS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DepartureError {
    InvalidIdentity,
    InvalidHistory,
    InvalidSources,
    StaleRevision,
    IdentificationUnavailable,
    AlreadyUsed,
    MaterialChanged,
    Capacity,
    RevisionExhausted,
    Dispatch(DispatchError),
    Movement(MovementError),
    Treatment(TreatmentError),
}
impl From<DispatchError> for DepartureError {
    fn from(value: DispatchError) -> Self {
        Self::Dispatch(value)
    }
}
impl From<MovementError> for DepartureError {
    fn from(value: MovementError) -> Self {
        Self::Movement(value)
    }
}
impl From<TreatmentError> for DepartureError {
    fn from(value: TreatmentError) -> Self {
        Self::Treatment(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepartureIdentities {
    pub operation: HubRecordId,
    pub departure: HubRecordId,
}
impl DepartureIdentities {
    pub fn from_operation_bytes(operation: HubRecordId) -> Result<Self, DepartureError> {
        let mut hash = Sha256::new();
        hash.update(b"harvestcircle.hub.departure.identity.v1\0");
        hash.update(operation.as_bytes());
        let digest = hash.finalize();
        Ok(Self {
            operation,
            departure: HubRecordId::from_bytes(digest[..16].try_into().expect("SHA256 prefix"))
                .map_err(|_| DepartureError::InvalidIdentity)?,
        })
    }
}
fn next(value: HubRevision) -> Result<HubRevision, DepartureError> {
    value.next().map_err(|_| DepartureError::RevisionExhausted)
}

// One requirement can span multiple selected groups. Freeze one consistent
// original, sum only pending exit, and advance the requirement exactly once.
fn retired_work(groups: &[ConsumedStockGroup]) -> Result<Vec<DepartureWorkChange>, DepartureError> {
    let mut work: BTreeMap<HubRecordId, (WorkRequirement, WholePacks)> = BTreeMap::new();
    for consumed in groups {
        consumed.before.validate()?;
        for allocation in &consumed.before.work {
            let entry = work.entry(allocation.requirement.id).or_insert_with(|| {
                (
                    allocation.requirement.clone(),
                    WholePacks::from_value(0).expect("zero whole packs"),
                )
            });
            if entry.0 != allocation.requirement {
                return Err(DepartureError::InvalidSources);
            }
            if allocation.progress == WorkProgress::Pending {
                entry.1 = entry
                    .1
                    .checked_add(consumed.before.packs)
                    .map_err(|_| DepartureError::InvalidSources)?;
            }
        }
        if work.len() > HUB_DEPARTURE_WORK_CHANGES {
            return Err(DepartureError::Capacity);
        }
    }
    let mut changes = Vec::new();
    for (before, retired) in work
        .into_values()
        .filter(|(_, retired)| retired.value() > 0)
    {
        let quantity = QuantityWork::restore(
            before.quantity.required(),
            before.quantity.completed(),
            before
                .quantity
                .no_longer_required()
                .checked_add(retired)
                .map_err(|_| DepartureError::InvalidSources)?,
            before
                .quantity
                .remaining()
                .checked_sub(retired)
                .map_err(|_| DepartureError::InvalidSources)?,
        )
        .map_err(|_| DepartureError::InvalidSources)?;
        let after = WorkRequirement {
            quantity,
            revision: next(before.revision)?,
            ..before.clone()
        };
        after.validate()?;
        changes.push(DepartureWorkChange {
            before,
            after,
            retired,
        });
    }
    Ok(changes)
}

#[cfg(test)]
mod tests;
