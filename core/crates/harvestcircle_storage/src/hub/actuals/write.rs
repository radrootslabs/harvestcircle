//! Protected actual reconciliation. The executor supplies fresh in-memory admission;
//! matching archived input is never an execution grant.
use super::{common::*, frozen, model::*, reviews};
use crate::{
    db::{corrupt_storage, storage_unavailable},
    hub::{handling, reference, stock},
};
use harvestcircle_application::DurableRequestId;
use harvestcircle_domain::{
    PublicKey, SafeError, UnixTimestamp,
    hub::actuals::{
        ActualNewDepartureCommand, ActualNewDeparturePlan, HistoricalHoldScopeContext,
        actual_departure_projection_digest, actual_departure_result_digest,
    },
};
use radroots_service_sqlite::ServiceSqliteTransaction;
use stock::read::OperationRecord;

pub(in crate::hub) async fn departure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: ActualNewDepartureCommand,
    now: UnixTimestamp,
) -> Result<HubActualReviewReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let id = operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, id);
    if let Some(prior) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if prior.kind != "actual_review" || prior.actor != actor || prior.hash != hash {
            return Err(stock::conflict());
        }
        return reviews::original_on(tx, hub.id(), prior).await;
    }
    require_admission_on(tx, hub.id(), actor, &operation, "actual_review", &hash, now).await?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::ActualReview { physical: true },
    )
    .await?;
    let context = frozen::current_on(tx, hub.id(), &command, Some(&operation)).await?;
    let plan = command
        .plan(&context.domain(), id, actor, now)
        .map_err(error)?;
    let findings = context
        .treatment
        .holds()
        .iter()
        .map(|hold| {
            HistoricalHoldScopeContext {
                lineage: &context.lineage,
                group: command.facts.group,
                occurred: command.facts.occurred,
                births: &context.births,
                now,
            }
            .verify(hold)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(error)?;
    let digest = actual_departure_result_digest(
        &hash,
        &command,
        &context.domain(),
        &context.births,
        &findings,
        &plan,
    )
    .map_err(error)?;
    let physical = plan.physical_projection().map_err(error)?;
    let projection =
        actual_departure_projection_digest(&hash, plan.report.id, &context.source, &physical)
            .map_err(error)?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "actual_review".into(),
        actor,
        hash,
        catalog_revision: context.catalog.revision(),
        revision: plan.stock_revision,
        result: plan.report.id,
        recorded_at: now,
    };
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    capture_departure_on(tx, &operation, &plan, &context, &digest, &projection).await?;
    frozen::persist_on(tx, &operation, &context).await?;
    let before = &plan.reduction.before;
    let changed =
        sqlx::query("UPDATE hub_stock_groups SET revision=? WHERE group_id=? AND revision=?")
            .bind(integer(plan.reduction.after_revision)?)
            .bind(before.id.as_bytes().as_slice())
            .bind(integer(before.revision)?)
            .execute(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    let changed=sqlx::query("UPDATE hub_stock_positions SET packs=?,revision=? WHERE group_id=? AND location_id=? AND packs=?")
        .bind(i64::from(plan.reduction.on_hand_after.value())).bind(integer(plan.reduction.after_revision)?)
        .bind(before.id.as_bytes().as_slice()).bind(before.location.as_bytes().as_slice()).bind(i64::from(before.packs.value()))
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    for allocation in &before.work {
        let changed=sqlx::query("UPDATE hub_group_work SET scoped_packs=? WHERE group_id=? AND work_id=? AND scoped_packs=? AND progress=?")
            .bind(i64::from(plan.reduction.on_hand_after.value())).bind(before.id.as_bytes().as_slice())
            .bind(allocation.requirement.id.as_bytes().as_slice()).bind(i64::from(before.packs.value())).bind(frozen::progress(allocation.progress))
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
            kind: "actual_review",
            prior_events: context.state.event_count,
            trace: before.trace,
            group: before.id,
            packs: plan.reduction.actual_departed,
            from: Some(&context.source.location),
            to: None,
            occurred: command.facts.occurred,
        },
    )
    .await?;
    close_report_on(tx, &operation, plan.report.id).await?;
    handling::write::advance_many_on(tx, hub.id(), &context.state, plan.stock_revision, 1).await?;
    let restored = reviews::original_on(tx, hub.id(), op).await?;
    if restored.original != HubActualReviewOriginal::Departure(Box::new(plan.clone()))
        || restored.lineage.as_ref() != Some(&context.lineage)
        || restored.historical_holds != findings
    {
        return Err(corrupt_storage());
    }
    // Native conservation includes this physical outflow before the transaction
    // can commit. Empty positions stay as retained zero-pack history.
    let loaded = handling::lineage::load_on(
        tx,
        hub.id(),
        before.trace,
        &context.catalog,
        plan.stock_revision,
    )
    .await?;
    handling::lineage::actual_births_on(tx, hub.id(), &loaded).await?;
    let node = loaded
        .rows
        .iter()
        .find(|node| node.id == before.id)
        .ok_or_else(corrupt_storage)?;
    if node.packs != plan.reduction.on_hand_after || node.revision != plan.reduction.after_revision
    {
        return Err(corrupt_storage());
    }
    Ok(restored)
}

