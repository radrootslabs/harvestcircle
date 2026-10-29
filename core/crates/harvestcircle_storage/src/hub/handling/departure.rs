//! Actual departure retains original usage, line and work scope atomically.
pub(in crate::hub) mod conservation;
pub(crate) mod schema;

pub(super) mod read;
mod write;
use super::*;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::{reference, stock};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::departure::{DepartureIdentities, DepartureRecord};

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
fn identities(operation: &DurableRequestId) -> Result<DepartureIdentities, SafeError> {
    let raw = HubRecordId::from_hex(&operation.as_str().replace('-', ""))
        .map_err(|_| corrupt_storage())?;
    DepartureIdentities::from_operation_bytes(raw).map_err(|_| corrupt_storage())
}

#[cfg(test)]
mod tests;
