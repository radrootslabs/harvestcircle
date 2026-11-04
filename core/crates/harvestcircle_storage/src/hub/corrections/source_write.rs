//! Fresh native admission appends source meaning; no stock, work or hold effect.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    corrections::SourceAttributionCommand,
    reference::{SourceAttribution, SourceReferenceKind},
};
#[cfg(test)]
mod tests;

pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: SourceAttributionCommand,
    now: UnixTimestamp,
) -> Result<HubSourceAttributionReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let own = crate::hub::actuals::common::operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, own);
    if let Some(op) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if op.kind != "correction_source" || op.actor != actor || op.hash != hash {
            return Err(stock::conflict());
        }
        let ledger = Box::pin(super::source_ledger::load_on(tx, hub.id())).await?;
        let original = ledger
            .records
            .into_iter()
            .find(|r| r.stamp.operation == own)
            .ok_or_else(corrupt_storage)?;
        return Ok(HubSourceAttributionReceipt {
            operation,
            original,
        });
    }
    crate::hub::actuals::common::require_admission_on(
        tx,
        hub.id(),
        actor,
        &operation,
        "correction_source",
        &hash,
        now,
    )
    .await?;
    let context = Box::pin(super::source_context::current_on(
        tx,
        hub.id(),
        &command,
        Some(&operation),
    ))
    .await?;
    let plan = command
        .plan(&context.domain(), own, actor, now)
        .map_err(super::source_context::error)?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::OrdinaryMetadata,
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_corrections")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if !(0..harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS as i64).contains(&count) {
        return Err(stock::capacity());
    }
    let state = stock::read::state_on(tx, hub.id()).await?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "correction_source".into(),
        actor,
        hash,
        catalog_revision: context.catalog.revision(),
        revision: plan.stamp.stock_revision,
        result: own,
        recorded_at: now,
    };
    let admission = crate::hub::submission::admission_capture_digest_on(
        tx,
        hub.id(),
        &op,
        HubSubmittedRequest::SourceAttribution(&command),
    )
    .await?;
    let result = super::source_ledger::digest(&op, &plan, admission)?;
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    let target = crate::hub::actuals::common::operation(context.original.operation)?;
    let prior = command
        .expected_prior
        .map(crate::hub::actuals::common::operation)
        .transpose()?;
    sqlx::query(concat!(
        "INSERT INTO hub_corrections (correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,payload_schema,payload,result_hash) ",
        "VALUES (?,?,'source',?,'population',?,?,1,?,?)"
    )).bind(own.as_bytes().as_slice()).bind(operation.as_str()).bind(target.as_str()).bind(command.trace.as_bytes().as_slice()).bind(prior.as_ref().map(DurableRequestId::as_str)).bind(super::source_payload::encode(&command)?).bind(result.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if let SourceAttribution::Referenced { source, .. } = &plan.after.source {
        let exists: i64 =
            sqlx::query_scalar("SELECT count(*) FROM hub_source_references WHERE source_id=?")
                .bind(source.id.as_bytes().as_slice())
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| storage_unavailable())?;
        if exists == 0 {
            let kind = match source.kind {
                SourceReferenceKind::ProducerBatch => "producer_batch",
                SourceReferenceKind::SupplierReference => "supplier_reference",
                SourceReferenceKind::Other => "other",
            };
            let issuer_revision = plan
                .after
                .issuer
                .as_ref()
                .map(|r| i64::try_from(r.revision.value()).map_err(|_| corrupt_storage()))
                .transpose()?;
            sqlx::query("INSERT INTO hub_source_references (source_id,hub_id,issuer_id,issuer_revision,kind,source_code,provenance,created_operation_id) VALUES (?,?,?,?,?,?,?,?)")
                .bind(source.id.as_bytes().as_slice()).bind(hub.id().as_bytes().as_slice()).bind(source.issuer.as_ref().map(|id|id.as_bytes().as_slice())).bind(issuer_revision).bind(kind).bind(source.code.as_str()).bind(source.provenance.as_str()).bind(operation.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        } else if exists != 1 {
            return Err(corrupt_storage());
        }
    }
    super::found_impact::persist_on(tx, &operation, &plan.impact).await?;
    let changed=sqlx::query("UPDATE hub_stock_state SET revision=? WHERE singleton=1 AND hub_id=? AND revision=? AND event_count=?")
        .bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?).bind(hub.id().as_bytes().as_slice()).bind(i64::try_from(state.revision.value()).map_err(|_|corrupt_storage())?).bind(i64::from(state.event_count)).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    // Reconstruct original context and compare the immutable admission capture
    // before committing any meaning or source-registry row.
    let ledger = Box::pin(super::source_ledger::load_on(tx, hub.id())).await?;
    let original = ledger
        .records
        .into_iter()
        .find(|r| r.stamp.operation == own)
        .ok_or_else(corrupt_storage)?;
    if original != plan {
        return Err(corrupt_storage());
    }
    stock::read::state_on(tx, hub.id()).await?;
    Ok(HubSourceAttributionReceipt {
        operation,
        original,
    })
}
