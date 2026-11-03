//! Returned custody is a new population, never a reversal of an old departure.
use crate::Database;
use crate::db::{corrupt_storage, map_transaction_error, storage_unavailable};
use crate::hub::{handling, reference, stock};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::hub::departure::DepartureRecord;
use harvestcircle_domain::hub::reference::{HubRecordId, ReferenceCatalog, ReferenceKind};
use harvestcircle_domain::hub::returns::{
    EstablishedReturnTotal, ReturnCommand, ReturnContext, ReturnError, ReturnIdentities,
    ReturnLinkResolution, ReturnPlan,
};
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
pub struct HubReturnReceipt {
    pub operation: DurableRequestId,
    pub original: ReturnPlan,
}
pub(in crate::hub) fn identities(
    operation: &DurableRequestId,
) -> Result<ReturnIdentities, SafeError> {
    let id = HubRecordId::from_hex(&operation.as_str().replace('-', ""))
        .map_err(|_| stock::invalid_input())?;
    ReturnIdentities::from_operation_bytes(id).map_err(|_| corrupt_storage())
}
fn error(error: ReturnError) -> SafeError {
    match error {
        ReturnError::StaleRevision
        | ReturnError::Stock(
            harvestcircle_domain::hub::stock::StockError::SourceIdentityConflict,
        ) => stock::conflict(),
        ReturnError::RevisionExhausted => stock::capacity(),
        ReturnError::InvalidNativeContext | ReturnError::InvalidIdentity => corrupt_storage(),
        _ => stock::invalid_input(),
    }
}

fn resolution(value: &ReturnLinkResolution) -> (&'static str, Option<&'static str>) {
    use harvestcircle_domain::hub::returns::UnresolvedReturnLink as Why;
    match value {
        ReturnLinkResolution::Unknown => ("unknown", None),
        ReturnLinkResolution::Unrecorded => ("unrecorded", None),
        ReturnLinkResolution::EstablishedScope => ("established", None),
        ReturnLinkResolution::Unresolved(reason) => (
            "unresolved",
            Some(match reason {
                Why::OriginalGroupUnknown => "original_group_unknown",
                Why::NoMatchingVariant => "no_matching_variant",
                Why::ClaimedGroupMissing => "claimed_group_missing",
                Why::VariantConflict => "variant_conflict",
                Why::QuantityConflict => "quantity_conflict",
                Why::TimeConflict => "time_conflict",
            }),
        ),
    }
}
fn timing(value: harvestcircle_domain::hub::returns::ReturnTiming) -> &'static str {
    use harvestcircle_domain::hub::returns::ReturnTiming;
    match value {
        ReturnTiming::AfterOrSame => "after_or_same",
        ReturnTiming::Uncertain => "uncertain",
        ReturnTiming::Contradictory => "contradictory",
    }
}

#[cfg(test)]
mod tests;
