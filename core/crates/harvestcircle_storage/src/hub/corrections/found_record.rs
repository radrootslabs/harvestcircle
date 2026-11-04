//! Full immutable found originals. Independently sealed shallow projections are
//! used by physical readers; those readers never recursively call this replanner.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::corrections::{
    CorrectionImpactSet, FoundCorrectionCommand, FoundStockPlan, found_correction_id,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubFoundCorrectionReceipt {
    pub operation: DurableRequestId,
    pub observation: HubCorrectionObservationReceipt,
    pub original: FoundStockPlan,
    /// Outstanding manual work by default, never external acknowledgement.
    pub impact: CorrectionImpactSet,
}

pub(super) fn payload(command: &FoundCorrectionCommand) -> Result<Vec<u8>, SafeError> {
    #[derive(Serialize)]
    struct Data<'a> {
        schema: u8,
        command: &'a FoundCorrectionCommand,
    }
    serde_json::to_vec(&Data { schema: 1, command }).map_err(|_| corrupt_storage())
}

pub(super) fn result_digest(
    op: &OperationRecord,
    command: &FoundCorrectionCommand,
    observation: &HubCorrectionObservationReceipt,
    plan: &FoundStockPlan,
    impact: &CorrectionImpactSet,
    frozen: [u8; 32],
    admission: [u8; 32],
) -> Result<[u8; 32], SafeError> {
    #[derive(Serialize)]
    struct Bound<'a> {
        operation: &'a str,
        input: [u8; 32],
        frozen: [u8; 32],
        admission: [u8; 32],
        command: &'a FoundCorrectionCommand,
        observation: &'a harvestcircle_domain::hub::corrections::CorrectionObservationRecord,
        plan: &'a FoundStockPlan,
        impact: &'a CorrectionImpactSet,
    }
    let bytes = serde_json::to_vec(&Bound {
        operation: op.id.as_str(),
        input: op.hash,
        frozen,
        admission,
        command,
        observation: &observation.original,
        plan,
        impact,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(stock::capacity());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.found.result.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

impl Database {
    pub async fn hub_found_correction_original(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubFoundCorrectionReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        None => Ok(None),
                        Some(op) => Ok(Some(original_on(tx, hub.id(), op).await?)),
                    }
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}

pub(super) async fn original_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubFoundCorrectionReceipt, SafeError> {
    if op.kind != "correction_found" {
        return Err(stock::conflict());
    }
    let row=sqlx::query(concat!(
        "SELECT substr(correction_id,1,17) AS correction_id,substr(family,1,33) AS family,substr(subject_kind,1,33) AS subject_kind,substr(subject_id,1,17) AS subject_id,",
        "substr(target_operation_id,1,37) AS target,substr(previous_operation_id,1,37) AS previous,substr(observation_operation_id,1,37) AS observation,",
        "account_operation_id,account_previous_operation_id,report_id,payload_schema,substr(payload,1,1025) AS payload,substr(result_hash,1,33) AS result_hash ",
        "FROM hub_corrections WHERE operation_id=? LIMIT 1"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let observed = DurableRequestId::parse(
        row.try_get::<String, _>("observation")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let observed = stock::read::operation_on(tx, hub, &observed)
        .await?
        .ok_or_else(corrupt_storage)?;
    let observation = super::original_on(tx, hub, observed).await?;
    let (context, frozen, admission) =
        Box::pin(super::found_frozen::restore_on(tx, hub, &op, observation)).await?;
    let command = FoundCorrectionCommand {
        observation_operation: context.observation.original.operation,
        expected_stock_revision: context.stock_revision,
        expected_catalog_revision: context.catalog.revision(),
    };
    let id = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let target =
        crate::hub::actuals::common::operation(context.funding.quantity.original().operation)
            .map_err(|_| corrupt_storage())?;
    let previous: Option<String> = row.try_get("previous").map_err(|_| corrupt_storage())?;
    let previous = previous
        .map(DurableRequestId::parse)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    let previous = previous
        .as_ref()
        .map(crate::hub::actuals::common::operation_id)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    if op.result != found_correction_id(id).map_err(|_| corrupt_storage())?
        || stock::read::record_id(&row, "correction_id")? != op.result
        || command.fingerprint(hub, op.actor, id) != op.hash
        || row
            .try_get::<String, _>("family")
            .map_err(|_| corrupt_storage())?
            != "found"
        || row
            .try_get::<String, _>("subject_kind")
            .map_err(|_| corrupt_storage())?
            != "line"
        || stock::read::record_id(&row, "subject_id")? != context.funding.quantity.original().group
        || row
            .try_get::<String, _>("target")
            .map_err(|_| corrupt_storage())?
            != target.as_str()
        || previous
            != context
                .funding
                .quantity
                .amendments()
                .last()
                .map(|a| a.operation)
        || row
            .try_get::<Option<String>, _>("account_operation_id")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || row
            .try_get::<Option<String>, _>("account_previous_operation_id")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || row
            .try_get::<Option<Vec<u8>>, _>("report_id")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || row
            .try_get::<i64, _>("payload_schema")
            .map_err(|_| corrupt_storage())?
            != 1
        || row
            .try_get::<Vec<u8>, _>("payload")
            .map_err(|_| corrupt_storage())?
            != payload(&command)?
    {
        return Err(corrupt_storage());
    }
    let plan = context
        .plan(id, op.actor, op.recorded_at)
        .map_err(|_| corrupt_storage())?;
    let impact = super::found_impact::load_on(tx, &context).await?;
    let stored: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    if result_digest(
        &op,
        &command,
        &context.observation,
        &plan,
        &impact,
        frozen,
        admission,
    )?
    .as_slice()
        != stored.as_slice()
    {
        return Err(corrupt_storage());
    }
    super::found_impact::verify_on(tx, &op.id, &impact).await?;
    verify_outputs_on(tx, hub, &op, &context, &plan).await?;
    crate::hub::submission::verify_recorded_input_on(
        tx,
        hub,
        &op,
        HubSubmittedRequest::FoundCorrection(&command),
    )
    .await?;
    Ok(HubFoundCorrectionReceipt {
        operation: op.id,
        observation: context.observation,
        original: plan,
        impact,
    })
}

async fn verify_outputs_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
    context: &super::found_context::Context,
    plan: &FoundStockPlan,
) -> Result<(), SafeError> {
    let work: i64 = sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_work_changes WHERE operation_id=?) + ",
        "(SELECT count(*) FROM hub_work_actions WHERE operation_id=?) + ",
        "(SELECT count(*) FROM hub_action_groups WHERE operation_id=?) + ",
        "(SELECT count(*) FROM hub_work_requirements WHERE created_operation_id=?)"
    ))
    .bind(op.id.as_str())
    .bind(op.id.as_str())
    .bind(op.id.as_str())
    .bind(op.id.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if work != 0 {
        return Err(corrupt_storage());
    }
    let inherited:i64=sqlx::query_scalar(concat!(
        "SELECT count(*) FROM hub_group_work assigned JOIN hub_correction_found_allocations child ON child.child_group_id=assigned.group_id ",
        "JOIN hub_work_requirements requirement ON requirement.work_id=assigned.work_id JOIN hub_stock_operations original ON original.operation_id=requirement.created_operation_id ",
        "WHERE child.operation_id=? AND original.stock_revision<=?"
    )).bind(op.id.as_str()).bind(i64::try_from(op.revision.value()).map_err(|_|corrupt_storage())?).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
    if inherited != 0 {
        return Err(corrupt_storage());
    }
    let debits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hub_correction_physical_debits WHERE operation_id=?",
    )
    .bind(op.id.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| storage_unavailable())?;
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hub_stock_events WHERE operation_id=?")
            .bind(op.id.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| storage_unavailable())?;
    if debits != 0
        || events != i64::try_from(plan.allocations.len()).map_err(|_| corrupt_storage())?
    {
        return Err(corrupt_storage());
    }
    for allocation in &plan.allocations {
        let projection = super::birth::birth_on(tx, hub, op, allocation.group.id).await?;
        if projection.group != allocation.group
            || projection.location != allocation.location
            || projection.observation_operation != context.observation.original.operation
            || projection.target_operation != plan.original_target.operation
            || projection.target_group != plan.original_target.group
            || projection.basis != plan.basis
            || projection.cohort_evidence != allocation.cohort_evidence
            || projection.allocation != allocation.credit.allocation
            || projection.recovery.debit_operation != allocation.credit.debit.operation
            || projection.recovery.debit_revision != allocation.funding.stock_revision
        {
            return Err(corrupt_storage());
        }
        let exact:i64=sqlx::query_scalar(concat!(
            "SELECT count(*) FROM hub_stock_events WHERE operation_id=? AND kind='correction_found' AND trace_id=? AND group_id=? AND packs=? ",
            "AND from_location_id IS NULL AND from_location_revision IS NULL AND to_location_id=? AND to_location_revision=? AND occurrence_kind='known' AND occurrence_start_unix_s=? AND occurrence_end_unix_s=?"
        )).bind(op.id.as_str()).bind(allocation.group.trace.as_bytes().as_slice()).bind(allocation.group.id.as_bytes().as_slice()).bind(i64::from(allocation.group.packs.value()))
            .bind(allocation.location.id.as_bytes().as_slice()).bind(i64::try_from(allocation.location.revision.value()).map_err(|_|corrupt_storage())?).bind(op.recorded_at.as_seconds()).bind(op.recorded_at.as_seconds()).fetch_one(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if exact != 1 {
            return Err(corrupt_storage());
        }
    }
    let initial = super::initial_review::original_on(tx, hub, &plan.after_lineage, op).await?;
    if initial != plan.review_required {
        return Err(corrupt_storage());
    }
    let target = crate::hub::actuals::common::operation(plan.original_target.operation)
        .map_err(|_| corrupt_storage())?;
    let history =
        super::quantity::history_on(tx, hub, &target, plan.original_target.group, op.revision)
            .await?;
    if history.amendments.len() != context.funding.quantity.amendments().len() + 1
        || history.amendments.last() != Some(&plan.amendment)
    {
        return Err(corrupt_storage());
    }
    Ok(())
}
