//! Owned fresh count metadata effects. No new physical debit, credit, work
//! transition or saved-request execution is created by these classifications.
use super::*;
#[cfg(test)]
mod tests;
use harvestcircle_domain::hub::corrections::{
    CountClassificationCommand, CountExplanationCommand, count_classification_id,
    count_explanation_id,
};

pub(in crate::hub) async fn explain_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: CountExplanationCommand,
    now: UnixTimestamp,
) -> Result<HubCountExplanationReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?.id();
    reference::validate_basis_on(tx, hub).await?;
    super::count_mode::epoch_on(tx, hub).await?;
    let own = crate::hub::actuals::common::operation_id(&operation)?;
    let hash = command.fingerprint(hub, actor, own);
    if let Some(op) = stock::read::operation_on(tx, hub, &operation).await? {
        if op.kind != "correction_count_explain" || op.actor != actor || op.hash != hash {
            return Err(stock::conflict());
        }
        let mut reader = OriginalReader::new(tx);
        return Ok(Box::pin(reader.count_explanation(hub, op))
            .await?
            .as_ref()
            .clone());
    }
    crate::hub::actuals::common::require_admission_on(
        tx,
        hub,
        actor,
        &operation,
        "correction_count_explain",
        &hash,
        now,
    )
    .await?;
    let context = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(super::count_context::current_with_pending_on(
            &mut reader,
            hub,
            &command,
            Some(&operation),
        ))
        .await?
    };
    let plan = command
        .plan(&context.domain(), own, actor, now)
        .map_err(super::additional_review::error)?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::ActualReview { physical: false },
    )
    .await?;
    ensure_rows_on(tx).await?;
    // Complete population inventories include normal and actual departures as
    // well as every positive original count explanation, including later zero.
    if context.facts.known.len() >= harvestcircle_domain::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(stock::capacity());
    }
    let explanations:i64=sqlx::query_scalar("SELECT count(*) FROM hub_stock_operations WHERE hub_id=? AND kind='correction_count_explain'")
        .bind(hub.as_bytes().as_slice()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if explanations < 0 {
        return Err(corrupt_storage());
    }
    if explanations >= harvestcircle_domain::hub::actuals::HUB_ACTUAL_REPORT_CAPACITY as i64 {
        return Err(stock::capacity());
    }
    let state = stock::read::state_on(tx, hub).await?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "correction_count_explain".into(),
        actor,
        hash,
        catalog_revision: context.facts.catalog.revision(),
        revision: plan.explanation.stock_revision,
        result: count_explanation_id(own).map_err(|_| corrupt_storage())?,
        recorded_at: now,
    };
    let admission = crate::hub::submission::admission_capture_digest_on(
        tx,
        hub,
        &op,
        harvestcircle_application::hub_submission::HubSubmittedRequest::CountExplanation(&command),
    )
    .await?;
    let result_hash = super::count_record::result_digest(hub, &op, &command, &plan, admission)?;
    let target = crate::hub::actuals::common::operation(plan.count.operation)?;
    let previous = context
        .facts
        .account
        .explanations()
        .last()
        .map(|row| row.operation);
    let root = super::count_account::Root {
        op,
        command: super::count_payload::Command::Explanation(command.clone()),
        target: target.clone(),
        account: target,
        previous,
        account_previous: command.facts.expected_prior,
        report: Some(plan.report.id),
        observation: None,
        subject: plan.count.id,
        result_hash,
        admission,
    };
    insert_root_on(tx, hub, &root, "count_explain", "count").await?;
    let seal = super::count_account::projection_seal(hub, &root, &plan.count, &plan.explanation)?;
    sqlx::query("INSERT INTO hub_correction_count_explanations (operation_id,count_operation_id,reconciliation_id,packs,projection_hash) VALUES (?,?,?,?,?)")
        .bind(operation.as_str()).bind(root.account.as_str()).bind(plan.count.id.as_bytes().as_slice()).bind(i64::from(plan.explanation.packs.value())).bind(seal.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    crate::hub::actuals::write::close_report_on(tx, &operation, plan.report.id).await?;
    advance_metadata_on(tx, hub, &state, &root.op).await?;
    let restored = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(reader.count_explanation(hub, root.op)).await?
    };
    if restored.original != plan {
        return Err(corrupt_storage());
    }
    stock::read::state_on(tx, hub).await?;
    Ok(restored.as_ref().clone())
}

