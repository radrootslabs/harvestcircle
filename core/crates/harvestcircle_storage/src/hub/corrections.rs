//! Append-only correction facts and canonical original reads. Saved submission
//! data never becomes a fresh native admission or an effect authorization.
use crate::{
    Database,
    db::{corrupt_storage, map_transaction_error, storage_unavailable},
    hub::{handling, reference, stock},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    PublicKey, SafeError, UnixTimestamp,
    hub::{
        HubId, HubRevision,
        corrections::{
            CorrectionObservationCommand, CorrectionObservationError, CorrectionObservationRecord,
            HUB_CORRECTION_OBSERVATION_CAPACITY, correction_observation_result_digest,
        },
        reference::HubRecordId,
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use sqlx::Row;
use stock::read::OperationRecord;

const OBSERVATION_TOTAL_BYTES: i64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Default)]
pub(in crate::hub) struct ObservationReservations {
    pub operations: u32,
    pub events: u32,
    pub reviews: u32,
}

fn closure_events(command: &CorrectionObservationCommand) -> Result<u32, SafeError> {
    use harvestcircle_domain::hub::corrections::CorrectionObservedFacts;
    command.validate().map_err(error)?;
    match &command.facts {
        CorrectionObservedFacts::Found { allocations, .. } => {
            u32::try_from(allocations.len()).map_err(|_| stock::capacity())
        }
        CorrectionObservedFacts::CountClassification { .. } => Ok(0),
    }
}

pub(in crate::hub) async fn reservations_on(
    tx: &mut ServiceSqliteTransaction<'_>,
) -> Result<ObservationReservations, SafeError> {
    let row=sqlx::query(concat!(
        "SELECT count(*) AS operations,COALESCE(sum(closure_events),0) AS events,COALESCE(sum(length(payload)),0) AS bytes FROM hub_correction_observations observation ",
        "WHERE NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=observation.operation_id) ",
        "AND NOT EXISTS(SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=observation.operation_id)"
    )).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let operations: i64 = row.try_get("operations").map_err(|_| corrupt_storage())?;
    let events: i64 = row.try_get("events").map_err(|_| corrupt_storage())?;
    let bytes: i64 = row.try_get("bytes").map_err(|_| corrupt_storage())?;
    if !(0..=HUB_CORRECTION_OBSERVATION_CAPACITY as i64).contains(&operations)
        || !(0..=operations * 64).contains(&events)
    {
        return Err(corrupt_storage());
    }
    if !(0..=OBSERVATION_TOTAL_BYTES).contains(&bytes) {
        return Err(corrupt_storage());
    }
    if operations == 0 {
        return Ok(ObservationReservations::default());
    }
    let (hub, _) = super::load_on(tx).await?.ok_or_else(corrupt_storage)?;
    let ids:Vec<String>=sqlx::query_scalar(concat!(
        "SELECT substr(observation.operation_id,1,37) FROM hub_correction_observations observation ",
        "JOIN hub_stock_operations original ON original.operation_id=observation.operation_id ",
        "WHERE NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=observation.operation_id) ",
        "AND NOT EXISTS(SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=observation.operation_id) ",
        "ORDER BY original.stock_revision LIMIT 4097"
    )).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if i64::try_from(ids.len()).map_err(|_| corrupt_storage())? != operations {
        return Err(corrupt_storage());
    }
    let mut verified = 0_u32;
    let mut reviews = 0_u32;
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub.id(), &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let original = original_on(tx, hub.id(), op).await?;
        if matches!(
            &original.original.observation.facts,
            harvestcircle_domain::hub::corrections::CorrectionObservedFacts::Found { .. }
        ) {
            reviews = reviews.checked_add(1).ok_or_else(corrupt_storage)?;
        }
        verified = verified
            .checked_add(
                closure_events(&original.original.observation).map_err(|_| corrupt_storage())?,
            )
            .ok_or_else(corrupt_storage)?;
    }
    if i64::from(verified) != events {
        return Err(corrupt_storage());
    }
    Ok(ObservationReservations {
        operations: u32::try_from(operations).map_err(|_| corrupt_storage())?,
        events: verified,
        reviews,
    })
}

