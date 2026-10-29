//! Protected count observations retain comparison evidence without changing stock.
use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::{handling, reference, stock};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::counts::{
    CountError, CountObservation, CountObservationContext, CountObserveCommand, CountScope,
    HUB_COUNT_CAPACITY, count_observation_result_digest,
};
use harvestcircle_domain::hub::reference::{HubRecordId, SourceProvenance};
use harvestcircle_domain::hub::{HubId, HubRevision};
use harvestcircle_domain::{PublicKey, SafeError, UnixTimestamp};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use stock::read::{record_id, revision_field, whole_packs};

pub(in crate::hub) mod read;
mod review;
pub use review::HubCountReview;
pub(crate) mod reconciliation_schema;
#[cfg(test)]
mod review_tests;
pub(crate) mod schema;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod tests;
mod write;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCountObservationReceipt {
    pub operation: DurableRequestId,
    pub original: CountObservation,
}

fn operation_id(operation: &DurableRequestId) -> Result<HubRecordId, SafeError> {
    HubRecordId::from_hex(&operation.as_str().replace('-', "")).map_err(|_| stock::invalid_input())
}

fn count_error(error: CountError) -> SafeError {
    match error {
        CountError::StaleRevision | CountError::ReviewRequired => stock::conflict(),
        CountError::RevisionExhausted => stock::capacity(),
        _ => stock::invalid_input(),
    }
}
