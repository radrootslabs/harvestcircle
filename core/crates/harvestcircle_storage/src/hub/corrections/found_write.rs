//! One fresh owned admission atomically closes its observed fact, creates new
//! restricted children and preserves all original groups, work and paperwork.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    actuals::{ActualPhysicalBirth, ActualPhysicalBirthKind},
    corrections::{
        FoundCorrectionCommand, FoundStockAllocation, FoundStockPlan, found_correction_id,
    },
    portions::PhysicalRecoveryOrigin,
    stock::Occurrence,
};

pub(in crate::hub) async fn apply_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: FoundCorrectionCommand,
    now: UnixTimestamp,
) -> Result<HubFoundCorrectionReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?;
    reference::validate_basis_on(tx, hub.id()).await?;
    let own = crate::hub::actuals::common::operation_id(&operation)?;
    let hash = command.fingerprint(hub.id(), actor, own);
    if let Some(original) = stock::read::operation_on(tx, hub.id(), &operation).await? {
        if original.kind != "correction_found" || original.actor != actor || original.hash != hash {
            return Err(stock::conflict());
        }
        return super::found_record::original_on(tx, hub.id(), original).await;
    }
    crate::hub::actuals::common::require_admission_on(
        tx,
        hub.id(),
        actor,
        &operation,
        "correction_found",
        &hash,
        now,
    )
    .await?;
    let context = super::found_context::prepare_on(
        tx,
        hub.id(),
        actor,
        &operation,
        &command,
        Some(&operation),
    )
    .await?;
    let plan = context
        .plan(own, actor, now)
        .map_err(super::found_context::error)?;
    let count = u32::try_from(plan.allocations.len()).map_err(|_| corrupt_storage())?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::FoundClosure { events: count },
    )
    .await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    let impact = super::found_impact::load_on(tx, &context).await?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "correction_found".into(),
        actor,
        hash,
        catalog_revision: context.catalog.revision(),
        revision: plan.amendment.stock_revision,
        result: found_correction_id(own).map_err(super::found_context::error)?,
        recorded_at: now,
    };
    let admission = crate::hub::submission::admission_capture_digest_on(
        tx,
        hub.id(),
        &op,
        HubSubmittedRequest::FoundCorrection(&command),
    )
    .await?;
    handling::write::insert_operation_on(tx, hub.id(), &op).await?;
    let frozen = super::found_frozen::persist_on(tx, &context, &op).await?;
    let result = super::found_record::result_digest(
        &op,
        &command,
        &context.observation,
        &plan,
        &impact,
        frozen,
        admission,
    )?;
    let target = crate::hub::actuals::common::operation(plan.original_target.operation)?;
    let previous = context
        .funding
        .quantity
        .amendments()
        .last()
        .map(|a| crate::hub::actuals::common::operation(a.operation))
        .transpose()?;
    sqlx::query(concat!(
        "INSERT INTO hub_corrections (correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,observation_operation_id,payload_schema,payload,result_hash) ",
        "VALUES (?,?,'found',?,'line',?,?,?,1,?,?)"
    )).bind(op.result.as_bytes().as_slice()).bind(operation.as_str()).bind(target.as_str()).bind(plan.original_target.group.as_bytes().as_slice())
        .bind(previous.as_ref().map(DurableRequestId::as_str)).bind(context.observation.operation.as_str()).bind(super::found_record::payload(&command)?).bind(result.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    super::found_impact::persist_on(tx, &operation, &impact).await?;
    let mut birth_seals = Vec::with_capacity(plan.allocations.len());
    for (index, allocation) in plan.allocations.iter().enumerate() {
        let seal = persist_allocation_on(tx, hub.id(), &op, &context, allocation).await?;
        birth_seals.push((allocation.group.id, seal));
        handling::write::event_on(
            tx,
            &operation,
            handling::write::EventEffect {
                kind: "correction_found",
                prior_events: state
                    .event_count
                    .checked_add(u32::try_from(index).map_err(|_| corrupt_storage())?)
                    .ok_or_else(stock::capacity)?,
                trace: allocation.group.trace,
                group: allocation.group.id,
                packs: allocation.group.packs,
                from: None,
                to: Some(&allocation.location),
                occurred: Occurrence::Known(now),
            },
        )
        .await?;
    }
    persist_review_on(tx, hub.id(), &op, &plan, &mut birth_seals).await?;
    let original = super::quantity::original_on(
        tx,
        hub.id(),
        &target,
        plan.original_target.group,
        context.stock_revision,
    )
    .await?;
    let projection = super::quantity::QuantityProjection {
        original,
        amendment: plan.amendment,
        previous: context
            .funding
            .quantity
            .amendments()
            .last()
            .map(|a| a.operation),
        report: None,
        observation: Some(context.observation.original.operation),
    };
    let quantity_hash = super::quantity::seal(&op, hub.id(), &projection)?;
    let delta = plan
        .amendment
        .before
        .checked_sub(plan.amendment.after)
        .map_err(|_| corrupt_storage())?;
    sqlx::query("INSERT INTO hub_correction_quantity_changes (operation_id,target_operation_id,group_id,before_packs,after_packs,delta_kind,delta_packs,projection_hash) VALUES (?,?,?,?,?,'found',?,?)")
        .bind(operation.as_str()).bind(target.as_str()).bind(plan.original_target.group.as_bytes().as_slice()).bind(i64::from(plan.amendment.before.value())).bind(i64::from(plan.amendment.after.value())).bind(i64::from(delta.value())).bind(quantity_hash.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    handling::write::advance_many_on(tx, hub.id(), &state, op.revision, count).await?;
    let restored = Box::pin(super::found_record::original_on(tx, hub.id(), op)).await?;
    if restored.original != plan
        || restored.observation != context.observation
        || restored.impact != impact
    {
        return Err(corrupt_storage());
    }
    // No transaction may commit a new kind before all current physical readers
    // accept its balanced state and fixed negative treatment together.
    stock::read::state_on(tx, hub.id()).await?;
    let lineage = handling::lineage::load_on(
        tx,
        hub.id(),
        plan.original_target.trace,
        &context.catalog,
        plan.amendment.stock_revision,
    )
    .await?;
    if lineage.graph != plan.after_lineage
        || handling::treatment::read::state_on(tx, hub.id(), &lineage.graph).await?
            != plan.after_treatment
    {
        return Err(corrupt_storage());
    }
    let ids = plan
        .allocations
        .iter()
        .map(|a| a.group.id)
        .collect::<Vec<_>>();
    let groups = handling::read::groups_on(tx, hub.id(), &ids, &context.catalog).await?;
    for (allocation, current) in plan.allocations.iter().zip(groups) {
        if current != allocation.group {
            return Err(corrupt_storage());
        }
    }
    Ok(restored)
}

async fn persist_allocation_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    context: &super::found_context::Context,
    allocation: &FoundStockAllocation,
) -> Result<[u8; 32], SafeError> {
    let group = &allocation.group;
    let identity = group.identification.as_ref().ok_or_else(corrupt_storage)?;
    let debit = crate::hub::actuals::common::operation(allocation.credit.debit.operation)?;
    let canonical = super::quantity::original_on(
        tx,
        hub,
        &debit,
        allocation.credit.debit.group,
        context.stock_revision,
    )
    .await?;
    // Quantity projections deliberately bind the bounded known-event seal for
    // actual departures. A physical birth instead retains the full immutable
    // result seal, independently checked by birth::verify_debit_on. The full
    // original has already been verified by the context's native funding read.
    let debit_result_hash = match canonical.kind.as_str() {
        "departure" => canonical.evidence_digest,
        "actual_review" => {
            let bytes: Vec<u8> = sqlx::query_scalar("SELECT substr(result_hash,1,33) FROM hub_actual_reviews WHERE operation_id=? AND group_id=? AND review_kind='departure'")
                .bind(debit.as_str()).bind(allocation.credit.debit.group.as_bytes().as_slice())
                .fetch_one(&mut *tx).await.map_err(|_| storage_unavailable())?;
            bytes.try_into().map_err(|_| corrupt_storage())?
        }
        _ => return Err(corrupt_storage()),
    };
    let data = super::birth::BirthData {
        schema: 1,
        location_revision: allocation.location.revision.value(),
        debit_packs: allocation.funding.packs.value(),
        debit_before_packs: allocation.funding.before.packs.value(),
        debit_before_group_revision: allocation.funding.before.revision.value(),
        debit_after_group_revision: allocation
            .funding
            .before
            .revision
            .value()
            .checked_add(1)
            .ok_or_else(corrupt_storage)?,
        debit_result_hash,
        debit_kind: canonical.kind,
        debit_actor: canonical.actor.to_hex(),
        debit_input_hash: canonical.intent_hash,
        debit_catalog_revision: canonical.catalog_revision.value(),
        debit_result: canonical.result.to_hex(),
        debit_recorded_at: canonical.recorded_at.as_seconds(),
    };
    let projection = super::birth::FoundBirthProjection {
        group: group.clone(),
        location: allocation.location.clone(),
        birth: ActualPhysicalBirth {
            kind: ActualPhysicalBirthKind::FoundRemainder {
                allocation: allocation.credit.allocation,
            },
            operation: allocation.credit.operation,
            hub,
            trace: group.trace,
            variant: group.variant,
            group: group.id,
            parent: group.parent,
            stock_revision: op.revision,
            packs: group.packs,
            actor: op.actor,
            occurred: Occurrence::Known(op.recorded_at),
            recorded_at: op.recorded_at,
        },
        recovery: PhysicalRecoveryOrigin {
            debit_operation: allocation.credit.debit.operation,
            debit_revision: allocation.funding.stock_revision,
        },
        basis: context.command()?.basis,
        cohort_evidence: allocation.cohort_evidence.clone(),
        observation_operation: context.observation.original.operation,
        target_operation: context.funding.quantity.original().operation,
        target_group: context.funding.quantity.original().group,
        allocation: allocation.credit.allocation,
    };
    let seal = super::birth::seal(op, hub, &data, &projection)?;
    sqlx::query("INSERT INTO hub_stock_groups (group_id,trace_id,revision) VALUES (?,?,1)")
        .bind(group.id.as_bytes().as_slice())
        .bind(group.trace.as_bytes().as_slice())
        .execute(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    sqlx::query("INSERT INTO hub_group_identification (group_id,parent_group_id,physical_label,observation,created_operation_id) VALUES (?,?,?,?,?)")
        .bind(group.id.as_bytes().as_slice()).bind(group.parent.map(|id|id.as_bytes().to_vec())).bind(identity.label.as_str()).bind(identity.observation.as_str()).bind(op.id.as_str()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    sqlx::query("INSERT INTO hub_stock_positions (group_id,location_id,location_revision,packs,revision) VALUES (?,?,?,?,1)")
        .bind(group.id.as_bytes().as_slice()).bind(group.location.as_bytes().as_slice()).bind(i64::try_from(allocation.location.revision.value()).map_err(|_|corrupt_storage())?).bind(i64::from(group.packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    sqlx::query("INSERT INTO hub_correction_found_allocations (operation_id,allocation,debit_operation_id,debit_group_id,debit_stock_revision,child_group_id,packs,birth_schema,birth_payload,projection_hash) VALUES (?,?,?,?,?,?,?,1,?,?)")
        .bind(op.id.as_str()).bind(i64::from(allocation.credit.allocation)).bind(debit.as_str()).bind(allocation.credit.debit.group.as_bytes().as_slice()).bind(i64::try_from(allocation.funding.stock_revision.value()).map_err(|_|corrupt_storage())?).bind(group.id.as_bytes().as_slice()).bind(i64::from(group.packs.value())).bind(serde_json::to_vec(&data).map_err(|_|corrupt_storage())?).bind(seal.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(seal)
}

async fn persist_review_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    plan: &FoundStockPlan,
    seals: &mut [(HubRecordId, [u8; 32])],
) -> Result<(), SafeError> {
    seals.sort_by_key(|(group, _)| *group);
    let review = &plan.review_required;
    let hash = super::initial_review::seal(op, hub, review, seals)?;
    sqlx::query("INSERT INTO hub_correction_initial_reviews (operation_id,record_id,trace_id,capture_stock_revision,anchor_count,payload_schema,payload,result_hash) VALUES (?,?,?,?,?,1,?,?)")
        .bind(op.id.as_str()).bind(review.stamp.id.as_bytes().as_slice()).bind(plan.original_target.trace.as_bytes().as_slice()).bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?).bind(i64::try_from(review.scope.anchors().len()).map_err(|_|corrupt_storage())?).bind(serde_json::to_vec(review).map_err(|_|corrupt_storage())?).bind(hash.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    for anchor in review.scope.anchors() {
        sqlx::query("INSERT INTO hub_correction_initial_review_anchors (operation_id,group_id,created_revision,packs) VALUES (?,?,?,?)")
            .bind(op.id.as_str()).bind(anchor.group.as_bytes().as_slice()).bind(i64::try_from(anchor.created_revision.value()).map_err(|_|corrupt_storage())?).bind(i64::from(anchor.captured_packs.value())).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    }
    Ok(())
}
