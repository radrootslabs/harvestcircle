//! Actual departure retains original usage, line and work scope atomically.
pub(in crate::hub) mod conservation;
pub(crate) mod schema;

mod plan;
pub(super) mod read;
mod review;
mod write;
use super::dispatch::HubDispatchView;
use super::*;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::{reference, stock};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::departure::{
    DepartureIdentities, DepartureRecord, DepartureWorkChange,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDepartureReceipt {
    pub operation: DurableRequestId,
    pub record: DepartureRecord,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDepartureUse {
    pub id: HubRecordId,
    pub operation: DurableRequestId,
    pub preparation: HubRecordId,
    pub identification: HubRecordId,
    pub revision: HubRevision,
    pub actor: PublicKey,
    pub recorded_at: UnixTimestamp,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDepartureReviewInput {
    pub preparation: HubRecordId,
    pub expected_preparation_revision: HubRevision,
    pub identification: HubRecordId,
}
/// Captured readiness is observation only, never a reservation or future effect grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDepartureReview {
    pub request: HubDepartureReviewInput,
    pub view: HubDispatchView,
    pub block: Option<SafeError>,
    pub proposed_work_changes: Vec<DepartureWorkChange>,
}
fn identities(operation: &DurableRequestId) -> Result<DepartureIdentities, SafeError> {
    let raw = HubRecordId::from_hex(&operation.as_str().replace('-', ""))
        .map_err(|_| corrupt_storage())?;
    DepartureIdentities::from_operation_bytes(raw).map_err(|_| corrupt_storage())
}

#[cfg(test)]
mod tests;
