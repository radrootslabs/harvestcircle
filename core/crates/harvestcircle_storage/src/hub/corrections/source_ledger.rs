//! Complete ordered source originals reconstructed from canonical native cuts.
//! Root payloads and admission archives are evidence, never execution grants.
use super::*;
use harvestcircle_application::hub_submission::{HubSubmittedMaterial, HubSubmittedRequest};
use harvestcircle_domain::hub::corrections::SourceAttributionCorrection;
use sha2::{Digest, Sha256};

pub(super) struct Ledger {
    pub populations: Vec<super::attribution_review::registry::Population>,
    pub records: Vec<SourceAttributionCorrection>,
    pub issued_versions:
        Vec<harvestcircle_domain::hub::corrections::AttributionAffectedIssuedVersion>,
}

pub(super) fn digest(
    op: &OperationRecord,
    plan: &SourceAttributionCorrection,
    admission: [u8; 32],
) -> Result<[u8; 32], SafeError> {
    let bytes = serde_json::to_vec(&(
        op.id.as_str(),
        op.actor,
        op.hash,
        op.revision,
        op.catalog_revision,
        op.result,
        op.recorded_at,
        plan,
        admission,
    ))
    .map_err(|_| corrupt_storage())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(stock::capacity());
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.correction.source.result.v1\0");
    hash.update(
        u64::try_from(bytes.len())
            .map_err(|_| corrupt_storage())?
            .to_be_bytes(),
    );
    hash.update(bytes);
    Ok(hash.finalize().into())
}

pub(super) async fn load_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    hub: HubId,
) -> Result<Ledger, SafeError> {
    let mut reader = OriginalReader::new(tx);
    Box::pin(load_with_proofs_on(&mut reader, hub)).await
}

pub(super) async fn load_with_proofs_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
) -> Result<Ledger, SafeError> {
    Box::pin(load_through_on(tx, hub, None)).await
}

