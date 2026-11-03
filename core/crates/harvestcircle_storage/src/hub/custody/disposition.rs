//! Distinct observed physical removal, without paper, recipient, money or count loss.
use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::{handling, reference, stock};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::disposition::{
    DispositionActivity, DispositionCommand, DispositionContext, DispositionError,
    DispositionIdentities, DispositionKind, DispositionRecord, disposition_result_digest,
};
use harvestcircle_domain::hub::reference::{HubRecordId, ReferenceCatalog, WholePacks};
use harvestcircle_domain::hub::{HubId, HubRevision};
use harvestcircle_domain::{PublicKey, SafeError, UnixTimestamp};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use stock::read::{OperationRecord, record_id, revision_field, whole_packs};
pub(in crate::hub) mod context;
pub(in crate::hub) mod read;
pub(in crate::hub) mod validation;
mod write;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubDispositionReceipt {
    pub operation: DurableRequestId,
    pub original: DispositionRecord,
}
pub(in crate::hub) fn identities(
    operation: &DurableRequestId,
) -> Result<DispositionIdentities, SafeError> {
    let id = HubRecordId::from_hex(&operation.as_str().replace('-', ""))
        .map_err(|_| stock::invalid_input())?;
    DispositionIdentities::from_operation_bytes(id).map_err(|_| corrupt_storage())
}
fn integer(revision: HubRevision) -> Result<i64, SafeError> {
    i64::try_from(revision.value()).map_err(|_| corrupt_storage())
}
fn error(error: DispositionError) -> SafeError {
    match error {
        DispositionError::StaleRevision | DispositionError::UnresolvedActivity => stock::conflict(),
        DispositionError::RevisionExhausted => stock::capacity(),
        DispositionError::PhysicalSeparationRequired
        | DispositionError::Movement(harvestcircle_domain::hub::movement::MovementError::Stock(
            harvestcircle_domain::hub::stock::StockError::InvalidOccurrence,
        )) => stock::invalid_input(),
        _ => corrupt_storage(),
    }
}
#[cfg(test)]
mod tests;
