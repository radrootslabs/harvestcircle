//! Canonical original reads, never a saved Pending-request execution path.
use super::*;
use harvestcircle_application::hub_submission::HubSubmittedRequest;
use harvestcircle_domain::hub::{
    actuals::{HistoricalHoldScopeAssessment, HistoricalHoldScopeContext},
    corrections::{
        AdditionalQuantityCommand, AdditionalQuantityPlan, CorrectedLineFacts, CorrectionImpactSet,
        additional_correction_id,
    },
    reference::{SourceProvenance, WholePacks},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubAdditionalQuantityReceipt {
    pub operation: DurableRequestId,
    pub original: AdditionalQuantityPlan,
    pub historical_holds: Vec<HistoricalHoldScopeAssessment>,
    pub impact: CorrectionImpactSet,
}

/// Only an existing native root row can supply this read-only proof input.
/// It is not reconstructed from a Pending submission and creates no admission.
pub(super) struct NativeInput {
    pub command: AdditionalQuantityCommand,
    pub result_hash: [u8; 32],
}

pub(super) async fn input_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
    op: &OperationRecord,
) -> Result<NativeInput, SafeError> {
    if op.kind != "correction_additional" {
        return Err(stock::conflict());
    }
    let row=sqlx::query(concat!(
        "SELECT substr(correction_id,1,17) AS correction_id,family,subject_kind,substr(subject_id,1,17) AS subject_id,",
        "substr(target_operation_id,1,37) AS target,substr(previous_operation_id,1,37) AS previous,",
        "account_operation_id,account_previous_operation_id,observation_operation_id,substr(report_id,1,17) AS report_id,",
        "payload_schema,substr(payload,1,65537) AS payload,substr(result_hash,1,33) AS result_hash ",
        "FROM hub_corrections WHERE operation_id=?"
    )).bind(op.id.as_str()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
    let raw: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
    let description = super::additional_payload::read(&raw)?;
    let id = super::additional_payload::id;
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let target = DurableRequestId::parse(
        row.try_get::<String, _>("target")
            .map_err(|_| corrupt_storage())?,
    )
    .map_err(|_| corrupt_storage())?;
    let previous: Option<String> = row.try_get("previous").map_err(|_| corrupt_storage())?;
    let previous = previous
        .map(DurableRequestId::parse)
        .transpose()
        .map_err(|_| corrupt_storage())?
        .as_ref()
        .map(crate::hub::actuals::common::operation_id)
        .transpose()
        .map_err(|_| corrupt_storage())?;
    if row
        .try_get::<String, _>("family")
        .map_err(|_| corrupt_storage())?
        != "additional"
        || row
            .try_get::<String, _>("subject_kind")
            .map_err(|_| corrupt_storage())?
            != "line"
        || row
            .try_get::<i64, _>("payload_schema")
            .map_err(|_| corrupt_storage())?
            != 1
        || row
            .try_get::<Option<String>, _>("account_operation_id")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || row
            .try_get::<Option<String>, _>("account_previous_operation_id")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || row
            .try_get::<Option<String>, _>("observation_operation_id")
            .map_err(|_| corrupt_storage())?
            .is_some()
        || stock::read::record_id(&row, "correction_id")? != op.result
        || additional_correction_id(own).map_err(|_| corrupt_storage())? != op.result
        || description.expected_stock_revision.checked_add(1) != Some(op.revision.value())
        || description.expected_catalog_revision != op.catalog_revision.value()
        || id(&description.target_operation)?
            != crate::hub::actuals::common::operation_id(&target).map_err(|_| corrupt_storage())?
        || id(&description.target_group)? != stock::read::record_id(&row, "subject_id")?
        || id(&description.report)? != stock::read::record_id(&row, "report_id")?
        || description.expected_prior.as_deref().map(id).transpose()? != previous
    {
        return Err(corrupt_storage());
    }
    let command = AdditionalQuantityCommand {
        facts: CorrectedLineFacts {
            report: stock::read::record_id(&row, "report_id")?,
            target_operation: crate::hub::actuals::common::operation_id(&target)
                .map_err(|_| corrupt_storage())?,
            target_group: stock::read::record_id(&row, "subject_id")?,
            expected_stock_revision: HubRevision::from_value(description.expected_stock_revision)
                .map_err(|_| corrupt_storage())?,
            expected_catalog_revision: op.catalog_revision,
            expected_prior: previous,
            corrected_line_packs: WholePacks::from_value(description.corrected_line_packs)
                .map_err(|_| corrupt_storage())?,
            recipient: description.recipient.as_deref().map(id).transpose()?,
            occurred: super::additional_payload::occurrence(&description)?,
            source_evidence: SourceProvenance::new(description.source_evidence)
                .map_err(|_| corrupt_storage())?,
            quantity_evidence: SourceProvenance::new(description.quantity_evidence)
                .map_err(|_| corrupt_storage())?,
            recipient_evidence: SourceProvenance::new(description.recipient_evidence)
                .map_err(|_| corrupt_storage())?,
            timing_evidence: SourceProvenance::new(description.timing_evidence)
                .map_err(|_| corrupt_storage())?,
        },
        source_operation: id(&description.source_operation)?,
        source_group: id(&description.source_group)?,
        balancing_group: id(&description.balancing_group)?,
        expected_balancing_revision: HubRevision::from_value(
            description.expected_balancing_revision,
        )
        .map_err(|_| corrupt_storage())?,
        duplicate_review_basis: SourceProvenance::new(description.duplicate_review_basis)
            .map_err(|_| corrupt_storage())?,
    };
    if command.fingerprint(hub, op.actor, own) != op.hash
        || super::additional_payload::encode(&command)? != raw
    {
        return Err(corrupt_storage());
    }
    let result: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
    Ok(NativeInput {
        command,
        result_hash: result.try_into().map_err(|_| corrupt_storage())?,
    })
}

pub(super) struct Seals {
    pub frozen: [u8; 32],
    pub admission: [u8; 32],
    pub physical_base: [u8; 32],
    pub quantity: [u8; 32],
}

pub(super) fn result_digest(
    hub: HubId,
    op: &OperationRecord,
    command: &AdditionalQuantityCommand,
    plan: &AdditionalQuantityPlan,
    findings: &[HistoricalHoldScopeAssessment],
    impact: &CorrectionImpactSet,
    seals: Seals,
) -> Result<[u8; 32], SafeError> {
    #[derive(Serialize)]
    struct Bound<'a> {
        hub: HubId,
        operation: &'a str,
        actor: PublicKey,
        input: [u8; 32],
        result: HubRecordId,
        catalog: HubRevision,
        revision: HubRevision,
        recorded_at: UnixTimestamp,
        frozen: [u8; 32],
        admission: [u8; 32],
        physical_base: [u8; 32],
        quantity: [u8; 32],
        command: &'a AdditionalQuantityCommand,
        plan: &'a AdditionalQuantityPlan,
        findings: &'a [HistoricalHoldScopeAssessment],
        impact: &'a CorrectionImpactSet,
    }
    let bytes = serde_json::to_vec(&Bound {
        hub,
        operation: op.id.as_str(),
        actor: op.actor,
        input: op.hash,
        result: op.result,
        catalog: op.catalog_revision,
        revision: op.revision,
        recorded_at: op.recorded_at,
        frozen: seals.frozen,
        admission: seals.admission,
        physical_base: seals.physical_base,
        quantity: seals.quantity,
        command,
        plan,
        findings,
        impact,
    })
    .map_err(|_| corrupt_storage())?;
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(stock::capacity());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.additional.result.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

impl Database {
    pub async fn hub_additional_quantity_original(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubAdditionalQuantityReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    match stock::read::operation_on(tx, hub.id(), &operation).await? {
                        None => Ok(None),
                        Some(op) => Ok(Some(Box::pin(original_on(tx, hub.id(), op)).await?)),
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
) -> Result<HubAdditionalQuantityReceipt, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(original_with_proofs_on(&mut reader, hub, op)).await
}

pub(super) async fn original_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    op: OperationRecord,
) -> Result<HubAdditionalQuantityReceipt, SafeError> {
    let native = input_on(tx, hub, &op).await?;
    let command = &native.command;
    let (context, frozen, admission) = Box::pin(super::additional_frozen::restore_with_proofs_on(
        tx, hub, &op, command,
    ))
    .await?;
    let own = crate::hub::actuals::common::operation_id(&op.id).map_err(|_| corrupt_storage())?;
    let plan = command
        .plan(&context.domain(), own, op.actor, op.recorded_at)
        .map_err(|_| corrupt_storage())?;
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
                now: op.recorded_at,
            }
            .verify(hold)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| corrupt_storage())?;
    let impact = super::found_impact::for_line_with_proofs_on(
        tx,
        hub,
        context.revision,
        &plan.original_line,
    )
    .await?;
    let projected = Box::pin(super::additional_projection::read_on(tx, hub, &op)).await?;
    if projected.reduction != plan.reduction
        || projected.source != context.source
        || projected.original.target != plan.original_line
        || projected.quantity.amendment != plan.amendment
        || projected.quantity.previous != command.facts.expected_prior
        || projected.quantity.report != Some(plan.report.id)
        || projected.quantity.observation.is_some()
        || projected.frozen != frozen
        || projected.admission != admission
    {
        return Err(corrupt_storage());
    }
    let result = result_digest(
        hub,
        &op,
        command,
        &plan,
        &findings,
        &impact,
        Seals {
            frozen,
            admission,
            physical_base: projected.base_hash,
            quantity: projected.quantity_hash,
        },
    )?;
    if native.result_hash != result || projected.result_hash != result {
        return Err(corrupt_storage());
    }
    super::found_impact::verify_on(tx, &op.id, &impact).await?;
    crate::hub::actuals::reviews::verify_work_changes_on(tx, &op, &plan.reduction.work_changes)
        .await?;
    let extras: i64 = sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_work_actions WHERE operation_id=?)+",
        "(SELECT count(*) FROM hub_action_groups WHERE operation_id=?)+",
        "(SELECT count(*) FROM hub_work_requirements WHERE created_operation_id=?)+",
        "(SELECT count(*) FROM hub_group_identification WHERE created_operation_id=?)"
    ))
    .bind(op.id.as_str())
    .bind(op.id.as_str())
    .bind(op.id.as_str())
    .bind(op.id.as_str())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| storage_unavailable())?;
    if extras != 0 {
        return Err(corrupt_storage());
    }
    // Verify the immutable submission's input as well as its complete material.
    crate::hub::submission::verify_recorded_input_on(
        tx,
        hub,
        &op,
        HubSubmittedRequest::AdditionalQuantity(command),
    )
    .await?;
    Ok(HubAdditionalQuantityReceipt {
        operation: op.id,
        original: plan,
        historical_holds: findings,
        impact,
    })
}
