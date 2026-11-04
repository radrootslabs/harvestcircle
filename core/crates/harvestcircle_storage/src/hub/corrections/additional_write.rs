//! One owned fresh admission closes its preserved report and debits only the
//! explicitly selected current group's delta. No independent departure is minted.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    actuals::HistoricalHoldScopeContext,
    corrections::{AdditionalQuantityCommand, additional_correction_id},
};

pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: AdditionalQuantityCommand,
    now: UnixTimestamp,
) -> Result<super::additional_record::HubAdditionalQuantityReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let own = crate::hub::actuals::common::operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, own);
    if let Some(prior) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if prior.kind != "correction_additional" || prior.actor != actor || prior.hash != hash {
            return Err(stock::conflict());
        }
        return Box::pin(super::additional_record::original_on(tx, hub.id(), prior)).await;
    }
    crate::hub::actuals::common::require_admission_on(
        tx,
        hub.id(),
        actor,
        &operation,
        "correction_additional",
        &hash,
        now,
    )
    .await?;
    let context = Box::pin(super::additional_review::load_on(
        tx,
        hub.id(),
        &command,
        Some(&operation),
    ))
    .await?;
    let plan = command
        .plan(&context.domain(), own, actor, now)
        .map_err(super::additional_review::error)?;
    super::quantity::ensure_capacity_on(tx, hub.id(), plan.original_line.trace).await?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::ActualReview { physical: true },
    )
    .await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    if state.revision != context.revision {
        return Err(stock::conflict());
    }
    let findings = context
        .treatment
        .holds()
        .iter()
        .map(|hold| {
            HistoricalHoldScopeContext {
                lineage: &context.lineage,
                group: context.group.id,
                occurred: command.facts.occurred,
                births: &context.births,
                now,
            }
            .verify(hold)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(crate::hub::actuals::common::error)?;
    let impact =
        super::found_impact::for_line_on(tx, hub.id(), context.revision, &plan.original_line)
            .await?;
    let target = crate::hub::actuals::common::operation(plan.original_line.operation)?;
    let canonical = Box::pin(super::quantity::original_on(
        tx,
        hub.id(),
        &target,
        plan.original_line.group,
        context.revision,
    ))
    .await?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "correction_additional".into(),
        actor,
        hash,
        catalog_revision: context.catalog.revision(),
        revision: plan.amendment.stock_revision,
        result: additional_correction_id(own).map_err(|_| corrupt_storage())?,
        recorded_at: now,
    };
    let admission = crate::hub::submission::admission_capture_digest_on(
        tx,
        hub.id(),
        &op,
        HubSubmittedRequest::AdditionalQuantity(&command),
    )
    .await?;
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    let frozen = super::additional_frozen::persist_on(tx, &context, &op).await?;
    let projection = super::quantity::QuantityProjection {
        original: canonical,
        amendment: plan.amendment,
        previous: command.facts.expected_prior,
        report: Some(plan.report.id),
        observation: None,
    };
    let quantity_hash = super::quantity::seal(&op, hub.id(), &projection)?;
    let physical_base = super::additional_projection::base_digest(
        hub.id(),
        &op,
        &super::additional_projection::Basis {
            command: &command,
            target: &projection.original,
            source: &context.source,
            reduction: &plan.reduction,
            quantity: quantity_hash,
            frozen,
            admission,
        },
    )?;
    let result = super::additional_record::result_digest(
        hub.id(),
        &op,
        &command,
        &plan,
        &findings,
        &impact,
        super::additional_record::Seals {
            frozen,
            admission,
            physical_base,
            quantity: quantity_hash,
        },
    )?;
    let previous = command
        .facts
        .expected_prior
        .map(crate::hub::actuals::common::operation)
        .transpose()?;
    sqlx::query(concat!(
        "INSERT INTO hub_corrections (correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,report_id,payload_schema,payload,result_hash) ",
        "VALUES (?,?,'additional',?,'line',?,?,?,1,?,?)"
    )).bind(op.result.as_bytes().as_slice()).bind(operation.as_str()).bind(target.as_str()).bind(plan.original_line.group.as_bytes().as_slice())
        .bind(previous.as_ref().map(DurableRequestId::as_str)).bind(plan.report.id.as_bytes().as_slice())
        .bind(super::additional_payload::encode(&command)?).bind(result.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    let before = &plan.reduction.before;
    let physical_hash = super::additional_projection::seal(physical_base, result);
    sqlx::query(concat!(
        "INSERT INTO hub_correction_physical_debits (operation_id,target_operation_id,target_group_id,group_id,trace_id,variant_id,",
        "before_group_revision,after_group_revision,before_packs,packs,after_packs,projection_hash) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(operation.as_str()).bind(target.as_str()).bind(plan.original_line.group.as_bytes().as_slice()).bind(before.id.as_bytes().as_slice())
        .bind(before.trace.as_bytes().as_slice()).bind(before.variant.as_bytes().as_slice())
        .bind(i64::try_from(before.revision.value()).map_err(|_|corrupt_storage())?)
        .bind(i64::try_from(plan.reduction.after_revision.value()).map_err(|_|corrupt_storage())?)
        .bind(i64::from(before.packs.value())).bind(i64::from(plan.reduction.actual_departed.value()))
        .bind(i64::from(plan.reduction.on_hand_after.value())).bind(physical_hash.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    sqlx::query(concat!(
        "INSERT INTO hub_correction_quantity_changes (operation_id,target_operation_id,group_id,before_packs,after_packs,delta_kind,delta_packs,projection_hash) ",
        "VALUES (?,?,?,?,?,'additional',?,?)"
    )).bind(operation.as_str()).bind(target.as_str()).bind(plan.original_line.group.as_bytes().as_slice())
        .bind(i64::from(plan.amendment.before.value())).bind(i64::from(plan.amendment.after.value()))
        .bind(i64::from(plan.reduction.actual_departed.value())).bind(quantity_hash.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    super::found_impact::persist_on(tx, &operation, &impact).await?;
    let changed =
        sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
            .bind(
                i64::try_from(plan.reduction.after_revision.value())
                    .map_err(|_| corrupt_storage())?,
            )
            .bind(before.id.as_bytes().as_slice())
            .bind(i64::try_from(before.revision.value()).map_err(|_| corrupt_storage())?)
            .execute(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    let changed=sqlx::query("UPDATE hub_stock_positions SET packs=?,revision=? WHERE group_id=? AND location_id=? AND packs=?")
        .bind(i64::from(plan.reduction.on_hand_after.value())).bind(i64::try_from(plan.reduction.after_revision.value()).map_err(|_|corrupt_storage())?)
        .bind(before.id.as_bytes().as_slice()).bind(before.location.as_bytes().as_slice()).bind(i64::from(before.packs.value()))
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    for allocation in &before.work {
        let changed=sqlx::query("UPDATE hub_group_work SET scoped_packs=? WHERE group_id=? AND work_id=? AND scoped_packs=? AND progress=?")
            .bind(i64::from(plan.reduction.on_hand_after.value())).bind(before.id.as_bytes().as_slice()).bind(allocation.requirement.id.as_bytes().as_slice())
            .bind(i64::from(before.packs.value())).bind(super::additional_frozen::progress(allocation.progress))
            .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if changed.rows_affected() != 1 {
            return Err(stock::conflict());
        }
    }
    for change in &plan.reduction.work_changes {
        handling::write::update_work_on(tx, &change.before, &change.after).await?;
        handling::write::work_change_on(tx, &operation, Some(&change.before), &change.after)
            .await?;
    }
    handling::write::event_on(
        tx,
        &operation,
        handling::write::EventEffect {
            kind: "correction_additional",
            prior_events: state.event_count,
            trace: before.trace,
            group: before.id,
            packs: plan.reduction.actual_departed,
            from: Some(&context.source.location),
            to: None,
            occurred: command.facts.occurred,
        },
    )
    .await?;
    crate::hub::actuals::write::close_report_on(tx, &operation, plan.report.id).await?;
    handling::write::advance_many_on(tx, hub.id(), &state, op.revision, 1).await?;
    let restored = Box::pin(super::additional_record::original_on(tx, hub.id(), op)).await?;
    if restored.original != plan
        || restored.historical_holds != findings
        || restored.impact != impact
    {
        return Err(corrupt_storage());
    }
    // All current readers and complete operation/event audits must qualify this
    // balanced state before commit. An unqualified kind remains transaction-red.
    stock::read::state_on(tx, hub.id()).await?;
    let loaded = Box::pin(handling::lineage::load_on(
        tx,
        hub.id(),
        before.trace,
        &context.catalog,
        plan.amendment.stock_revision,
    ))
    .await?;
    handling::lineage::actual_births_on(tx, hub.id(), &loaded).await?;
    let node = loaded
        .rows
        .iter()
        .find(|n| n.id == before.id)
        .ok_or_else(corrupt_storage)?;
    if node.packs != plan.reduction.on_hand_after || node.revision != plan.reduction.after_revision
    {
        return Err(corrupt_storage());
    }
    if plan.reduction.on_hand_after.value() > 0 {
        let current = Box::pin(handling::read::group_on(
            tx,
            hub.id(),
            before.id,
            &context.catalog,
        ))
        .await?;
        let mut expected = before.clone();
        expected.packs = plan.reduction.on_hand_after;
        expected.revision = plan.reduction.after_revision;
        expected.causal_floor = plan.after_causal_floor;
        for allocation in &mut expected.work {
            if let Some(change) = plan
                .reduction
                .work_changes
                .iter()
                .find(|c| c.before.id == allocation.requirement.id)
            {
                allocation.requirement = change.after.clone();
            }
        }
        if current != expected {
            return Err(corrupt_storage());
        }
    }
    Ok(restored)
}