pub(in crate::hub) async fn headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid: i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_correction_observations c ON c.operation_id=o.operation_id ",
        "WHERE o.kind='correction_observe' AND (c.operation_id IS NULL OR o.hub_id<>? OR o.result_id<>c.observation_id ",
        "OR EXISTS(SELECT 1 FROM hub_stock_events event WHERE event.operation_id=o.operation_id) ",
        "OR NOT EXISTS(SELECT 1 FROM hub_submissions s WHERE s.operation_id=o.operation_id AND s.hub_id=o.hub_id ",
        "AND s.actor_public_key=o.actor_public_key AND s.kind=o.kind AND s.domain_intent_hash=o.intent_hash AND s.submitted_at_unix_s<=o.recorded_at_unix_s))"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let (bodies, operations, bytes): (i64, i64, i64) = sqlx::query_as(concat!(
        "SELECT (SELECT count(*) FROM hub_correction_observations),",
        "(SELECT count(*) FROM hub_stock_operations WHERE kind='correction_observe'),",
        "(SELECT COALESCE(sum(length(payload)),0) FROM hub_correction_observations)"
    ))
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if invalid != 0
        || bodies != operations
        || !(0..=HUB_CORRECTION_OBSERVATION_CAPACITY as i64).contains(&bodies)
        || !(0..=OBSERVATION_TOTAL_BYTES).contains(&bytes)
    {
        return Err(corrupt_storage());
    }
    reservations_on(tx).await?;
    found_headers_on(tx, hub).await?;
    let recipient_roots:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM hub_stock_operations WHERE kind='correction_recipient')+(SELECT count(*) FROM hub_corrections WHERE family='recipient')").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if recipient_roots != 0 {
        Box::pin(recipient_ledger::load_on(tx, hub)).await?;
    }

    additional_headers_on(tx, hub).await?;
    let sources:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM hub_stock_operations WHERE kind='correction_source')+(SELECT count(*) FROM hub_corrections WHERE family='source')").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if sources != 0 {
        Box::pin(source_ledger::load_on(tx, hub)).await?;
    }
    Ok(())
}

async fn additional_headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid: i64 = sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.family='additional' AND (o.operation_id IS NULL OR o.kind<>'correction_additional' OR o.hub_id<>? OR o.result_id<>c.correction_id)) + ",
        "(SELECT count(*) FROM hub_correction_physical_debits d LEFT JOIN hub_stock_operations o ON o.operation_id=d.operation_id ",
        "LEFT JOIN hub_corrections c ON c.operation_id=d.operation_id WHERE o.operation_id IS NULL OR o.kind<>'correction_additional' OR c.family IS NULL OR c.family<>'additional') + ",
        "(SELECT count(*) FROM hub_correction_captures c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id ",
        "WHERE c.kind='correction_additional' AND (o.operation_id IS NULL OR o.kind<>c.kind OR o.hub_id<>?))"
    )).bind(hub.as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE kind='correction_additional' ORDER BY stock_revision LIMIT 4097")
        .fetch_all(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if invalid != 0 || ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        Box::pin(additional_record::original_on(tx, hub, op)).await?;
    }
    Ok(())
}

async fn found_headers_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<(), SafeError> {
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_corrections correction LEFT JOIN hub_stock_operations original ON original.operation_id=correction.operation_id ",
        "WHERE correction.family='found' AND (original.operation_id IS NULL OR original.kind<>'correction_found' OR original.hub_id<>? OR original.result_id<>correction.correction_id)"
    )).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let ids:Vec<String>=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE kind='correction_found' AND hub_id=? ORDER BY stock_revision LIMIT 4097")
        .bind(hub.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 || ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let original = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        Box::pin(found_record::original_on(tx, hub, original)).await?;
    }
    Ok(())
}