/// Select the ordered native prefix before reconstructing any context, target,
/// population or document. Future complete originals are not old dependencies.
pub(super) async fn load_through_on(
    tx: &mut OriginalReader<'_, '_>,
    hub: HubId,
    cut: Option<HubRevision>,
) -> Result<Ledger, SafeError> {
    let through = cut
        .map(|v| i64::try_from(v.value()).map_err(|_| corrupt_storage()))
        .transpose()?;
    let invalid:i64=sqlx::query_scalar(concat!(
        "SELECT (SELECT count(*) FROM hub_corrections c LEFT JOIN hub_stock_operations o ON o.operation_id=c.operation_id WHERE c.family='source' AND (o.operation_id IS NULL OR o.hub_id<>?1 OR (?2 IS NULL OR o.stock_revision<=?2)) AND (o.operation_id IS NULL OR o.kind<>'correction_source' OR o.hub_id<>?1 OR o.result_id<>c.correction_id)) + ",
        "(SELECT count(*) FROM hub_stock_operations o LEFT JOIN hub_corrections c ON c.operation_id=o.operation_id WHERE o.kind='correction_source' AND (o.hub_id<>?1 OR (?2 IS NULL OR o.stock_revision<=?2)) AND (o.hub_id<>?1 OR c.family IS NULL OR c.family<>'source'))"
    )).bind(hub.as_bytes().as_slice()).bind(through).fetch_one(&mut **tx).await.map_err(|_|storage_unavailable())?;
    let ids:Vec<String>=sqlx::query_scalar("SELECT substr(operation_id,1,37) FROM hub_stock_operations WHERE kind='correction_source' AND (? IS NULL OR stock_revision<=?) ORDER BY stock_revision LIMIT 4097").bind(through).bind(through).fetch_all(&mut **tx).await.map_err(|_|storage_unavailable())?;
    if invalid != 0 || ids.len() > harvestcircle_domain::hub::corrections::HUB_CORRECTION_ROWS {
        return Err(corrupt_storage());
    }
    let populations = match cut {
        Some(_) => {
            Box::pin(super::attribution_review::registry::populations_through_on(
                tx, hub, cut,
            ))
            .await?
        }
        None => Box::pin(super::attribution_review::registry::populations_on(tx, hub)).await?,
    };
    let mut ledger = Ledger {
        populations,
        records: Vec::with_capacity(ids.len()),
        issued_versions: match cut {
            Some(_) => Box::pin(super::source_documents::load_through_on(tx, hub, cut)).await?,
            None => Box::pin(super::source_documents::load_on(tx, hub)).await?,
        },
    };
    for id in ids {
        let id = DurableRequestId::parse(id).map_err(|_| corrupt_storage())?;
        let op = stock::read::operation_on(tx, hub, &id)
            .await?
            .ok_or_else(corrupt_storage)?;
        let row=sqlx::query(concat!(
            "SELECT substr(correction_id,1,17) AS correction_id,family,subject_kind,substr(subject_id,1,17) AS subject_id,",
            "substr(target_operation_id,1,37) AS target,substr(previous_operation_id,1,37) AS previous,account_operation_id,account_previous_operation_id,report_id,observation_operation_id,",
            "payload_schema,substr(payload,1,4097) AS payload,substr(result_hash,1,33) AS result_hash FROM hub_corrections WHERE operation_id=?"
        )).bind(id.as_str()).fetch_optional(&mut **tx).await.map_err(|_|storage_unavailable())?.ok_or_else(corrupt_storage)?;
        let payload: Vec<u8> = row.try_get("payload").map_err(|_| corrupt_storage())?;
        let command = super::source_payload::decode(&payload)?;
        let own = crate::hub::actuals::common::operation_id(&id).map_err(|_| corrupt_storage())?;
        let prior: Option<String> = row.try_get("previous").map_err(|_| corrupt_storage())?;
        let expected_prior = command
            .expected_prior
            .map(crate::hub::actuals::common::operation)
            .transpose()
            .map_err(|_| corrupt_storage())?;
        if row
            .try_get::<String, _>("family")
            .map_err(|_| corrupt_storage())?
            != "source"
            || row
                .try_get::<String, _>("subject_kind")
                .map_err(|_| corrupt_storage())?
                != "population"
            || row
                .try_get::<i64, _>("payload_schema")
                .map_err(|_| corrupt_storage())?
                != 1
            || [
                "account_operation_id",
                "account_previous_operation_id",
                "observation_operation_id",
            ]
            .iter()
            .any(|name| {
                row.try_get::<Option<String>, _>(*name)
                    .map_or(true, |v| v.is_some())
            })
            || row
                .try_get::<Option<Vec<u8>>, _>("report_id")
                .map_err(|_| corrupt_storage())?
                .is_some()
            || stock::read::record_id(&row, "correction_id")? != own
            || op.result != own
            || stock::read::record_id(&row, "subject_id")? != command.trace
            || command.expected_stock_revision.value().checked_add(1) != Some(op.revision.value())
            || command.expected_catalog_revision != op.catalog_revision
            || prior.as_deref() != expected_prior.as_ref().map(DurableRequestId::as_str)
            || command.fingerprint(hub, op.actor, own) != op.hash
        {
            return Err(corrupt_storage());
        }
        let context = Box::pin(super::source_context::at_with_proofs_on(
            tx,
            hub,
            &ledger,
            command.trace,
            command.expected_stock_revision,
            command.expected_catalog_revision,
        ))
        .await
        .map_err(|_| corrupt_storage())?;
        let target = crate::hub::actuals::common::operation(context.original.operation)
            .map_err(|_| corrupt_storage())?;
        if row
            .try_get::<String, _>("target")
            .map_err(|_| corrupt_storage())?
            != target.as_str()
        {
            return Err(corrupt_storage());
        }
        let plan = command
            .plan(&context.domain(), own, op.actor, op.recorded_at)
            .map_err(|_| corrupt_storage())?;
        let material = [HubSubmittedMaterial::SourceAttribution(Box::new(
            context.material(own),
        ))];
        let admission = crate::hub::submission::verify_recorded_capture_on(
            tx,
            hub,
            &op,
            HubSubmittedRequest::SourceAttribution(&command),
            context.revision,
            context.catalog.revision(),
            &material,
        )
        .await?;
        let stored: Vec<u8> = row.try_get("result_hash").map_err(|_| corrupt_storage())?;
        if stored.as_slice() != digest(&op, &plan, admission)? {
            return Err(corrupt_storage());
        }
        super::found_impact::verify_on(tx, &id, &plan.impact).await?;
        ledger.records.push(plan);
    }
    // Raw registry rows are compared against every verified association and its
    // first population or source-correction creator; no orphan identity is valid.
    match cut {
        Some(_) => {
            super::attribution_review::registry::verify_through_on(
                tx,
                hub,
                ledger.populations.clone(),
                &ledger.records,
                cut,
            )
            .await?
        }
        None => {
            super::attribution_review::registry::verify_on(
                tx,
                hub,
                ledger.populations.clone(),
                &ledger.records,
            )
            .await?
        }
    };
    Ok(ledger)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HubSourceAttributionReceipt {
    pub operation: DurableRequestId,
    pub original: SourceAttributionCorrection,
}
impl Database {
    pub async fn hub_source_attribution_original(
        &self,
        actor: PublicKey,
        operation: DurableRequestId,
    ) -> Result<Option<HubSourceAttributionReceipt>, SafeError> {
        self.host()
            .transaction(|tx| {
                Box::pin(async move {
                    let hub = reference::authorize_on(tx, actor).await?;
                    reference::validate_basis_on(tx, hub.id()).await?;
                    let Some(op) = stock::read::operation_on(tx, hub.id(), &operation).await?
                    else {
                        return Ok(None);
                    };
                    if op.kind != "correction_source" {
                        return Err(stock::conflict());
                    }
                    let ledger = Box::pin(load_on(tx, hub.id())).await?;
                    let own = crate::hub::actuals::common::operation_id(&operation)?;
                    let original = ledger
                        .records
                        .into_iter()
                        .find(|r| r.stamp.operation == own)
                        .ok_or_else(corrupt_storage)?;
                    Ok(Some(HubSourceAttributionReceipt {
                        operation,
                        original,
                    }))
                })
            })
            .await
            .map_err(map_transaction_error)
    }
}