pub(super) async fn close_report_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    report: harvestcircle_domain::hub::reference::HubRecordId,
) -> Result<(), SafeError> {
    let changed=sqlx::query("UPDATE hub_actual_report_reviews SET review_operation_id=? WHERE report_id=? AND review_operation_id IS NULL")
        .bind(operation.as_str()).bind(report.as_bytes().as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    Ok(())
}

async fn capture_departure_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    operation: &DurableRequestId,
    plan: &ActualNewDeparturePlan,
    context: &frozen::Context,
    digest: &[u8; 32],
    projection: &[u8; 32],
) -> Result<(), SafeError> {
    let facts = &plan.comparison.facts;
    let scope = &plan.comparison.scope;
    let before = &plan.reduction.before;
    let source_operation = super::common::operation(context.source.operation)?;
    let (kind, start, end) = occurrence_fields(facts.occurred);
    sqlx::query(concat!(
        "INSERT INTO hub_actual_reviews (operation_id,report_id,policy_version,review_kind,trace_id,group_id,variant_id,variant_revision,recipient_id,recipient_revision,packs,",
        "source_operation_id,source_group_id,before_group_revision,after_group_revision,before_packs,before_causal_floor_unix_s,after_causal_floor_unix_s,",
        "historical_location_id,historical_location_revision,projection_hash,current_location_id,current_location_revision,occurrence_kind,occurrence_start_unix_s,occurrence_end_unix_s,",
        "source_evidence,quantity_evidence,recipient_evidence,timing_evidence,verification,lineage_count,scope_count,result_hash) ",
        "VALUES (?,?,1,'departure',?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
    )).bind(operation.as_str()).bind(plan.report.id.as_bytes().as_slice()).bind(before.trace.as_bytes().as_slice()).bind(before.id.as_bytes().as_slice())
        .bind(scope.variant.id.as_bytes().as_slice()).bind(integer(scope.variant.revision)?).bind(scope.recipient.id.as_bytes().as_slice()).bind(integer(scope.recipient.revision)?)
        .bind(i64::from(facts.packs.value())).bind(source_operation.as_str()).bind(context.source.group.id.as_bytes().as_slice())
        .bind(integer(before.revision)?).bind(integer(plan.reduction.after_revision)?).bind(i64::from(before.packs.value()))
        .bind(before.causal_floor.map(|at|at.as_seconds())).bind(plan.after_causal_floor.map(|at|at.as_seconds()))
        .bind(context.source.location.id.as_bytes().as_slice()).bind(integer(context.source.location.revision)?).bind(projection.as_slice())
        .bind(scope.current_location.id.as_bytes().as_slice()).bind(integer(scope.current_location.revision)?)
        .bind(kind).bind(start).bind(end).bind(facts.source_evidence.as_str()).bind(facts.quantity_evidence.as_str()).bind(facts.recipient_evidence.as_str())
        .bind(facts.timing_evidence.as_str()).bind(plan.duplicate_review_basis.as_str())
        .bind(i64::try_from(context.lineage.nodes().len()).map_err(|_|corrupt_storage())?).bind(i64::try_from(before.work.len()).map_err(|_|corrupt_storage())?)
        .bind(digest.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(())
}