pub(in crate::hub) mod birth;
pub(in crate::hub) mod funding;
pub(in crate::hub) mod initial_review;
mod known;
pub(in crate::hub) use known::current_actual_interpretations_on;
mod observation_payload;
#[cfg(test)]
pub(super) mod projection_native_tests;
#[cfg(test)]
mod projection_tests;
pub(in crate::hub) mod quantity;
pub use known::{HubCorrectionKnownDeparture, HubCorrectionKnownEvidence, HubCorrectionKnownQuery};
mod additional_review;
mod attribution_review;
mod recipient_context;
mod recipient_ledger;
mod recipient_payload;
pub(in crate::hub) mod recipient_write;
pub use recipient_ledger::HubRecipientAttributionReceipt;
mod source_context;
mod source_documents;
mod source_ledger;
mod source_payload;
pub(in crate::hub) mod source_write;
pub use source_ledger::HubSourceAttributionReceipt;
/// Normal material must not silently reuse original source facts after an
/// amendment. Qualified linked-document material will replace this closed gate.
pub(in crate::hub) async fn require_source_document_material_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    traces: &[HubRecordId],
    cut: HubRevision,
) -> Result<(), SafeError> {
    let records = Box::pin(source_records_on(tx, hub, cut)).await?;
    if records.iter().any(|r| traces.contains(&r.original.trace)) {
        return Err(harvestcircle_domain::SafeError::new(
            harvestcircle_domain::SafeErrorCode::HubConflict,
            harvestcircle_domain::SafeMessage::new(
                "Source attribution changed. Review the linked correction before preparing identification or recording a departure.",
            ),
        ));
    }
    Ok(())
}
pub(in crate::hub) async fn source_records_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<Vec<harvestcircle_domain::hub::corrections::SourceAttributionCorrection>, SafeError> {
    let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM hub_corrections WHERE family='source')+(SELECT count(*) FROM hub_stock_operations WHERE kind='correction_source')").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let ledger = Box::pin(source_ledger::load_on(tx, hub)).await?;
    Ok(ledger
        .records
        .into_iter()
        .filter(|c| c.stamp.stock_revision.value() <= cut.value())
        .collect())
}
pub(in crate::hub) async fn source_history_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    trace: HubRecordId,
    cut: HubRevision,
) -> Result<Vec<harvestcircle_domain::hub::corrections::SourceAttributionCorrection>, SafeError> {
    Ok(Box::pin(source_records_on(tx, hub, cut))
        .await?
        .into_iter()
        .filter(|c| c.original.trace == trace)
        .collect())
}

/// Temporary fresh-review fence until exact-cut actual association integration
/// qualifies. Raw report ingress and immutable historical reads stay available.
pub(in crate::hub) async fn require_recipient_actual_adapter_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<(), SafeError> {
    if !Box::pin(recipient_records_on(tx, hub, cut))
        .await?
        .is_empty()
    {
        return Err(harvestcircle_domain::SafeError::new(
            harvestcircle_domain::SafeErrorCode::HubConflict,
            harvestcircle_domain::SafeMessage::new(
                "Recipient attribution changed. Actual-event review needs the linked original and amended associations before it can continue.",
            ),
        ));
    }
    Ok(())
}

pub(in crate::hub) async fn recipient_records_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    cut: HubRevision,
) -> Result<Vec<harvestcircle_domain::hub::corrections::RecipientAttributionCorrection>, SafeError>
{
    let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM hub_corrections WHERE family='recipient')+(SELECT count(*) FROM hub_stock_operations WHERE kind='correction_recipient')").fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if count == 0 {
        return Ok(Vec::new());
    }
    Ok(
        Box::pin(recipient_ledger::load_through_on(tx, hub, Some(cut)))
            .await?
            .records,
    )
}
pub(in crate::hub) async fn capture_recipient_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &harvestcircle_domain::hub::corrections::RecipientAttributionCommand,
) -> Result<harvestcircle_application::hub_submission::HubSubmittedRecipientAttribution, SafeError>
{
    let context = Box::pin(recipient_context::current_on(tx, hub, command, None)).await?;
    let own = crate::hub::actuals::common::operation_id(operation)?;
    command
        .plan(&context.domain(), own, actor, context.latest)
        .map_err(source_context::error)?;
    Ok(context.material(own))
}

