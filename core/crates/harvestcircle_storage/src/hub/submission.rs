//! Native submitted semantics, durable admission, current-session fencing and recovery reads.
mod admission;
pub(in crate::hub) use admission::{uncertainty_all_on, uncertainty_on};
mod capture;
pub(in crate::hub) use capture::capture_references;
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
use sqlx::Row;
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

/// Compare reconstructed immutable native material with the exact admission
/// capture. Historical ownership here is serializer data, never current authority.
/// This encoder comparison cannot decode a saved request or renew a fresh grant.
pub(in crate::hub) async fn verify_recorded_capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: harvestcircle_domain::hub::HubId,
    op: &crate::hub::stock::read::OperationRecord,
    request: HubSubmittedRequest<'_>,
    stock_revision: HubRevision,
    catalog_revision: HubRevision,
    material: &[HubSubmittedMaterial],
) -> Result<[u8; 32], SafeError> {
    let original = read::record_on(tx, &op.id)
        .await?
        .ok_or_else(corrupt_storage)?;
    let input = request.archive().map_err(|_| corrupt_storage())?;
    if original.status != HubSubmissionStatus::Recorded
        || original.hub != hub
        || original.actor != op.actor
        || original.kind != op.kind
        || original.kind != request.kind()
        || original.domain_hash != Some(op.hash)
        || original.input_hash != *input.digest()
    {
        return Err(corrupt_storage());
    }
    let metadata = harvestcircle_application::hub_submission::inspect_hub_submission(
        original.captured_semantics(),
    )
    .map_err(|_| corrupt_storage())?;
    if metadata.hub.id() != hub
        || metadata.scope.actor != op.actor
        || metadata.stock_revision != Some(stock_revision)
        || metadata.catalog_revision != Some(catalog_revision)
    {
        return Err(corrupt_storage());
    }
    let expected = request
        .capture(
            &op.id,
            metadata.scope,
            &HubSubmissionContext {
                hub: metadata.hub,
                catalog_revision: Some(catalog_revision),
                stock_revision: Some(stock_revision),
                material,
            },
        )
        .map_err(|_| corrupt_storage())?;
    if expected.as_bytes() != original.captured_semantics()
        || expected.digest() != original.capture_hash()
    {
        return Err(corrupt_storage());
    }
    Ok(*expected.digest())
}

/// Immutable capture identity only. A pending digest is never a fresh grant;
/// the executor separately requires its owned admission and current session.
pub(in crate::hub) async fn admission_capture_digest_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: harvestcircle_domain::hub::HubId,
    op: &crate::hub::stock::read::OperationRecord,
    request: HubSubmittedRequest<'_>,
) -> Result<[u8; 32], SafeError> {
    let original = read::record_on(tx, &op.id)
        .await?
        .ok_or_else(corrupt_storage)?;
    let input = request.archive().map_err(|_| corrupt_storage())?;
    if matches!(original.status, HubSubmissionStatus::Rejected(_))
        || original.hub != hub
        || original.actor != op.actor
        || original.kind != op.kind
        || original.kind != request.kind()
        || original.domain_hash != Some(op.hash)
        || original.input_hash != *input.digest()
        || original.submitted_at > op.recorded_at
    {
        return Err(corrupt_storage());
    }
    Ok(*original.capture_hash())
}

/// Small immutable identity for lower count projections only. This deliberately
/// does not inspect or authenticate capture_body and is never a full-original
/// proof or an execution grant. Authoritative count consumers separately verify
/// the complete recorded capture through their transaction-owned proof reader.
pub(in crate::hub) async fn count_projection_admission_hash_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: harvestcircle_domain::hub::HubId,
    op: &crate::hub::stock::read::OperationRecord,
    request: HubSubmittedRequest<'_>,
) -> Result<[u8; 32], SafeError> {
    if !matches!(
        op.kind.as_str(),
        "correction_count_explain" | "correction_count_classify"
    ) || request.kind() != op.kind
    {
        return Err(corrupt_storage());
    }
    let input = request.archive().map_err(|_| corrupt_storage())?;
    let row=sqlx::query(concat!(
        "SELECT substr(hub_id,1,17) AS hub,substr(actor_public_key,1,33) AS actor,substr(kind,1,33) AS kind,",
        "substr(domain_intent_hash,1,33) AS intent,substr(input_hash,1,33) AS input,substr(capture_hash,1,33) AS capture,",
        "capture_schema,submitted_at_unix_s,(SELECT count(*) FROM hub_submission_rejections r WHERE r.operation_id=s.operation_id) AS rejected ",
        "FROM hub_submissions s WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let bytes =
        |field| -> Result<Vec<u8>, SafeError> { row.try_get(field).map_err(|_| corrupt_storage()) };
    if bytes("hub")?.as_slice() != hub.as_bytes()
        || bytes("actor")?.as_slice() != op.actor.as_bytes()
        || row
            .try_get::<String, _>("kind")
            .map_err(|_| corrupt_storage())?
            != op.kind
        || bytes("intent")?.as_slice() != op.hash.as_slice()
        || bytes("input")?.as_slice() != input.digest().as_slice()
        || row
            .try_get::<i64, _>("capture_schema")
            .map_err(|_| corrupt_storage())?
            != 1
        || row
            .try_get::<i64, _>("rejected")
            .map_err(|_| corrupt_storage())?
            != 0
    {
        return Err(corrupt_storage());
    }
    let submitted = harvestcircle_domain::UnixTimestamp::from_seconds(
        row.try_get("submitted_at_unix_s")
            .map_err(|_| corrupt_storage())?,
    )
    .ok_or_else(corrupt_storage)?;
    if submitted > op.recorded_at {
        return Err(corrupt_storage());
    }
    bytes("capture")?.try_into().map_err(|_| corrupt_storage())
}

fn archive_error(error: HubRequestArchiveError) -> SafeError {
    match error {
        HubRequestArchiveError::Capacity => stock::capacity(),
        HubRequestArchiveError::InvalidContext => corrupt_storage(),
    }
}

#[cfg(test)]
mod custody_tests;
