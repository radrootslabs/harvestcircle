//! Native submitted semantics, durable admission, current-session fencing and recovery reads.
mod admission;
mod capture;
mod execute;
mod intent;
pub use execute::HubMutationResult;
#[cfg(test)]
mod admission_tests;
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
        HubSubmissionScope, HubSubmittedDispatch, HubSubmittedGroup, HubSubmittedMaterial,
        HubSubmittedRequest,
    },
};
use harvestcircle_domain::hub::{
    HubRevision,
    reference::{HubRecordId, ReferenceCatalog},
};
use harvestcircle_domain::{PublicKey, SafeError};
use radroots_service_sqlite::ServiceSqliteTransaction;
use std::collections::{BTreeMap, BTreeSet};

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

fn archive_error(error: HubRequestArchiveError) -> SafeError {
    match error {
        HubRequestArchiveError::Capacity => stock::capacity(),
        HubRequestArchiveError::InvalidContext => corrupt_storage(),
    }
}