pub(in crate::hub) async fn capture_source_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &harvestcircle_domain::hub::corrections::SourceAttributionCommand,
) -> Result<harvestcircle_application::hub_submission::HubSubmittedSourceAttribution, SafeError> {
    let context = Box::pin(source_context::current_on(tx, hub, command, None)).await?;
    let own = crate::hub::actuals::common::operation_id(operation)?;
    command
        .plan(&context.domain(), own, actor, context.latest)
        .map_err(source_context::error)?;
    Ok(context.material(own))
}
#[cfg(test)]
pub(in crate::hub) use additional_review::tests::actual_world as additional_review_fixture;
pub use additional_review::{HubAdditionalQuantityReview, HubAdditionalQuantityReviewOutcome};
pub use attribution_review::{
    HubAttributionReviewOutcome, HubRecipientAttributionReview, HubSourceAttributionReview,
};
mod additional_frozen;
mod additional_payload;
mod additional_record;
pub use additional_record::HubAdditionalQuantityReceipt;
pub(in crate::hub) async fn additional_original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubAdditionalQuantityReceipt, SafeError> {
    Box::pin(additional_record::original_on(tx, hub, op)).await
}
pub(in crate::hub) mod additional_projection;
pub(in crate::hub) mod additional_write;
#[cfg(test)]
mod additional_write_tests;
pub(in crate::hub) async fn capture_additional_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &harvestcircle_domain::hub::corrections::AdditionalQuantityCommand,
) -> Result<harvestcircle_application::hub_submission::HubSubmittedAdditionalQuantity, SafeError> {
    let context = additional_review::load_on(tx, hub, command, None).await?;
    quantity::ensure_capacity_on(tx, hub, context.group.trace).await?;
    let own = crate::hub::actuals::common::operation_id(operation)?;
    command
        .plan(&context.domain(), own, actor, context.latest_recorded_at)
        .map_err(additional_review::error)?;
    Ok(context.material(own))
}
pub(in crate::hub) mod return_allowances;
pub(in crate::hub) mod return_capture;
pub(in crate::hub) mod return_projection;
pub(in crate::hub) mod target_funding;
pub use target_funding::{HubCorrectionFundingEvidence, HubCorrectionFundingQuery};
mod found_context;
mod found_frozen;
mod found_impact;
mod found_record;
mod found_review;
pub use found_record::HubFoundCorrectionReceipt;
pub(in crate::hub) async fn found_original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubFoundCorrectionReceipt, SafeError> {
    found_record::original_on(tx, hub, op).await
}
pub(in crate::hub) mod found_write;
pub(in crate::hub) mod physical_balance;
pub(in crate::hub) async fn capture_found_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
    command: &harvestcircle_domain::hub::corrections::FoundCorrectionCommand,
) -> Result<harvestcircle_application::hub_submission::HubSubmittedFoundCorrection, SafeError> {
    let context = found_context::prepare_on(tx, hub, actor, operation, command, None).await?;
    quantity::ensure_capacity_on(tx, hub, context.funding.quantity.original().trace).await?;
    Ok(context.material(crate::hub::actuals::common::operation_id(operation)?))
}
pub use found_review::{
    HubFoundCorrectionReview, HubFoundCorrectionReviewBlocker, HubFoundCorrectionReviewOutcome,
    HubFoundCorrectionReviewQuery,
};
pub(crate) mod schema;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod upgrade_tests;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubCorrectionObservationReceipt {
    pub operation: DurableRequestId,
    pub original: CorrectionObservationRecord,
}

pub(in crate::hub) fn error(value: CorrectionObservationError) -> SafeError {
    match value {
        CorrectionObservationError::Capacity => stock::capacity(),
        CorrectionObservationError::InvalidIdentity => stock::invalid_input(),
        CorrectionObservationError::InvalidHistory => corrupt_storage(),
    }
}

/// The report retains its raw original; a specialized correction closes only its
/// own exact marker. This header check is not full correction acceptance.
pub(in crate::hub) async fn closes_report_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    report: HubRecordId,
    operation: &DurableRequestId,
) -> Result<bool, SafeError> {
    let count:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_corrections correction JOIN hub_stock_operations current ON current.operation_id=correction.operation_id ",
        "JOIN hub_actual_reports report ON report.report_id=correction.report_id JOIN hub_stock_operations original ON original.operation_id=report.operation_id ",
        "JOIN hub_actual_report_reviews marker ON marker.report_id=report.report_id ",
        "WHERE correction.operation_id=? AND correction.report_id=? AND current.hub_id=? AND current.hub_id=original.hub_id ",
        "AND current.stock_revision>original.stock_revision AND current.result_id=correction.correction_id ",
        "AND marker.operation_id=report.operation_id AND marker.review_operation_id=current.operation_id ",
        "AND ((correction.family='additional' AND current.kind='correction_additional') ",
        "OR (correction.family='count_explain' AND current.kind='correction_count_explain') ",
        "OR (correction.family='count_classify' AND current.kind='correction_count_classify'))"
    )).bind(operation.as_str()).bind(report.as_bytes().as_slice()).bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if !(0..=1).contains(&count) {
        return Err(corrupt_storage());
    }
    Ok(count == 1)
}

