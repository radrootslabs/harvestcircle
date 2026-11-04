//! Native submitted semantics, durable admission, current-session fencing and recovery reads.
mod admission;
pub(in crate::hub) use admission::{uncertainty_all_on, uncertainty_on};
mod capture;
mod execute;
mod intent;
pub use execute::HubMutationResult;
#[cfg(test)]
mod actual_tests;
#[cfg(test)]
mod admission_tests;
#[cfg(test)]
mod correction_tests;
mod read;
mod session;
mod targets;
#[cfg(test)]
mod upgrade_tests;
pub use read::{HubSubmissionRecord, HubSubmissionStatus};
#[cfg(test)]
mod session_tests;
pub use session::HubRuntimeSession;
pub(crate) mod schema;
#[cfg(test)]
mod tests;

use super::{counts, handling, reference, stock};
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error, storage_unavailable},
};
use harvestcircle_application::{
    DurableRequestId,
    hub_submission::{
        ArchivedHubSubmission, HubMutation, HubRequestArchiveError, HubSubmissionContext,
        HubSubmissionScope, HubSubmittedDispatch, HubSubmittedDisposition, HubSubmittedGroup,
        HubSubmittedMaterial, HubSubmittedRequest, HubSubmittedReturn,
    },
};
use harvestcircle_domain::hub::{
    HubRevision,
    reference::{HubRecordId, ReferenceCatalog},
};
use harvestcircle_domain::{PublicKey, SafeError};
use radroots_service_sqlite::ServiceSqliteTransaction;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::hub) struct HubUncertainty {
    pub uncertain_intent: bool,
    pub unresolved_actual: bool,
}

impl Database {
    /// Reads one stable current cut. Admission must recapture inside its own transaction.
    /// This read neither persists submission nor authorizes executing the archive.
    pub async fn capture_hub_submission(
        &self,
        actor: PublicKey,
        scope: HubSubmissionScope,
        operation: DurableRequestId,
        request: HubMutation,
    ) -> Result<ArchivedHubSubmission, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    capture::capture_on(tx, actor, scope, &operation, request.submitted_request())
                        .await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

/// Shared current candidate-ID check for read-only previews. It grants neither
/// admission nor replay, including for published pre-admission originals.
pub(in crate::hub) async fn operation_exists_on(
    tx: &mut radroots_service_sqlite::ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
) -> Result<bool, SafeError> {
    Ok(read::record_on(tx, operation).await?.is_some()
        || read::original_on(tx, operation).await?.is_some())
}

/// Correlate a committed typed original with its preserved admission by encoding
/// the original again. This never decodes saved input into a command or grant.
pub(in crate::hub) async fn verify_recorded_input_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: harvestcircle_domain::hub::HubId,
    op: &crate::hub::stock::read::OperationRecord,
    request: HubSubmittedRequest<'_>,
) -> Result<(), SafeError> {
    let original = read::record_on(tx, &op.id)
        .await?
        .ok_or_else(corrupt_storage)?;
    let encoded = request.archive().map_err(|_| corrupt_storage())?;
    if original.status != HubSubmissionStatus::Recorded
        || original.hub != hub
        || original.actor != op.actor
        || original.kind != op.kind
        || original.kind != request.kind()
        || original.domain_hash != Some(op.hash)
        || original.input_hash != *encoded.digest()
    {
        return Err(corrupt_storage());
    }
    Ok(())
}

fn archive_error(error: HubRequestArchiveError) -> SafeError {
    match error {
        HubRequestArchiveError::Capacity => stock::capacity(),
        HubRequestArchiveError::InvalidContext => corrupt_storage(),
    }
}

#[cfg(test)]
mod custody_tests;
