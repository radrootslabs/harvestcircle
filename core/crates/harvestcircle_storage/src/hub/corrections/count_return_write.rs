//! Fresh native association changes only interpretation metadata. Existing
//! Return facts, physical events, work and treatment remain untouched.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    corrections::CountReturnAssociationCommand, stock::HUB_TRACE_CAPACITY,
};

#[cfg(test)]
pub(in crate::hub) mod tests;

pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: CountReturnAssociationCommand,
    now: UnixTimestamp,
) -> Result<HubCountReturnAssociationReceipt, SafeError> {
    // Every heavy child future stays heap-bounded: this write's own frame must
    // stay small because it polls the post-write restore verification, whose
    // recursive original cascade runs at this call boundary's deepest stack.
    let hub = Box::pin(reference::authorize_on(tx, actor)).await?.id();
    Box::pin(reference::validate_basis_on(tx, hub)).await?;
    let epoch = Box::pin(super::count_return_ledger::epoch_on(tx, hub)).await?;
    let own = crate::hub::actuals::common::operation_id(&operation)
        .map_err(|_| stock::invalid_input())?;
    let hash = command.fingerprint(hub, actor, own);
    if let Some(op) = Box::pin(stock::read::operation_on(tx, hub, &operation)).await? {
        if op.kind != "count_return_associate" || op.actor != actor || op.hash != hash {
            return Err(stock::conflict());
        }
        let mut reader = OriginalReader::new(tx);
        return Ok(Box::pin(reader.count_return_association(hub, op))
            .await?
            .as_ref()
            .clone());
    }
    Box::pin(crate::hub::actuals::common::require_admission_on(
        tx,
        hub,
        actor,
        &operation,
        "count_return_associate",
        &hash,
        now,
    ))
    .await?;
    Box::pin(super::row_budget::ensure_hub_on(tx)).await?;
    let context = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(super::count_return_context::current_on(
            &mut reader,
            hub,
            &command,
            Some(&operation),
        ))
        .await?
    };
    let plan = command
        .plan(&context.domain()?, own, actor, now)
        .map_err(super::count_return_context::error)?;
    Box::pin(handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::OrdinaryMetadata,
    ))
    .await?;
    let count: i64 = Box::pin(
        sqlx::query_scalar("SELECT count(*) FROM hub_count_return_associations")
            .fetch_one(&mut *tx),
    )
    .await
    .map_err(|_| storage_unavailable())?;
    if count < 0 {
        return Err(corrupt_storage());
    }
    if count >= HUB_TRACE_CAPACITY as i64 {
        return Err(stock::capacity());
    }
    Box::pin(super::row_budget::ensure_on(
        tx,
        hub,
        plan.association.explanation_trace,
    ))
    .await?;
    let state = Box::pin(stock::read::state_on(tx, hub)).await?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "count_return_associate".into(),
        actor,
        hash,
        catalog_revision: context.facts.catalog.revision(),
        revision: plan.association.stock_revision,
        result: plan.association.id,
        recorded_at: now,
    };
    let admission = Box::pin(crate::hub::submission::admission_capture_digest_on(
        tx,
        hub,
        &op,
        HubSubmittedRequest::CountReturnAssociation(&command),
    ))
    .await?;
    let result = super::count_return_record::result_digest(hub, &op, &command, &plan, admission)?;
    let mut root = super::count_return_ledger::Root {
        op,
        command,
        count: crate::hub::actuals::common::operation(plan.association.count_operation)
            .map_err(|_| corrupt_storage())?,
        packs: plan.association.packs.value(),
        epoch,
        capture: admission,
        result,
        projection: super::count_return_ledger::projection_bytes(&plan.association)?,
        projection_hash: [0; 32],
    };
    root.projection_hash = super::count_return_ledger::projection_seal(hub, &root)?;
    let returned = crate::hub::actuals::common::operation(root.command.return_operation)
        .map_err(|_| corrupt_storage())?;
    let explained = crate::hub::actuals::common::operation(root.command.explanation_operation)
        .map_err(|_| corrupt_storage())?;
    let account_previous = crate::hub::actuals::common::operation(
        root.command
            .expected_account_prior
            .ok_or_else(corrupt_storage)?,
    )
    .map_err(|_| corrupt_storage())?;
    let previous = root
        .command
        .expected_returns_prior
        .map(crate::hub::actuals::common::operation)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    Box::pin(handling::write::insert_operation_on(tx, hub, &root.op)).await?;
    Box::pin(sqlx::query(concat!(
        "INSERT INTO hub_count_return_associations (operation_id,association_id,hub_id,epoch,legacy_stock_revision,legacy_catalog_revision,stock_revision,catalog_revision,",
        "return_operation_id,explanation_operation_id,count_operation_id,account_previous_operation_id,previous_operation_id,packs,payload_schema,payload,projection_schema,projection,projection_hash,result_hash,capture_hash) ",
        "VALUES (?,?,?,1,?,?,?,?,?,?,?,?,?,?,1,?,1,?,?,?,?)"
    )).bind(root.op.id.as_str()).bind(root.op.result.as_bytes().as_slice()).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(epoch.stock).map_err(|_| corrupt_storage())?).bind(i64::try_from(epoch.catalog).map_err(|_| corrupt_storage())?)
        .bind(i64::try_from(root.op.revision.value()).map_err(|_| corrupt_storage())?).bind(i64::try_from(root.op.catalog_revision.value()).map_err(|_| corrupt_storage())?)
        .bind(returned.as_str()).bind(explained.as_str()).bind(root.count.as_str()).bind(account_previous.as_str()).bind(previous.as_ref().map(DurableRequestId::as_str))
        .bind(i64::from(root.packs)).bind(super::count_return_payload::encode(&root.command)?).bind(&root.projection)
        .bind(root.projection_hash.as_slice()).bind(root.result.as_slice()).bind(root.capture.as_slice())
        .execute(&mut *tx)).await.map_err(|_| storage_unavailable())?;
    Box::pin(super::count_write::advance_metadata_on(
        tx, hub, &state, &root.op,
    ))
    .await?;
    let restored = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(reader.count_return_association(hub, root.op)).await?
    };
    if restored.original != plan {
        return Err(corrupt_storage());
    }
    Box::pin(stock::read::state_on(tx, hub)).await?;
    Ok(restored.as_ref().clone())
}