impl Database {
    /// Current owner read of an immutable original. The returned record is
    /// descriptive data and does not create an in-memory admission grant.
    pub async fn hub_correction_observation_original(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<HubCorrectionObservationReceipt, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    let op = stock::read::operation_on(tx, hub.id(), &operation)
                        .await?
                        .ok_or_else(stock::invalid_input)?;
                    original_on(tx, hub.id(), op).await
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(in crate::hub) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubCorrectionObservationReceipt, SafeError> {
    if op.kind != "correction_observe" {
        return Err(stock::conflict());
    }
    let row = sqlx::query(concat!(
        "SELECT substr(observation_id,1,17) AS observation_id,substr(previous_operation_id,1,37) AS previous_operation_id,",
        "substr(family,1,33) AS family,substr(target_id,1,17) AS target_id,substr(group_id,1,17) AS group_id,",
        "payload_schema,closure_events,substr(payload,1,262145) AS payload,substr(result_hash,1,33) AS result_hash ",
        "FROM hub_correction_observations WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_| storage_unavailable())?.ok_or_else(corrupt_storage)?;
    if row
        .try_get::<i64, _>("payload_schema")
        .map_err(|_| corrupt_storage())?
        != 1
    {
        return Err(corrupt_storage());
    }
    let bytes: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let command = observation_payload::decode(&bytes)?;
    if row
        .try_get::<i64, _>("closure_events")
        .map_err(|_| corrupt_storage())?
        != i64::from(closure_events(&command).map_err(|_| corrupt_storage())?)
    {
        return Err(corrupt_storage());
    }
    let id = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let fingerprint = command
        .fingerprint(hub, op.actor, id)
        .map_err(|_| corrupt_storage())?;
    let prior = HubRevision::from_value(
        op.revision
            .value()
            .checked_sub(1)
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let original = command
        .preserve(
            hub,
            prior,
            op.catalog_revision,
            id,
            op.actor,
            op.recorded_at,
        )
        .map_err(|_| corrupt_storage())?;
    let previous: Option<String> = row
        .try_get("previous_operation_id")
        .map_err(|_| corrupt_storage())?;
    let previous = previous
        .map(DurableRequestId::parse)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    let previous = previous
        .as_ref()
        .map(crate::hub::actuals::common::operation_id)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    let result: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if original.id != op.result
        || original.id != stock::read::record_id(&row, "observation_id")?
        || command.previous != previous
        || fingerprint != op.hash
        || command.facts.family()
            != row
                .try_get::<String, _>("family")
                .map_err(|_| corrupt_storage())?
        || command.facts.target() != stock::read::record_id(&row, "target_id")?
        || command.facts.group() != stock::read::optional_id(&row, "group_id")?
        || correction_observation_result_digest(&fingerprint, &original)
            .map_err(|_| corrupt_storage())?
            .as_slice()
            != result.as_slice()
    {
        return Err(corrupt_storage());
    }
    crate::hub::submission::verify_recorded_input_on(
        tx,
        hub,
        &op,
        harvestcircle_application::hub_submission::HubSubmittedRequest::CorrectionObservation(
            &command,
        ),
    )
    .await?;
    Ok(HubCorrectionObservationReceipt {
        operation: op.id,
        original,
    })
}

/// Only execute_fresh's owned request enters a new observation. Original replay
/// precedes session/capacity checks and cannot produce another effect.
pub(in crate::hub) async fn observe_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: CorrectionObservationCommand,
    now: UnixTimestamp,
) -> Result<HubCorrectionObservationReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let id = crate::hub::actuals::common::operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, id).map_err(error)?;
    if let Some(prior) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if prior.kind != "correction_observe" || prior.actor != actor || prior.hash != hash {
            return Err(stock::conflict());
        }
        return original_on(tx, hub.id(), prior).await;
    }
    crate::hub::actuals::common::require_admission_on(
        tx,
        hub.id(),
        actor,
        &operation,
        "correction_observe",
        &hash,
        now,
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_correction_observations")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if count < 0 {
        return Err(corrupt_storage());
    }
    if count >= HUB_CORRECTION_OBSERVATION_CAPACITY as i64 {
        return Err(stock::capacity());
    }
    let previous = command.previous.map(operation_from_id).transpose()?;
    let mut prior_events = None;
    if let Some(previous) = &previous {
        let valid: i64 = sqlx::query_scalar(concat!(
            "SELECT count(*) FROM hub_correction_observations p JOIN hub_stock_operations o ON o.operation_id=p.operation_id ",
            "WHERE p.operation_id=? AND p.family=? AND o.hub_id=? ",
            "AND NOT EXISTS(SELECT 1 FROM hub_correction_observations child WHERE child.previous_operation_id=p.operation_id) ",
            "AND NOT EXISTS(SELECT 1 FROM hub_corrections used WHERE used.observation_operation_id=p.operation_id)"
        )).bind(previous.as_str()).bind(command.facts.family()).bind(hub.id().as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
        if valid != 1 {
            return Err(stock::conflict());
        }
        let op = stock::read::operation_on(tx, hub.id(), previous)
            .await?
            .ok_or_else(corrupt_storage)?;
        let prior = original_on(tx, hub.id(), op).await?;
        prior_events =
            Some(closure_events(&prior.original.observation).map_err(|_| corrupt_storage())?);
    }
    let events = closure_events(&command)?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::CorrectionObserve {
            events,
            prior_events,
        },
    )
    .await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let latest: Option<i64> = sqlx::query_scalar(
        "SELECT max(recorded_at_unix_s) FROM hub_stock_operations WHERE hub_id=?",
    )
    .bind(hub.id().as_bytes().as_slice())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if latest.is_some_and(|at| at > now.as_seconds()) {
        return Err(stock::conflict());
    }
    let original = command
        .preserve(hub.id(), state.revision, catalog.revision(), id, actor, now)
        .map_err(error)?;
    let result = correction_observation_result_digest(&hash, &original).map_err(error)?;
    let payload = observation_payload::encode(&command)?;
    let bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(length(payload)),0) FROM hub_correction_observations",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if !(0..=OBSERVATION_TOTAL_BYTES).contains(&bytes) {
        return Err(corrupt_storage());
    }
    if bytes
        .checked_add(i64::try_from(payload.len()).map_err(|_| stock::capacity())?)
        .is_none_or(|n| n > OBSERVATION_TOTAL_BYTES)
    {
        return Err(stock::capacity());
    }
    let op = OperationRecord {
        id: operation.clone(),
        kind: "correction_observe".into(),
        actor,
        hash,
        catalog_revision: original.catalog_revision,
        revision: original.stock_revision,
        result: original.id,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    sqlx::query(concat!(
        "INSERT INTO hub_correction_observations (observation_id,operation_id,previous_operation_id,family,target_id,group_id,payload_schema,closure_events,payload,result_hash) ",
        "VALUES (?,?,?,?,?,?,1,?,?,?)"
    )).bind(original.id.as_bytes().as_slice()).bind(operation.as_str()).bind(previous.as_ref().map(DurableRequestId::as_str))
        .bind(command.facts.family()).bind(command.facts.target().as_bytes().as_slice())
        .bind(command.facts.group().map(|id|id.as_bytes().to_vec())).bind(i64::from(events)).bind(payload).bind(result.as_slice())
        .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    // A journal-only observation creates no stock event and does not change the
    // physical event count. Its matching typed body is already committed above.
    let changed = sqlx::query("UPDATE hub_stock_state SET revision=? WHERE singleton=1 AND hub_id=? AND revision=? AND event_count=?")
        .bind(i64::try_from(original.stock_revision.value()).map_err(|_| corrupt_storage())?).bind(hub.id().as_bytes().as_slice())
        .bind(i64::try_from(state.revision.value()).map_err(|_| corrupt_storage())?).bind(i64::from(state.event_count))
        .execute(&mut *tx).await.map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    original_on(tx, hub.id(), op).await
}

fn operation_from_id(value: HubRecordId) -> Result<DurableRequestId, SafeError> {
    let hex = value.to_hex();
    DurableRequestId::parse(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
    .map_err(|_| stock::invalid_input())
}