pub(in crate::hub) async fn classify_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    operation: DurableRequestId,
    command: CountClassificationCommand,
    now: UnixTimestamp,
) -> Result<HubCountClassificationReceipt, SafeError> {
    let hub = reference::authorize_on(tx, actor).await?.id();
    reference::validate_basis_on(tx, hub).await?;
    super::count_mode::epoch_on(tx, hub).await?;
    let own = crate::hub::actuals::common::operation_id(&operation)?;
    let hash = command.fingerprint(hub, actor, own);
    if let Some(op) = stock::read::operation_on(tx, hub, &operation).await? {
        if op.kind != "correction_count_classify" || op.actor != actor || op.hash != hash {
            return Err(stock::conflict());
        }
        let mut reader = OriginalReader::new(tx);
        return Ok(Box::pin(reader.count_classification(hub, op))
            .await?
            .as_ref()
            .clone());
    }
    crate::hub::actuals::common::require_admission_on(
        tx,
        hub,
        actor,
        &operation,
        "correction_count_classify",
        &hash,
        now,
    )
    .await?;
    let context = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(super::count_classification::current_on(
            &mut reader,
            hub,
            &command,
            Some(&operation),
        ))
        .await?
    };
    let plan = command
        .plan(&context.domain(), own, actor, now)
        .map_err(super::additional_review::error)?;
    handling::treatment::capacity::ensure_on(
        tx,
        actor,
        &operation,
        handling::treatment::capacity::Reservation::CountClassification {
            report: plan.report.is_some(),
        },
    )
    .await?;
    ensure_rows_on(tx).await?;
    let state = stock::read::state_on(tx, hub).await?;
    let op = OperationRecord {
        id: operation.clone(),
        kind: "correction_count_classify".into(),
        actor,
        hash,
        catalog_revision: context.facts.catalog.revision(),
        revision: plan.amendment.stock_revision,
        result: count_classification_id(own).map_err(|_| corrupt_storage())?,
        recorded_at: now,
    };
    let admission = crate::hub::submission::admission_capture_digest_on(
        tx,
        hub,
        &op,
        harvestcircle_application::hub_submission::HubSubmittedRequest::CountClassification(
            &command,
        ),
    )
    .await?;
    let result_hash =
        super::count_class_record::result_digest(hub, &op, &command, &plan, admission)?;
    let target = crate::hub::actuals::common::operation(plan.amendment.explanation)?;
    let root = super::count_account::Root {
        op,
        command: super::count_payload::Command::Classification(command.clone()),
        target,
        account: crate::hub::actuals::common::operation(plan.count.operation)?,
        previous: command.expected_prior,
        account_previous: command.expected_account_prior,
        report: plan.report.as_ref().map(|r| r.id),
        observation: Some(command.observation_operation),
        subject: plan.amendment.explanation,
        result_hash,
        admission,
    };
    insert_root_on(tx, hub, &root, "count_classify", "classification").await?;
    let seal = super::count_account::projection_seal(hub, &root, &plan.count, &plan.amendment)?;
    sqlx::query("INSERT INTO hub_correction_quantity_changes (operation_id,target_operation_id,group_id,before_packs,after_packs,delta_kind,delta_packs,projection_hash) VALUES (?,?,?,?,?,'classification',?,?)")
        .bind(operation.as_str()).bind(root.target.as_str()).bind(plan.count.before.id.as_bytes().as_slice())
        .bind(i64::from(plan.amendment.before.value())).bind(i64::from(plan.amendment.after.value()))
        .bind(i64::from(plan.amendment.before.value().abs_diff(plan.amendment.after.value()))).bind(seal.as_slice())
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if let Some(report) = &plan.report {
        crate::hub::actuals::write::close_report_on(tx, &operation, report.id).await?;
    }
    advance_metadata_on(tx, hub, &state, &root.op).await?;
    let restored = {
        let mut reader = OriginalReader::new(tx);
        Box::pin(reader.count_classification(hub, root.op)).await?
    };
    if restored.original != plan {
        return Err(corrupt_storage());
    }
    stock::read::state_on(tx, hub).await?;
    Ok(restored.as_ref().clone())
}

async fn advance_metadata_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    state: &stock::read::StockState,
    op: &OperationRecord,
) -> Result<(), SafeError> {
    if !matches!(
        op.kind.as_str(),
        "correction_count_explain" | "correction_count_classify"
    ) || state.revision.value().checked_add(1) != Some(op.revision.value())
    {
        return Err(corrupt_storage());
    }
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_stock_events WHERE operation_id=?")
            .bind(op.id.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if events != 0 {
        return Err(corrupt_storage());
    }
    let changed=sqlx::query("UPDATE hub_stock_state SET revision=? WHERE singleton=1 AND hub_id=? AND revision=? AND event_count=?")
        .bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?).bind(hub.as_bytes().as_slice())
        .bind(i64::try_from(state.revision.value()).map_err(|_|corrupt_storage())?).bind(i64::from(state.event_count))
        .execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if changed.rows_affected() != 1 {
        return Err(stock::conflict());
    }
    Ok(())
}

async fn ensure_rows_on(tx: &mut ServiceSqliteTransaction<'_>) -> Result<(), SafeError> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM hub_corrections")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| storage_unavailable())?;
    if count < 0 {
        return Err(corrupt_storage());
    }
    if count >= harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS as i64 {
        return Err(stock::capacity());
    }
    Ok(())
}
async fn insert_root_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    root: &super::count_account::Root,
    family: &str,
    subject: &str,
) -> Result<(), SafeError> {
    handling::write::insert_operation_on(tx, hub, &root.op).await?;
    super::count_mode::persist_on(tx, hub, &root.op).await?;
    let previous = root
        .previous
        .map(crate::hub::actuals::common::operation)
        .transpose()?;
    let account_previous = root
        .account_previous
        .map(crate::hub::actuals::common::operation)
        .transpose()?;
    let observation = root
        .observation
        .map(crate::hub::actuals::common::operation)
        .transpose()?;
    sqlx::query("INSERT INTO hub_corrections (correction_id,operation_id,family,target_operation_id,subject_kind,subject_id,previous_operation_id,account_operation_id,account_previous_operation_id,report_id,observation_operation_id,payload_schema,payload,result_hash) VALUES (?,?,?,?,?,?,?,?,?,?,?,1,?,?)")
        .bind(root.op.result.as_bytes().as_slice()).bind(root.op.id.as_str()).bind(family).bind(root.target.as_str()).bind(subject).bind(root.subject.as_bytes().as_slice())
        .bind(previous.as_ref().map(DurableRequestId::as_str)).bind(root.account.as_str()).bind(account_previous.as_ref().map(DurableRequestId::as_str))
        .bind(root.report.as_ref().map(|id|id.as_bytes().as_slice())).bind(observation.as_ref().map(DurableRequestId::as_str))
        .bind(super::count_payload::encode(&root.command)?).bind(root.result_hash.as_slice()).execute(&mut *tx).await.map_err(|_|storage_unavailable())?;
    Ok(())
}
